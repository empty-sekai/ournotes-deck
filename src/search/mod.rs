//! Exact Top-K deck search.
//!
//! A search returns the best decks under one objective, one result per set of five member cards; each result is
//! the best representative of its set (leader, snaps, performance order) under the canonical order documented in
//! `docs/search.md`. A [`Completion::Complete`] outcome is exactly the canonical Top-K; a
//! [`Completion::TimedOut`] outcome holds legal, exactly evaluated decks but proves nothing about their rank.

pub mod oracle;
pub mod pool;

mod live;
mod matching;
mod power;
mod snaps;
mod tables;
mod topk;

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::live::model::{JudgementStream, LiveModel, Play};
use crate::live::score::{ComboTable, LiveScoreSettings, get_music_score_level_factor};
use crate::live::skip::{Chart, SkipEvaluator, skip_score};
use crate::master::Master;

pub use pool::{Deck, Pool};
pub use power::PowerStats;

use power::{Allowed, LiveMode, PowerSearch};
use snaps::FullSetup;
use tables::Tables;
use topk::TopK;

/// What to maximise.
#[derive(Clone, Debug)]
pub enum Objective {
    /// Deck power, song-less when `music_id` is `None`; with `event`, the held events' parameter bonuses count.
    Power { music_id: Option<i64>, event: bool },
    /// Skip score of one chart (`score_id` is its `MasterLiveMusicScore` id).
    SkipScore { score_id: i64, chart: Chart },
    /// Score of a played live, Gekisou off. With `exclude_snap_skills` the value is the score without snap skills
    /// under a per-note play ([`PlayInput::Notes`], per-order model); without it, the score of the whole-live
    /// simulation with live and snap skills under a judgement stream ([`PlayInput::Stream`]). `event`: whether the
    /// deck power includes the held events' parameter bonuses.
    LiveScore { score_id: i64, chart: Chart, play: PlayInput, event: bool, exclude_snap_skills: bool },
}

/// The play a live objective scores.
#[derive(Clone, Debug, PartialEq)]
pub enum PlayInput {
    /// Every judged note with its judgement, life and combo; for `exclude_snap_skills`.
    Notes(Play),
    /// A judgement stream for the whole-live simulation; for the objective with snap skills. `judgement_types[i]` is
    /// the note judgement type of `chart.notes[i]`.
    Stream { stream: JudgementStream, judgement_types: Vec<i32> },
}

/// Hard constraints on the decks considered.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Constraints {
    /// Member card id that must be the leader.
    pub leader: Option<i64>,
    /// Member card ids every deck must contain.
    pub include_members: Vec<i64>,
    pub exclude_members: Vec<i64>,
    pub exclude_snaps: Vec<i64>,
    /// Decks without snaps.
    pub no_snaps: bool,
}

#[derive(Clone, Debug)]
pub struct SearchRequest {
    pub objective: Objective,
    /// Number of results.
    pub k: usize,
    pub constraints: Constraints,
    /// Wall-clock limit; `None` runs to completion.
    pub time_limit: Option<Duration>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Completion {
    /// The results are exactly the canonical Top-K.
    Complete,
    /// The time limit was reached; the results are legal and exactly evaluated but unproven.
    TimedOut,
}

/// One result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedDeck {
    /// Member card ids in slot order; slot 2 is the leader.
    pub members: [i64; 5],
    /// Snap id of each slot.
    pub snaps: [Option<i64>; 5],
    /// Performance order: `performance_order[k]` is the slot performing at position k.
    pub performance_order: [usize; 5],
    /// Deck power under the objective's song and event setting.
    pub power: i32,
    /// Skip or live score, for score objectives.
    pub score: Option<i32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchOutcome {
    pub completion: Completion,
    pub results: Vec<RankedDeck>,
    pub stats: PowerStats,
    pub elapsed: Duration,
}

fn resolve_allowed(pool: &Pool, c: &Constraints) -> Result<(Allowed, Vec<usize>), Error> {
    let idx =
        |id: i64| pool.member_index(id).ok_or_else(|| Error::Input(format!("member card {id} is not in the roster")));
    let mut members = vec![true; pool.members.len()];
    for &id in &c.exclude_members {
        members[idx(id)?] = false;
    }
    let mut required = Vec::new();
    for &id in &c.include_members {
        let i = idx(id)?;
        if !members[i] {
            return Err(Error::Input(format!("member card {id} is both required and excluded")));
        }
        if !required.contains(&i) {
            required.push(i);
        }
    }
    let leader = match c.leader {
        None => None,
        Some(id) => {
            let i = idx(id)?;
            if !members[i] {
                return Err(Error::Input(format!("member card {id} is both the leader and excluded")));
            }
            Some(i)
        }
    };
    let mut snaps: Vec<usize> = Vec::new();
    if !c.no_snaps {
        for (i, s) in pool.snaps.iter().enumerate() {
            if !c.exclude_snaps.contains(&s.id) {
                snaps.push(i);
            }
        }
        for &id in &c.exclude_snaps {
            pool.snap_index(id).ok_or_else(|| Error::Input(format!("snap {id} is not in the roster")))?;
        }
    }
    snaps.sort_by_key(|&i| pool.snaps[i].id);
    Ok((Allowed { members, required, leader }, snaps))
}

/// The song (live music) that owns a chart id.
pub fn music_of_score(master: &Master, score_id: i64) -> Result<i64, Error> {
    master
        .live_musics
        .iter()
        .find(|m| [m.easy_id, m.normal_id, m.hard_id, m.expert_id].contains(&score_id))
        .map(|m| m.id)
        .ok_or_else(|| Error::Input(format!("no live music has chart {score_id}")))
}

/// Settings of the skip score, and the checks that make it non-decreasing in deck power.
struct SkipModel<'c> {
    level: i32,
    chart: &'c Chart,
    settings: LiveScoreSettings,
    valid: Vec<i32>,
    combo: ComboTable,
    fast: SkipEvaluator,
}

impl<'c> SkipModel<'c> {
    fn new(master: &Master, score_id: i64, chart: &'c Chart) -> Result<SkipModel<'c>, Error> {
        let row = master.live_music_score(score_id).ok_or_else(|| Error::Input(format!("unknown chart {score_id}")))?;
        let settings = LiveScoreSettings::from_master(master)?;
        let combo = ComboTable::from_master(master)?;
        let valid = settings.valid_note_types();
        let level = row.music_score_level as i32;
        let positive = |x: f32| x.is_finite() && x > 0.0;
        let great = settings.judgement_score_factor_percent.get(&crate::live::score::GREAT).copied().unwrap_or(-1);
        if !positive(settings.score_adjustment_factor)
            || !positive(get_music_score_level_factor(level))
            || chart.converted_note_count <= 0
            || great < 0
            || valid.iter().any(|t| settings.note_factor_percent[t] < 0)
            || combo.get_cumulative_factor(crate::live::score::COMBO, 0)? < 0.0
        {
            return Err(Error::Domain("skip score settings are not all positive".into()));
        }
        let fast = SkipEvaluator::new(level, chart, &settings, &valid, Some(&combo))?;
        Ok(SkipModel { level, chart, settings, valid, combo, fast })
    }

    fn score(&self, power: i32) -> Result<i32, Error> {
        Ok(self.fast.score(power).0)
    }

    /// The skip score through the general calculator (paired with `score`).
    fn score_reference(&self, power: i32) -> Result<i32, Error> {
        skip_score(power, self.level, self.chart, &self.settings, &self.valid, Some(&self.combo))
    }
}

/// Song, event flag and skip model of an objective.
fn objective_song<'c>(pool: &Pool, o: &'c Objective) -> Result<(Option<i64>, bool, Option<SkipModel<'c>>), Error> {
    Ok(match o {
        Objective::Power { music_id, event } => (*music_id, *event, None),
        Objective::SkipScore { score_id, chart } => {
            (Some(music_of_score(pool.master, *score_id)?), false, Some(SkipModel::new(pool.master, *score_id, chart)?))
        }
        Objective::LiveScore { score_id, event, exclude_snap_skills, play, .. } => {
            match (exclude_snap_skills, play) {
                (true, PlayInput::Notes(_)) | (false, PlayInput::Stream { .. }) => {}
                (true, _) => return Err(Error::Input("a live score without snap skills needs a per-note play".into())),
                (false, _) => {
                    return Err(Error::Input("a live score with snap skills needs a judgement stream".into()));
                }
            }
            (Some(music_of_score(pool.master, *score_id)?), *event, None)
        }
    })
}

fn live_model(pool: &Pool, o: &Objective) -> Result<Option<LiveModel>, Error> {
    let Objective::LiveScore { score_id, chart, play: PlayInput::Notes(play), exclude_snap_skills: true, .. } = o
    else {
        return Ok(None);
    };
    let row =
        pool.master.live_music_score(*score_id).ok_or_else(|| Error::Input(format!("unknown chart {score_id}")))?;
    Ok(Some(LiveModel::new(pool.master, row.music_score_level as i32, chart, play)?))
}

/// The whole-live simulation setup of a live objective with snap skills.
fn full_setup(pool: &Pool, o: &Objective) -> Result<Option<FullSetup>, Error> {
    let Objective::LiveScore {
        score_id,
        chart,
        play: PlayInput::Stream { stream, judgement_types },
        exclude_snap_skills: false,
        ..
    } = o
    else {
        return Ok(None);
    };
    let row =
        pool.master.live_music_score(*score_id).ok_or_else(|| Error::Input(format!("unknown chart {score_id}")))?;
    Ok(Some(FullSetup::new(pool.master, row.music_score_level as i32, chart, stream, judgement_types)?))
}

/// Deck power and score of one deck under an objective (a live score through the general calculator, or through the
/// whole-live simulation with snap skills).
pub fn evaluate(pool: &Pool, deck: &Deck, objective: &Objective) -> Result<(i32, Option<i32>), Error> {
    let (music_id, event, skip) = objective_song(pool, objective)?;
    let song = music_id.map(|id| pool.song(id)).transpose()?;
    let power = pool.deck_power(deck, song.as_ref(), event)?.power();
    if let Some(m) = skip {
        return Ok((power, Some(m.score(power)?)));
    }
    if let Some(setup) = full_setup(pool, objective)? {
        return Ok((power, Some(setup.score(pool.master, &snaps::deck_performers(pool, deck)?, power)?)));
    }
    let Some(model) = live_model(pool, objective)? else { return Ok((power, None)) };
    let Objective::LiveScore { play: PlayInput::Notes(play), .. } = objective else { unreachable!() };
    let perf: Vec<(i64, i64)> = deck
        .performance_order
        .iter()
        .map(|&slot| {
            let m = &pool.members[deck.members[slot]];
            (m.live_skill_id, m.live_skill_level)
        })
        .collect();
    let cmds = model.commands(pool.master, &perf)?;
    Ok((power, Some(model.score_reference(power, play, &cmds)?)))
}

/// Runs a search.
pub fn search(pool: &Pool, req: &SearchRequest) -> Result<SearchOutcome, Error> {
    let t0 = Instant::now();
    let (allowed, snaps) = resolve_allowed(pool, &req.constraints)?;
    let (music_id, event, skip) = objective_song(pool, &req.objective)?;
    let song = music_id.map(|id| pool.song(id)).transpose()?;
    let t = Tables::new(pool, song.clone(), event, &snaps)?;
    let live_model = live_model(pool, &req.objective)?;
    let live_ctx = match &live_model {
        None => None,
        Some(m) => Some(live::LiveCtx::new(pool.master, pool, m)?),
    };
    let setup = full_setup(pool, &req.objective)?;
    let snap_live = match &setup {
        None => None,
        Some(s) => Some(snaps::SnapLive::new(pool, &t, &allowed.members, s)?),
    };
    let mode = match (&live_ctx, &snap_live) {
        (Some(l), _) => LiveMode::Order(l),
        (None, Some(s)) => LiveMode::Snaps(s),
        (None, None) => LiveMode::None,
    };
    let mut s = PowerSearch {
        pool,
        t: &t,
        allowed: &allowed,
        top: TopK::new(req.k),
        deadline: req.time_limit.map(|d| t0 + d),
        timed_out: false,
        stats: PowerStats::default(),
        error: None,
        live: mode,
        gains: Vec::new(),
    };
    s.run();
    if let Some(e) = s.error {
        return Err(e);
    }
    let completion = if s.timed_out { Completion::TimedOut } else { Completion::Complete };
    let mut results = Vec::new();
    for e in s.top.into_vec() {
        let deck = Deck { members: e.members, snaps: e.snaps, performance_order: e.order };
        let full = pool.deck_power(&deck, song.as_ref(), event)?;
        if full.power() as i64 != e.power {
            return Err(Error::Domain(format!(
                "decomposed power {} differs from the full evaluation {}",
                e.power,
                full.power()
            )));
        }
        let score = match (&skip, &live_ctx, &req.objective) {
            _ if setup.is_some() => {
                let s = setup.as_ref().expect("setup");
                let v = s.score(pool.master, &snaps::deck_performers(pool, &deck)?, full.power())?;
                if v as i64 != e.value {
                    return Err(Error::Domain(format!(
                        "live score {} differs from the simulation of the deck {v}",
                        e.value
                    )));
                }
                Some(v)
            }
            (Some(m), _, _) => {
                let v = m.score_reference(full.power())?;
                if v != m.score(full.power())? {
                    return Err(Error::Domain("skip score evaluators disagree".into()));
                }
                Some(v)
            }
            (None, Some(lc), Objective::LiveScore { play: PlayInput::Notes(play), .. }) => {
                let v = lc.model.score_reference(full.power(), play, &lc.commands(&e.members, &e.order))?;
                if v as i64 != e.value {
                    return Err(Error::Domain(format!(
                        "live score {} differs from the reference evaluation {v}",
                        e.value
                    )));
                }
                Some(v)
            }
            _ => None,
        };
        results.push(RankedDeck {
            members: e.members.map(|i| pool.members[i].id),
            snaps: e.snaps.map(|s| s.map(|i| pool.snaps[i].id)),
            performance_order: e.order,
            power: full.power(),
            score,
        });
    }
    if let (Some(m), Some(best)) = (&skip, results.first()) {
        // the skip ranking equals the power ranking only while the per-note sum does not wrap
        let wide = m.fast.score(best.power).1;
        if wide > i32::MAX as i64 {
            return Err(Error::Domain("skip score exceeds the 32-bit range".into()));
        }
    }
    Ok(SearchOutcome { completion, results, stats: s.stats, elapsed: t0.elapsed() })
}
