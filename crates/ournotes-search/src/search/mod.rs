//! Power/skip Top-K search and explicit native-root expectation search.
//! Played native objectives use [`expectation::oracle`]; order optimization is diagnostic only.
//!
//! A search returns the best decks under one objective, one result per set of five member cards; each result is
//! the best representative of its set (leader, snaps, performance order) under the canonical order documented in
//! `docs/search.md`. A [`Completion::Complete`] outcome is exactly the canonical Top-K; a
//! [`Completion::TimedOut`] outcome holds legal, exactly evaluated decks but proves nothing about their rank.

pub(crate) mod dispatch;
pub use dispatch::recommend_built;
pub mod expectation;
pub mod oracle;
pub(crate) mod physical;
pub(crate) use physical::session_start_clock;
pub use physical::{
    GOAL_SPEC_VERSION, GoalSpec, SESSION_FORMAT, SearchSession, SessionBinding, SessionProgress, SessionStatus,
    StepBudget, evaluate_declared_context, score_summary, solve_physical,
};

mod budget;
#[cfg(feature = "search-diagnostics")]
pub mod diagnostics;
pub(crate) mod joint;
mod live;
mod luck;
mod matching;
mod power;
mod snaps;
mod tables;
pub mod telemetry;
mod topk;

use crate::clock::Instant;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use ournotes_sim::cards::SongView;
use ournotes_sim::error::Error;
use ournotes_sim::live::model::{JudgementStream, JustRule, LiveModel, Play};
use ournotes_sim::live::score::{ComboTable, LiveScoreSettings, get_music_score_level_factor};
use ournotes_sim::live::skip::{Chart, SkipEvaluator, skip_score};
use ournotes_sim::master::Master;
use ournotes_sim::scenario::ResolvedContext;

use ournotes_sim::pool::{Deck, Pool};
pub use power::PowerStats;
pub use snaps::{ablate, set_bound_ablation};

use budget::SearchBudget;
use power::{Allowed, LiveMode, PowerSearch};
use snaps::FullSetup;
use tables::Tables;

/// What to maximise.
#[derive(Clone, Debug)]
pub enum Objective {
    /// Explicit client scenario; inner objective selects the scoring algorithm, not the song view.
    InScenario { context: Box<ResolvedContext>, objective: Box<Objective> },
    /// Deck power, song-less when `music_id` is `None`; with `event`, the held events' parameter bonuses count.
    Power { music_id: Option<i64>, event: bool },
    /// Skip score of one chart (`score_id` is its `MasterLiveMusicScore` id).
    SkipScore { score_id: i64, chart: Chart },
    /// Score of a played live. With `exclude_snap_skills` the value is the score without snap skills under a
    /// per-note play ([`PlayInput::Notes`], per-order model); without it, the score of the whole-live simulation
    /// with live and snap skills under a judgement stream ([`PlayInput::Stream`]). `event`: whether the deck power
    /// includes the held events' parameter bonuses. `gekisou`: `None` plays the live with Gekisou off; `Some` plays
    /// it with Gekisou on and ranks by the sum of the scores over a seed set (see [`GekisouObjective`]).
    LiveScore {
        score_id: i64,
        chart: Chart,
        play: PlayInput,
        event: bool,
        exclude_snap_skills: bool,
        gekisou: Option<GekisouObjective>,
    },
}

impl Objective {
    pub fn in_scenario(self, context: ResolvedContext) -> Self {
        Self::InScenario { context: Box::new(context), objective: Box::new(self) }
    }
    pub(crate) fn inner(&self) -> &Self {
        match self {
            Self::InScenario { objective, .. } => objective.inner(),
            _ => self,
        }
    }
    pub fn context(&self) -> Option<&ResolvedContext> {
        match self {
            Self::InScenario { context, .. } => Some(context),
            _ => None,
        }
    }
}

/// The seeds a live with Gekisou on is played with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SeedSet {
    /// The first `n` seeds of the published sequence ([`ournotes_sim::live::seeds::published_seeds`]).
    Published(usize),
    /// These seeds, in this order.
    List(Vec<i32>),
}

impl SeedSet {
    /// The seeds, in order; an Input error for an empty set.
    pub fn seeds(&self) -> Result<Vec<i32>, Error> {
        let v = match self {
            SeedSet::Published(n) => ournotes_sim::live::seeds::published_seeds(*n),
            SeedSet::List(v) => v.clone(),
        };
        if v.is_empty() {
            return Err(Error::Input("an empty seed set".into()));
        }
        Ok(v)
    }
}

/// The live objective with Gekisou on. Each deck is simulated once per seed of `seeds` (the play's random seed set
/// to it), each exactly; its value is the sum of these scores (equivalently, their mean), rank bonuses included.
/// `fevers` are the chart's fever ranges `(start, end)` in ms, sorted by start (the deck data file's `fevers`); the
/// Gekisou missions come from the song's master row.
#[derive(Clone, Debug, PartialEq)]
pub struct GekisouObjective {
    pub seeds: SeedSet,
    pub fevers: Vec<(i32, i32)>,
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
    /// Skip or live score, for score objectives; with Gekisou on, the floor of the mean over the seed set (for
    /// display: the ranking uses `score_sum`).
    pub score: Option<i32>,
    /// With Gekisou on: the sum of the live scores over the seed set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_sum: Option<i64>,
    /// With Gekisou on: the live score of each seed, in seed-set order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed_scores: Option<Vec<i32>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchOutcome {
    pub completion: Completion,
    pub results: Vec<RankedDeck>,
    pub stats: PowerStats,
    /// Wall-clock time of the whole call, the final verification included.
    pub elapsed: Duration,
    /// With Gekisou on: the seed set used, in order.
    pub seeds: Option<Vec<i32>>,
    /// Time of the final verification of the results (every result evaluated again; with Gekisou on, simulated
    /// again on every seed). It shares the request deadline; only completed
    /// regular-path verifications are returned.
    pub verify_elapsed: Duration,
    /// For the live objective with snap skills: the number of snap classes of each allowed member card
    /// `(card id, classes)`, in pool order (class 0, the snaps that change nothing, included).
    pub classes: Vec<(i64, u32)>,
}

pub(crate) fn resolve_allowed(pool: &Pool, c: &Constraints) -> Result<(Allowed, Vec<usize>), Error> {
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
pub(crate) struct SkipModel<'c> {
    level: i32,
    chart: &'c Chart,
    settings: LiveScoreSettings,
    valid: Vec<i32>,
    combo: ComboTable,
    pub(crate) fast: SkipEvaluator,
}

impl<'c> SkipModel<'c> {
    fn new(master: &Master, score_id: i64, chart: &'c Chart) -> Result<SkipModel<'c>, Error> {
        let row = master.live_music_score(score_id).ok_or_else(|| Error::Input(format!("unknown chart {score_id}")))?;
        let settings = LiveScoreSettings::from_master(master)?;
        let combo = ComboTable::from_master(master)?;
        let valid = settings.valid_note_types();
        let level = row.music_score_level as i32;
        let positive = |x: f32| x.is_finite() && x > 0.0;
        let great =
            settings.judgement_score_factor_percent.get(&ournotes_sim::live::score::GREAT).copied().unwrap_or(-1);
        let combo_zero = combo.get_cumulative_factor(ournotes_sim::live::score::COMBO, 0)?;
        if !positive(settings.score_adjustment_factor)
            || !positive(get_music_score_level_factor(level))
            || chart.converted_note_count <= 0
            || great < 0
            || valid.iter().any(|t| settings.note_factor_percent[t] < 0)
            || !combo_zero.is_finite()
            || combo_zero < 0.0
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

fn validate_live_input(o: &Objective) -> Result<(), Error> {
    if let Objective::LiveScore { gekisou, exclude_snap_skills, play, .. } = o {
        if gekisou.is_some() {
            if *exclude_snap_skills {
                return Err(Error::Input("a live score with Gekisou on counts snap skills".into()));
            }
            if let PlayInput::Stream { stream, .. } = play
                && stream.base_seed != 0
            {
                return Err(Error::Input(
                    "a live score with Gekisou on takes its seeds from the seed set, not the stream".into(),
                ));
            }
        }
        match (exclude_snap_skills, play) {
            (true, PlayInput::Notes(_)) | (false, PlayInput::Stream { .. }) => {}
            (true, _) => return Err(Error::Input("a live score without snap skills needs a per-note play".into())),
            (false, _) => {
                return Err(Error::Input("a live score with snap skills needs a judgement stream".into()));
            }
        }
    }
    Ok(())
}

/// Song, event flag and skip model of an objective.
pub(crate) fn objective_song<'c>(
    pool: &Pool,
    o: &'c Objective,
) -> Result<(Option<SongView>, bool, Option<SkipModel<'c>>), Error> {
    if let Objective::InScenario { context, objective } = o {
        if objective.context().is_some() {
            return Err(Error::Input("nested scenario objectives are not supported".into()));
        }
        context.validate_pool(pool)?;
        match objective.as_ref() {
            Objective::Power { music_id, .. } => {
                if music_id.is_some_and(|id| id != context.resolved.live_music_id) {
                    return Err(Error::Input("power music differs from resolved base song".into()));
                }
            }
            Objective::SkipScore { score_id, .. } | Objective::LiveScore { score_id, .. } => {
                if context.score_id != Some(*score_id) {
                    return Err(Error::Input("objective chart differs from resolved context".into()));
                }
            }
            _ => unreachable!(),
        }
        if matches!(objective.as_ref(), Objective::SkipScore { .. })
            && !matches!(
                context.scenario,
                ournotes_sim::scenario::Scenario::Free(_) | ournotes_sim::scenario::Scenario::Challenge(_)
            )
        {
            return Err(Error::Input("client skip is only verified for Free and Challenge".into()));
        }
        if matches!(
            (objective.as_ref(), context.result_clock),
            (Objective::LiveScore { .. }, Some(ournotes_sim::event::EventResultClock::Skip { .. }))
                | (Objective::SkipScore { .. }, Some(ournotes_sim::event::EventResultClock::Played { .. }))
        ) {
            return Err(Error::Input("resolved result clock execution differs from objective".into()));
        }
        validate_live_input(objective)?;
        let skip = match objective.as_ref() {
            Objective::SkipScore { score_id, chart } => Some(SkipModel::new(pool.master, *score_id, chart)?),
            _ => None,
        };
        return Ok((Some(context.resolved.power_music.clone()), context.resolved.calc_event_parameter, skip));
    }
    Ok(match o {
        Objective::InScenario { .. } => unreachable!(),
        Objective::Power { music_id, event } => (music_id.map(|id| pool.song(id)).transpose()?, *event, None),
        Objective::SkipScore { score_id, chart } => (
            Some(pool.song(music_of_score(pool.master, *score_id)?)?),
            false,
            Some(SkipModel::new(pool.master, *score_id, chart)?),
        ),
        Objective::LiveScore { score_id, event, .. } => {
            validate_live_input(o)?;
            (Some(pool.song(music_of_score(pool.master, *score_id)?)?), *event, None)
        }
    })
}

fn live_model(pool: &Pool, o: &Objective) -> Result<Option<LiveModel>, Error> {
    let Objective::LiveScore { score_id, chart, play: PlayInput::Notes(play), exclude_snap_skills: true, .. } =
        o.inner()
    else {
        return Ok(None);
    };
    let row =
        pool.master.live_music_score(*score_id).ok_or_else(|| Error::Input(format!("unknown chart {score_id}")))?;
    Ok(Some(LiveModel::new(pool.master, row.music_score_level as i32, chart, play)?))
}

/// The whole-live simulation setup of a live objective with snap skills.
pub(crate) fn full_setup(pool: &Pool, o: &Objective) -> Result<Option<FullSetup>, Error> {
    let Objective::LiveScore {
        score_id,
        chart,
        play: PlayInput::Stream { stream, judgement_types },
        exclude_snap_skills: false,
        gekisou,
        ..
    } = o.inner()
    else {
        return Ok(None);
    };
    let row =
        pool.master.live_music_score(*score_id).ok_or_else(|| Error::Input(format!("unknown chart {score_id}")))?;
    let mut setup = FullSetup::new(pool.master, row.music_score_level as i32, chart, stream, judgement_types)?;
    setup.params.skill_target_music_type = match o.context() {
        Some(context) => context.resolved.skill_target_music_type,
        None => {
            ournotes_sim::scenario::Scenario::Free(music_of_score(pool.master, *score_id)?)
                .resolve(pool.master)?
                .skill_target_music_type
        }
    };
    if let Some(g) = gekisou {
        let gs = if let Some(context) = o.context() {
            if context.gekisou.fevers != g.fevers {
                return Err(Error::Input("objective fever ranges differ from resolved context".into()));
            }
            context.gekisou.clone()
        } else {
            let scenario = ournotes_sim::scenario::Scenario::Free(music_of_score(pool.master, *score_id)?);
            ResolvedContext::resolve(pool.master, scenario, Some(*score_id), &g.fevers, pool.player.events.clone())?
                .gekisou
        };
        let rule = JustRule::new(pool.master, &gs)?;
        stream.check_just(chart, judgement_types, &rule)?;
        let dt = stream.delta_times()?;
        setup.set_gekisou(gs, dt, g.seeds.seeds()?);
    }
    Ok(Some(setup))
}

/// Deck power and score of one deck under an objective (a live score through the general calculator, or through the
/// whole-live simulation with snap skills; with Gekisou on, the floor of the mean over the seed set).
pub fn evaluate(pool: &Pool, deck: &Deck, objective: &Objective) -> Result<(i32, Option<i32>), Error> {
    let (song, event, skip) = objective_song(pool, objective)?;
    let power = pool.deck_power(deck, song.as_ref(), event)?.power();
    if let Some(m) = skip {
        return Ok((power, Some(m.score(power)?)));
    }
    if let Some(setup) = full_setup(pool, objective)? {
        let perf = snaps::deck_performers(pool, deck)?;
        if setup.gk.is_some() {
            let v = setup.seed_scores(pool.master, &perf, power)?;
            return Ok((power, Some(mean_floor(&v))));
        }
        return Ok((power, Some(setup.score(pool.master, &perf, power)?)));
    }
    let Some(model) = live_model(pool, objective)? else { return Ok((power, None)) };
    let Objective::LiveScore { play: PlayInput::Notes(play), .. } = objective.inner() else { unreachable!() };
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

/// The live score of one deck on every seed of the objective's seed set, in order (`None` unless the objective is
/// the live score with Gekisou on). Each seed is an independent whole-live simulation.
pub fn seed_scores(pool: &Pool, deck: &Deck, objective: &Objective) -> Result<Option<Vec<i32>>, Error> {
    let (song, event, _) = objective_song(pool, objective)?;
    let Some(setup) = full_setup(pool, objective)? else { return Ok(None) };
    if setup.gk.is_none() {
        return Ok(None);
    }
    let power = pool.deck_power(deck, song.as_ref(), event)?.power();
    Ok(Some(setup.seed_scores(pool.master, &snaps::deck_performers(pool, deck)?, power)?))
}

/// The floor of the mean of seed scores.
pub(crate) fn mean_floor(v: &[i32]) -> i32 {
    let sum: i64 = v.iter().map(|&x| x as i64).sum();
    sum.div_euclid(v.len().max(1) as i64) as i32
}

/// Runs a search.
pub fn search(pool: &Pool, req: &SearchRequest) -> Result<SearchOutcome, Error> {
    if req.k == 0 {
        return Err(Error::Input("k must be positive".into()));
    }
    if matches!(req.objective.inner(), Objective::LiveScore { .. }) {
        return Err(Error::Input("live search requires search::expectation::oracle with an explicit root law; the order-optimized diagnostic is search_best_order_diagnostic".into()));
    }
    search_best_order_diagnostic(pool, req)
}

/// Historical best-order optimizer, not native MemberShuffle expectation.
pub fn search_best_order_diagnostic(pool: &Pool, req: &SearchRequest) -> Result<SearchOutcome, Error> {
    if req.k == 0 {
        return Err(Error::Input("k must be positive".into()));
    }
    let t0 = budget::now();
    let budget = SearchBudget::new(t0, req.time_limit)?;
    if budget.expired() {
        return Ok(preparation_timeout(t0));
    }
    let (allowed, snaps) = resolve_allowed(pool, &req.constraints)?;
    if budget.expired() {
        return Ok(preparation_timeout(t0));
    }
    let (song, event, skip) = objective_song(pool, &req.objective)?;
    if budget.expired() {
        return Ok(preparation_timeout(t0));
    }
    let Some(t) = Tables::new(pool, song.clone(), event, &snaps, budget)? else { return Ok(preparation_timeout(t0)) };
    if budget.expired() {
        return Ok(preparation_timeout(t0));
    }
    let live_model = live_model(pool, &req.objective)?;
    if budget.expired() {
        return Ok(preparation_timeout(t0));
    }
    let live_ctx = match &live_model {
        None => None,
        Some(m) => Some(live::LiveCtx::new(pool.master, pool, m)?),
    };
    if budget.expired() {
        return Ok(preparation_timeout(t0));
    }
    let setup = full_setup(pool, &req.objective)?;
    if budget.expired() {
        return Ok(preparation_timeout(t0));
    }
    let snap_live = match &setup {
        None => None,
        Some(s) => Some(snaps::SnapLive::new(pool, &t, &allowed.members, s)?),
    };
    if budget.expired() {
        return Ok(preparation_timeout(t0));
    }
    let mode = match (&live_ctx, &snap_live) {
        (Some(l), _) => LiveMode::Order(l),
        (None, Some(s)) => LiveMode::Snaps(s),
        (None, None) => LiveMode::None,
    };
    let mut s = PowerSearch::new(pool, &t, &allowed, req.k, budget, mode);
    if let Some(skip) = &skip {
        let Some(power_upper_bound) = s.power_upper_bound() else { return Ok(preparation_timeout(t0)) };
        if !t.prove_nonnegative_power(pool, &s.prepared_leaders(), &allowed, budget)?
            || !skip.fast.prove_monotone_through(power_upper_bound, || budget.expired())?
        {
            return Ok(preparation_timeout(t0));
        }
    }
    s.run();
    if let Some(e) = s.error {
        return Err(e);
    }
    let mut completion = if s.timed_out { Completion::TimedOut } else { Completion::Complete };
    let t_verify = Instant::now();
    let mut results = Vec::new();
    for e in s.top.into_vec() {
        #[cfg(test)]
        budget::test_clock::stage("verify-candidate");
        if budget.expired() {
            completion = Completion::TimedOut;
            break;
        }
        let deck = Deck { members: e.members, snaps: e.snaps, performance_order: e.order };
        let full = pool.deck_power(&deck, song.as_ref(), event)?;
        if full.power() as i64 != e.power {
            return Err(Error::Domain(format!(
                "decomposed power {} differs from the full evaluation {}",
                e.power,
                full.power()
            )));
        }
        let mut seeds_out = None;
        let score = match (&skip, &live_ctx, req.objective.inner()) {
            _ if setup.as_ref().is_some_and(|s| s.gk.is_some()) => {
                let s = setup.as_ref().expect("setup");
                let v = s.seed_scores(pool.master, &snaps::deck_performers(pool, &deck)?, full.power())?;
                let sum: i64 = v.iter().map(|&x| x as i64).sum();
                if sum != e.value {
                    return Err(Error::Domain(format!(
                        "live score sum {} differs from the simulation of the deck {sum}",
                        e.value
                    )));
                }
                let m = mean_floor(&v);
                seeds_out = Some(v);
                Some(m)
            }
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
            score_sum: seeds_out.as_ref().map(|_| e.value),
            seed_scores: seeds_out,
        });
        #[cfg(test)]
        budget::test_clock::stage("verify-finished");
        // A complete atomic verification may overrun. Keep that verified entry,
        // mark the budget exit and never begin another candidate afterwards.
        if budget.expired() {
            completion = Completion::TimedOut;
            break;
        }
    }
    let verify_elapsed = t_verify.elapsed();
    let seeds = setup.as_ref().and_then(|s| s.gk.as_ref()).map(|g| g.seeds.clone());
    let classes = snap_live.as_ref().map(|sl| sl.class_counts(pool)).unwrap_or_default();
    Ok(SearchOutcome { completion, results, stats: s.stats, elapsed: t0.elapsed(), seeds, verify_elapsed, classes })
}

fn preparation_timeout(start: Instant) -> SearchOutcome {
    SearchOutcome {
        completion: Completion::TimedOut,
        results: Vec::new(),
        stats: PowerStats::default(),
        elapsed: start.elapsed(),
        seeds: None,
        verify_elapsed: Duration::ZERO,
        classes: Vec::new(),
    }
}

#[cfg(test)]
mod gate_tests;
