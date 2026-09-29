//! Gekisou skills in the chart statistics: a chart's best formation for the page's default scenario, found on the
//! Gekisou windows of the simulation ([`crate::live::full::window_note_scores`]) and measured on the whole-live
//! simulation.
//!
//! A formation is five performers, each a member card with its Gekisou skill at the skill's highest level and
//! optionally a snap with its Gekisou support skill at the level of the snap's highest rank; the five members are
//! five different characters and a snap is used at most once. The default scenario is a Gekisou live (Battle Live) at
//! rank 1 in every range, the theoretical best play (every Just inside the Just-count ranges) and a deck of plain
//! score-up skills (the plain kind, [`plain_kind`], at factor 1 at every position); its figure per unit of deck power
//! is `score / power + sum_k weights[plain][k]`, and the search maximises the formation's gain of that figure averaged
//! over the seeds ([`GekisouStats::objective`]).
//!
//! The search evaluates a formation on the Gekisou windows only (the frames from each range's Start to its Finish,
//! where every Gekisou skill acts): the window's note scores are recomputed from the commands filed there, the range
//! scores and rank bonuses follow, and the plain skill's weights move by the change of the window notes' score per unit
//! of factor on the notes each position's skill covers. It fills the five performers greedily, then replaces one
//! performer's member or snap, or swaps two performers' snaps, while the figure improves. Member cards with the same
//! Gekisou skill are one candidate, snaps with the same support skill and level too; a support skill's band condition
//! (condition 5000 on its own member) splits a candidate by its result. Skills of missions the chart does not play never
//! act, so their members are fillers; support skills whose effects change no score on the best play (combo protection,
//! Great to Perfect, the Just window) are left out. The result is a local optimum of the screened seeds; the chosen
//! formation is then measured on the whole-live simulation on every seed ([`GekisouSeed`]).

use std::collections::HashMap;

use serde::Serialize;

use super::{
    Check, KIND_SKILL_BASE, Kind, Live, MAX_GEKISOU_FEVERS, POWER, RANKS, RangeInfo, RankCheck, Rng, SeedStats,
    UNIT_VALUE, check_deck, kind_factor,
};
use crate::error::Error;
use crate::live::full::{GekisouRange, LiveModel, LiveNote, NoteEval, Performer, WindowNote, window_note_scores};
use crate::live::score::{GekisouComboInfo, convert_score_type, get_frame};
use crate::live::skill::{FactorCommand, OWNER_MEMBER};
use crate::master::{GekisouSkillEffectRow, Master};

/// The number of seeds a luck chart's search screens formations on (the first seeds of its seed set).
pub const SCREEN_SEEDS: usize = 8;
/// Sweeps of the coordinate descent at most.
const MAX_SWEEPS: usize = 4;
/// Skill condition type of a member target: a support skill's condition on its own member.
const CONDITION_MEMBER_TARGET: i64 = 5000;
/// Support effect types that change no score on the theoretical best play (no Great, Bad or Miss): combo protection,
/// Great to Perfect, the Just window of a judged stream.
const ZERO_SUPPORT_TYPES: [i64; 3] = [12004, 12006, 4004];
/// Start states of the generators of the formation's check decks and ranks, each xored with the score id.
const GK_CHECK_SALT: u64 = 0x676b_5f63_6865_636b;
const GK_RANK_SALT: u64 = 0x676b_5f72_616e_6b73;
/// The method, for the document.
pub const SEARCH: &str = "greedy fill of the five performers, then coordinate descent (one performer's member or \
    snap replaced, or two performers' snaps swapped) while the default figure improves; member cards with the same \
    Gekisou skill are one candidate, snaps with the same support skill and level too, split by the support skill's \
    band condition on its member; members whose skill's mission the chart does not play are fillers, support skills \
    that change no score on the best play (12004, 12006, 4004) are left out; formations are evaluated on the Gekisou \
    windows of the simulation (the frames from each range's Start to its Finish), a luck chart on its first \
    screenSeeds seeds; the chosen formation is measured on the whole-live simulation on every seed. A local optimum, \
    not proven global, chosen for the default scenario only (rank 1 in every range, every Just, no Great, the plain \
    kind at factor 1 at every position)";

/// A skill and its level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct SkillLevel {
    pub id: i64,
    pub level: i64,
}

/// A member card of the catalog.
#[derive(Clone, Debug)]
pub struct CatalogMember {
    pub id: i64,
    pub character_id: i64,
    pub band_id: i64,
    /// The Gekisou skill at its highest level; `None` without one.
    pub skill: Option<SkillLevel>,
    /// The Gekisou skill's mission (0 without one).
    pub mission: i64,
    performer: Performer,
}

/// A snap of the catalog.
#[derive(Clone, Debug)]
pub struct CatalogSnap {
    pub id: i64,
    /// The first Gekisou support skill at the level of the snap's highest rank; `None` without one.
    pub support: Option<SkillLevel>,
    pub mission: i64,
    /// Whether every effect of the support skill at that level is of [`ZERO_SUPPORT_TYPES`].
    pub zero: bool,
}

/// The member cards and snaps of a master, with the Gekisou (support) skills a formation takes.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub members: Vec<CatalogMember>,
    pub snaps: Vec<CatalogSnap>,
}

fn max_level(rows: &[GekisouSkillEffectRow], id: i64) -> Option<i64> {
    if id == 0 {
        return None;
    }
    rows.iter().filter(|r| r.skill_id == id).map(|r| r.level).max()
}

impl Catalog {
    pub fn new(master: &Master) -> Catalog {
        let mut members = Vec::with_capacity(master.member_cards.len());
        for c in &master.member_cards {
            let band_id = master.character(c.character_id).map_or(0, |ch| ch.band_id);
            let skill = max_level(&master.gekisou_skill_effects, c.gekisou_skill_id)
                .map(|level| SkillLevel { id: c.gekisou_skill_id, level });
            let row = master.gekisou_skill(c.gekisou_skill_id);
            let mission = if skill.is_some() { row.map_or(0, |r| r.gekisou_mission_type) } else { 0 };
            let performer = Performer {
                band_id,
                character_id: c.character_id,
                card_type: c.card_type,
                tag_ids: c.best_music_tag_ids.clone(),
                live_skill_categories: master
                    .live_skill(c.live_skill_id)
                    .map(|s| s.skill_categories.clone())
                    .unwrap_or_default(),
                gekisou_skill_categories: row.map(|s| s.skill_categories.clone()).unwrap_or_default(),
                gekisou_mission_type: mission,
                gekisou_skill: skill.map(|s| (s.id, s.level)),
                ..Default::default()
            };
            members.push(CatalogMember { id: c.id, character_id: c.character_id, band_id, skill, mission, performer });
        }
        let mut snaps = Vec::with_capacity(master.support_cards.len());
        for s in &master.support_cards {
            let id = s.gekisou_support_skill_id_01;
            let level = master
                .support_card_ranks
                .iter()
                .filter(|r| r.group == s.rank_group)
                .max_by_key(|r| r.rank)
                .map_or(0, |r| r.gekisou_support_skill_01_level);
            let rows: Vec<&GekisouSkillEffectRow> = master
                .gekisou_support_skill_effects
                .iter()
                .filter(|r| id != 0 && r.skill_id == id && r.level == level)
                .collect();
            let support = (!rows.is_empty()).then_some(SkillLevel { id, level });
            let mission = if support.is_some() {
                master.gekisou_support_skill(id).map_or(0, |r| r.gekisou_mission_type)
            } else {
                0
            };
            let zero = rows.iter().all(|r| ZERO_SUPPORT_TYPES.contains(&r.skill_effect_type));
            snaps.push(CatalogSnap { id: s.id, support, mission, zero });
        }
        Catalog { members, snaps }
    }

    /// Whether some member card has a Gekisou skill.
    pub fn has_skills(&self) -> bool {
        self.members.iter().any(|m| m.skill.is_some())
    }
}

/// Whether a support skill's member target conditions (5000) hold for its member: `None` without such a condition.
fn band_match(master: &Master, support: SkillLevel, p: &Performer) -> Option<bool> {
    let mut found: Option<bool> = None;
    let rows =
        master.gekisou_support_skill_effects.iter().filter(|r| r.skill_id == support.id && r.level == support.level);
    for r in rows {
        for g in [r.skill_trigger_condition_group, r.skill_condition_group] {
            if g == 0 {
                continue;
            }
            for s in master.skill_condition_sets.iter().filter(|s| s.group == g) {
                for &cid in &s.condition_ids {
                    let Some(c) = master.skill_condition(cid) else { continue };
                    if c.condition_type != CONDITION_MEMBER_TARGET {
                        continue;
                    }
                    let hit = c
                        .condition_target_ids
                        .iter()
                        .filter_map(|&t| master.skill_target(t))
                        .any(|t| p.matches_skill_target(t));
                    let ok = hit == c.is_positive;
                    found = Some(found.unwrap_or(false) || ok);
                }
            }
        }
    }
    found
}

/// One performer of a formation.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormationSlot {
    pub member_card_id: i64,
    pub character_id: i64,
    pub band_id: i64,
    pub gekisou_skill: SkillLevel,
    pub snap_id: Option<i64>,
    pub support_skill: Option<SkillLevel>,
    /// Whether the support skill's band condition (5000) holds for the member; `None` without a support skill or
    /// without such a condition.
    pub band_match: Option<bool>,
}

/// A range of a seed with the formation.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GekisouRangeResult {
    pub range_score: i32,
    pub rank_bonus: i32,
    pub range_score_perfect: i32,
    pub max_combo: i32,
    pub just_count: i32,
    pub luck_points: i32,
    pub lot_results: [i32; 4],
}

/// A seed of the best formation, shaped as a no-skill seed ([`SeedStats`]): the whole-live simulation with the
/// formation; only the plain kind has weights.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GekisouSeed {
    pub seed: i32,
    /// The exact score with the formation and no live skill, rank 1 bonuses included.
    pub score: i32,
    /// The same on the Perfect play.
    pub score_perfect: i32,
    pub ranges: Vec<GekisouRangeResult>,
    /// `weights[kind][position]` with the formation, the plain kind only (every other kind `None`).
    pub weights: Vec<Option<Vec<f64>>>,
    /// `rangeWeights[kind][position][range]`, the plain kind only; `None` when the chart's no-skill seeds have none.
    pub range_weights: Option<Vec<Option<Vec<Vec<f64>>>>>,
    /// A random deck of the plain kind's master values with the formation at [`super::CHECK_POWER`].
    pub check: Check,
    /// The check deck at random ranks; `None` without range weights or without ranges.
    pub rank_check: Option<RankCheck>,
}

/// The Gekisou skill statistics of a chart: its best formation for the default scenario and the formation's seeds.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GekisouStats {
    pub formation: Vec<FormationSlot>,
    /// The formation's gain of the default figure on the whole-live simulation: the mean over the seeds of
    /// `score / power + sum_k weights[plain][k]` with the formation minus the same without it.
    pub objective: f64,
    /// The same gain on the Gekisou windows of the screened seeds, as the search saw it.
    pub screened: f64,
    /// Formations the search evaluated.
    pub evaluations: usize,
    pub seeds: Vec<GekisouSeed>,
}

/// The document header of the Gekisou skills (`gekisouSkills`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GekisouHeader {
    pub plain_kind: Option<usize>,
    pub member_level: &'static str,
    pub snap_level: &'static str,
    pub search: &'static str,
    pub screen_seeds: usize,
    /// Seeds of a luck chart's formation (`charts[].gekisou.seeds`); the first ones are the chart's seeds.
    pub seeds: usize,
}

impl GekisouHeader {
    pub fn new(kinds: &[Kind], seeds: usize) -> GekisouHeader {
        GekisouHeader {
            plain_kind: plain_kind(kinds),
            member_level: "highest",
            snap_level: "highestRank",
            search: SEARCH,
            screen_seeds: SCREEN_SEEDS,
            seeds,
        }
    }
}

/// The plain kind: effect type 2000 on the whole deck for 5 s without targets, conditions or limits (the page's
/// `plainKind`).
pub fn plain_kind(kinds: &[Kind]) -> Option<usize> {
    kinds
        .iter()
        .find(|k| {
            k.effect_type == 2000
                && k.skill_target_ids.is_empty()
                && k.skill_condition_group == 0
                && k.skill_release_condition_group == 0
                && k.effect_limit_count == 0
                && k.effect_execute_limit_count == 0
                && k.duration_ms == 5000
        })
        .map(|k| k.id)
}

struct Info<'a>(&'a LiveModel);

impl GekisouComboInfo for Info<'_> {
    fn gekisou_combo(&self, t: i32) -> Option<i32> {
        self.0.gekisou_combo_at(t)
    }
}

/// A candidate performer: a member class (`class` into the classes), a snap class (`None`: no snap) and the band
/// condition's result, with the member cards (catalog indices) that give it.
#[derive(Clone, Debug)]
struct Opt {
    class: usize,
    snap: Option<usize>,
    cards: Vec<usize>,
}

/// A snap class: a support skill and level, the snaps (catalog indices) that have it.
#[derive(Clone, Debug)]
struct SnapClass {
    support: SkillLevel,
    snaps: Vec<usize>,
}

/// A screened seed: the no-formation window, range scores and rank bonuses.
struct Base {
    seed: i32,
    window: Vec<NoteEval>,
    range_scores: Vec<i64>,
    rank_bonuses: Vec<i64>,
}

/// The formation search of a chart. Its public methods are diagnostics ([`super::search_probe`]), not a stable
/// interface.
pub struct Search<'a, 'm> {
    live: &'a Live<'m>,
    catalog: &'a Catalog,
    infos: &'a [RangeInfo],
    frames: Vec<usize>,
    notes: Vec<WindowNote>,
    /// Bit i: the note is in range i's score frames.
    in_range: Vec<u8>,
    /// Per position: the window notes the plain skill covers there.
    cover: Vec<Vec<bool>>,
    snaps: Vec<SnapClass>,
    /// The member classes' Gekisou skills (`None`: the fillers).
    classes: Vec<Option<SkillLevel>>,
    opts: Vec<Opt>,
    base: Vec<Base>,
    cache: HashMap<Vec<usize>, f64>,
    evaluations: usize,
}

impl<'a, 'm> Search<'a, 'm> {
    fn new(
        live: &'a Live<'m>,
        catalog: &'a Catalog,
        infos: &'a [RangeInfo],
        plain: Option<&Kind>,
        seeds: &[SeedStats],
        screen: usize,
    ) -> Result<Search<'a, 'm>, Error> {
        let g = live.gekisou.as_ref().ok_or_else(|| Error::Input("Gekisou skills without Gekisou".into()))?;
        let missions: Vec<i64> = infos.iter().map(|r| r.mission).collect();
        let mut lm = LiveModel::new_gekisou(live.master, &[], live.notes, &[], live.params, &g.setup)?;
        let rf = lm.record_range_frames(&live.play, &g.dt)?;
        let mut frames: Vec<usize> = rf.iter().flat_map(|r| r.start..=r.finish).collect();
        frames.sort_unstable();
        frames.dedup();
        let by_id: HashMap<i32, &LiveNote> = live.notes.iter().map(|n| (n.note_id, n)).collect();
        let note = |id: i32| by_id.get(&id).copied().ok_or_else(|| Error::Input(format!("unknown note {id}")));
        let mut times: Vec<i32> = Vec::new();
        for f in &live.play.frames {
            for j in &f.judged {
                times.push(note(j.note_id)?.time_ms);
            }
        }
        times.sort_unstable();
        let mut notes = Vec::new();
        for &fi in &frames {
            for j in &live.play.frames[fi].judged {
                let n = *note(j.note_id)?;
                let combo = times.partition_point(|&t| t < n.time_ms) as i32;
                notes.push(WindowNote { note: n, combo, score_type: 0 });
            }
        }
        let in_range = notes
            .iter()
            .map(|n| {
                let f = get_frame(n.note.time_ms);
                infos.iter().enumerate().fold(0u8, |m, (i, r)| {
                    if get_frame(r.start_ms) < f && f <= get_frame(r.end_ms) { m | (1 << i) } else { m }
                })
            })
            .collect();

        // candidates: members of the chart's missions by skill, the rest fillers; snaps of its missions by support
        // skill and level
        let playing = |mission: i64| mission != 0 && (mission == 4 || missions.contains(&mission));
        let mut classes: Vec<(Option<SkillLevel>, Vec<usize>)> = vec![(None, Vec::new())];
        for (i, m) in catalog.members.iter().enumerate() {
            if m.skill.is_none() {
                continue;
            }
            let key = m.skill.filter(|_| playing(m.mission));
            match classes.iter_mut().find(|c| c.0 == key) {
                Some(c) => c.1.push(i),
                None => classes.push((key, vec![i])),
            }
        }
        let mut snaps: Vec<SnapClass> = Vec::new();
        for (i, s) in catalog.snaps.iter().enumerate() {
            let Some(support) = s.support else { continue };
            if s.zero || !playing(s.mission) {
                continue;
            }
            match snaps.iter_mut().find(|c| c.support == support) {
                Some(c) => c.snaps.push(i),
                None => snaps.push(SnapClass { support, snaps: vec![i] }),
            }
        }
        let mut opts = Vec::new();
        for (ci, (_, cards)) in classes.iter().enumerate() {
            if !cards.is_empty() {
                opts.push(Opt { class: ci, snap: None, cards: cards.clone() });
            }
        }
        for (si, sc) in snaps.iter().enumerate() {
            for (ci, (_, cards)) in classes.iter().enumerate() {
                let mut groups: Vec<(Option<bool>, Vec<usize>)> = Vec::new();
                for &c in cards {
                    let b = band_match(live.master, sc.support, &catalog.members[c].performer);
                    match groups.iter_mut().find(|g| g.0 == b) {
                        Some(g) => g.1.push(c),
                        None => groups.push((b, vec![c])),
                    }
                }
                for (_, cards) in groups {
                    opts.push(Opt { class: ci, snap: Some(si), cards });
                }
            }
        }
        let mut out = Search {
            live,
            catalog,
            infos,
            frames,
            notes,
            in_range,
            cover: Vec::new(),
            snaps,
            classes: classes.iter().map(|c| c.0).collect(),
            opts,
            base: Vec::new(),
            cache: HashMap::new(),
            evaluations: 0,
        };
        if let Some(kind) = plain {
            out.cover = out.plain_cover(kind)?;
        }
        for s in seeds.iter().take(screen.max(1)) {
            let window = out.window(&[], s.seed)?;
            out.base.push(Base {
                seed: s.seed,
                window,
                range_scores: s.ranges.iter().map(|r| r.range_score as i64).collect(),
                rank_bonuses: s.ranges.iter().map(|r| r.rank_bonus as i64).collect(),
            });
        }
        Ok(out)
    }

    /// Per position, the window notes the plain skill covers there: the notes at or after a factor command it files
    /// and before the command that takes it back.
    fn plain_cover(&self, kind: &Kind) -> Result<Vec<Vec<bool>>, Error> {
        let g = self.live.gekisou.as_ref().expect("Gekisou on");
        let id = KIND_SKILL_BASE - kind.id as i64;
        let mut out = Vec::with_capacity(self.live.positions);
        for k in 0..self.live.positions {
            let deck: Vec<Performer> = (0..self.live.positions)
                .map(|i| Performer { live_skill: (i == k).then_some((id, 1)), ..Default::default() })
                .collect();
            let (notes, events, params) = (self.live.notes, self.live.events, self.live.params);
            let mut lm = LiveModel::new_gekisou(self.live.measure, &deck, notes, events, params, &g.setup)?;
            lm.run_timed(&self.live.play, &g.dt)?;
            let owner = (k as i32) * 100 + OWNER_MEMBER;
            let mut cmds: Vec<FactorCommand> =
                lm.factor_commands().into_iter().filter(|c| c.owner_id == owner && c.note_mill != 0).collect();
            cmds.sort_by_key(|c| c.time_ms);
            let mut spans: Vec<(i32, i32)> = Vec::new();
            let mut open: Option<i32> = None;
            for c in cmds {
                if c.note_mill > 0 {
                    open.get_or_insert(c.time_ms);
                } else if let Some(s) = open.take() {
                    spans.push((s, c.time_ms));
                }
            }
            if let Some(s) = open {
                spans.push((s, i32::MAX));
            }
            out.push(
                self.notes
                    .iter()
                    .map(|n| spans.iter().any(|&(s, e)| s <= n.note.time_ms && n.note.time_ms < e))
                    .collect(),
            );
        }
        Ok(out)
    }

    /// The window notes' scores with these performers on a seed.
    fn window(&self, deck: &[Performer], seed: i32) -> Result<Vec<NoteEval>, Error> {
        let g = self.live.gekisou.as_ref().expect("Gekisou on");
        let mut lm = LiveModel::new_gekisou(self.live.master, deck, self.live.notes, &[], self.live.params, &g.setup)?;
        let judged = lm.run_frames(&self.live.play, seed, &g.dt, &self.frames)?;
        if judged.len() != self.notes.len() {
            return Err(Error::Game("the window run judged other notes".into()));
        }
        let mut notes = self.notes.clone();
        for (n, &(id, j)) in notes.iter_mut().zip(&judged) {
            if n.note.note_id != id {
                return Err(Error::Game("the window run judged the notes in another order".into()));
            }
            n.score_type = convert_score_type(j as i64)?;
        }
        let calc = lm.initial_calculator(self.live.params.total_power);
        let cmds = lm.factor_commands();
        window_note_scores(&calc, lm.score_max_frame(), &notes, &cmds, &Info(&lm))
    }

    /// The performers of a formation state: a representative member card of each candidate with its snap's support
    /// skill, at least five performers.
    fn deck(&self, state: &[usize]) -> Vec<Performer> {
        let mut out: Vec<Performer> = state
            .iter()
            .map(|&o| {
                let opt = &self.opts[o];
                let mut p = self.catalog.members[opt.cards[0]].performer.clone();
                if let Some(s) = opt.snap {
                    let s = self.snaps[s].support;
                    p.gekisou_support_skills = vec![(s.id, s.level)];
                }
                p
            })
            .collect();
        while out.len() < self.live.positions.max(5) {
            out.push(Performer::default());
        }
        out
    }

    /// A member card (catalog index) per performer of a state, five different characters (augmenting paths, cards
    /// in catalog order); `None` when there is none or a snap class is used more often than it has snaps.
    pub(crate) fn assign(&self, state: &[usize]) -> Option<Vec<usize>> {
        let mut used = vec![0usize; self.snaps.len()];
        for &o in state {
            if let Some(s) = self.opts[o].snap {
                used[s] += 1;
                if used[s] > self.snaps[s].snaps.len() {
                    return None;
                }
            }
        }
        fn augment(
            s: &Search<'_, '_>,
            state: &[usize],
            j: usize,
            seen: &mut Vec<i64>,
            owner: &mut HashMap<i64, usize>,
            card: &mut [usize],
        ) -> bool {
            for &c in &s.opts[state[j]].cards {
                let ch = s.catalog.members[c].character_id;
                if seen.contains(&ch) {
                    continue;
                }
                seen.push(ch);
                let free = match owner.get(&ch).copied() {
                    None => true,
                    Some(other) => augment(s, state, other, seen, owner, card),
                };
                if free {
                    owner.insert(ch, j);
                    card[j] = c;
                    return true;
                }
            }
            false
        }
        let mut owner: HashMap<i64, usize> = HashMap::new();
        let mut card = vec![usize::MAX; state.len()];
        for j in 0..state.len() {
            let mut seen = Vec::new();
            if !augment(self, state, j, &mut seen, &mut owner, &mut card) {
                return None;
            }
        }
        Some(card)
    }

    /// The default figure's gain of a state on the screened seeds, from the windows: `(score gain) / power + sum_k
    /// (weight gain of the plain skill at k)`, averaged.
    pub fn value(&mut self, state: &[usize]) -> Result<f64, Error> {
        if let Some(&v) = self.cache.get(state) {
            return Ok(v);
        }
        let deck = self.deck(state);
        let mut total = 0f64;
        for b in &self.base {
            let e = match self.window(&deck, b.seed) {
                Ok(e) => e,
                // a skill the simulation cannot play: never chosen
                Err(Error::Unsupported(_)) => {
                    self.cache.insert(state.to_vec(), f64::NEG_INFINITY);
                    return Ok(f64::NEG_INFINITY);
                }
                Err(e) => return Err(e),
            };
            let ranges = self.infos.len();
            let mut dn = 0i64;
            let mut drs = vec![0i64; ranges];
            let mut dw = vec![0f64; self.cover.len()];
            let mut drw = vec![vec![0f64; ranges]; self.cover.len()];
            for (i, (x, x0)) in e.iter().zip(&b.window).enumerate() {
                let d = x.score as i64 - x0.score as i64;
                let du = x.unit - x0.unit;
                dn += d;
                let m = self.in_range[i];
                for (r, v) in drs.iter_mut().enumerate() {
                    if m & (1 << r) != 0 {
                        *v += d;
                    }
                }
                for (k, cover) in self.cover.iter().enumerate() {
                    if cover[i] {
                        dw[k] += du;
                        for (r, v) in drw[k].iter_mut().enumerate() {
                            if m & (1 << r) != 0 {
                                *v += du;
                            }
                        }
                    }
                }
            }
            let mut ds = dn;
            for (r, info) in self.infos.iter().enumerate() {
                ds += (b.range_scores[r] + drs[r]) * info.rank_bonus_percent / 100 - b.rank_bonuses[r];
            }
            let mut w = 0f64;
            for k in 0..dw.len() {
                w += dw[k];
                for (r, info) in self.infos.iter().enumerate() {
                    w += info.rank_bonus_percent as f64 / 100.0 * drw[k][r];
                }
            }
            total += (ds as f64 + w) / POWER as f64;
        }
        let v = total / self.base.len().max(1) as f64;
        self.evaluations += 1;
        self.cache.insert(state.to_vec(), v);
        Ok(v)
    }

    /// The best feasible state among `candidates` (the first on ties), with its value.
    fn best_of(&mut self, candidates: Vec<Vec<usize>>) -> Result<Option<(Vec<usize>, f64)>, Error> {
        let mut best: Option<(Vec<usize>, f64)> = None;
        for c in candidates {
            if self.assign(&c).is_none() {
                continue;
            }
            let v = self.value(&c)?;
            if best.as_ref().is_none_or(|b| v > b.1 + 1e-12) {
                best = Some((c, v));
            }
        }
        Ok(best)
    }

    /// Greedy fill, then coordinate descent: the chosen state and its screened value.
    pub fn run(&mut self) -> Result<(Vec<usize>, f64), Error> {
        let mut state: Vec<usize> = Vec::new();
        let mut value = 0f64;
        for _ in 0..5 {
            let cands: Vec<Vec<usize>> = (0..self.opts.len())
                .map(|o| {
                    let mut s = state.clone();
                    s.push(o);
                    s
                })
                .collect();
            let (s, v) =
                self.best_of(cands)?.ok_or_else(|| Error::Domain("no five members of different characters".into()))?;
            state = s;
            value = v;
        }
        for _ in 0..MAX_SWEEPS {
            let before = value;
            for j in 0..state.len() {
                let cands: Vec<Vec<usize>> = (0..self.opts.len())
                    .filter(|&o| o != state[j])
                    .map(|o| {
                        let mut s = state.clone();
                        s[j] = o;
                        s
                    })
                    .collect();
                if let Some((s, v)) = self.best_of(cands)?
                    && v > value + 1e-12
                {
                    state = s;
                    value = v;
                }
            }
            // two performers swap snaps, each keeping its member class (any band condition result it gives)
            for a in 0..state.len() {
                for b in a + 1..state.len() {
                    let (oa, ob) = (&self.opts[state[a]], &self.opts[state[b]]);
                    if oa.snap == ob.snap {
                        continue;
                    }
                    let with = |class: usize, snap: Option<usize>| -> Vec<usize> {
                        (0..self.opts.len())
                            .filter(|&o| self.opts[o].class == class && self.opts[o].snap == snap)
                            .collect()
                    };
                    let (na, nb) = (with(oa.class, ob.snap), with(ob.class, oa.snap));
                    let mut cands = Vec::new();
                    for &x in &na {
                        for &y in &nb {
                            let mut s = state.clone();
                            s[a] = x;
                            s[b] = y;
                            cands.push(s);
                        }
                    }
                    if let Some((s, v)) = self.best_of(cands)?
                        && v > value + 1e-12
                    {
                        state = s;
                        value = v;
                    }
                }
            }
            if value <= before + 1e-12 {
                break;
            }
        }
        Ok((state, value))
    }
}

/// A candidate performer of a search, for diagnostics.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchOption {
    /// The member class's Gekisou skill; `None`: fillers (a skill of a mission the chart does not play).
    pub skill: Option<SkillLevel>,
    pub support: Option<SkillLevel>,
    pub band_match: Option<bool>,
    pub member_card_ids: Vec<i64>,
    pub snap_ids: Vec<i64>,
}

impl Search<'_, '_> {
    /// The candidates; a state is a list of candidate indices, one per performer (at most five).
    pub fn options(&self) -> Vec<SearchOption> {
        self.opts
            .iter()
            .map(|o| {
                let first = &self.catalog.members[o.cards[0]];
                let support = o.snap.map(|s| self.snaps[s].support);
                SearchOption {
                    skill: self.classes[o.class],
                    support,
                    band_match: support.and_then(|s| band_match(self.live.master, s, &first.performer)),
                    member_card_ids: o.cards.iter().map(|&c| self.catalog.members[c].id).collect(),
                    snap_ids: o.snap.map_or(Vec::new(), |s| {
                        self.snaps[s].snaps.iter().map(|&i| self.catalog.snaps[i].id).collect()
                    }),
                }
            })
            .collect()
    }

    /// Whether a state has five different characters and every snap at most once.
    pub fn feasible(&self, state: &[usize]) -> bool {
        state.len() <= 5 && state.iter().all(|&o| o < self.opts.len()) && self.assign(state).is_some()
    }

    /// Formations evaluated so far (cached states not counted again).
    pub fn evaluations(&self) -> usize {
        self.evaluations
    }
}

/// A chart's search, for [`super::search_probe`].
pub(super) fn search<'a, 'm>(
    live: &'a Live<'m>,
    kinds: &'a [Kind],
    infos: &'a [RangeInfo],
    catalog: &'a Catalog,
    seeds: &[SeedStats],
) -> Result<Search<'a, 'm>, Error> {
    Search::new(live, catalog, infos, plain_kind(kinds).map(|p| &kinds[p]), seeds, SCREEN_SEEDS)
}

/// The score at these ranks: `score - sum_i rankBonus_i + sum_i trunc(rangeScore_i * percent_i(ranks[i]) / 100)`.
fn score_at_ranks(score: i32, ranges: &[GekisouRangeResult], infos: &[RangeInfo], ranks: &[i32]) -> Result<i32, Error> {
    let mut s = score as i64;
    for ((r, info), &rank) in ranges.iter().zip(infos).zip(ranks) {
        s += r.range_score as i64 * info.percent(rank)? / 100 - r.rank_bonus as i64;
    }
    Ok(s as i32)
}

/// The inputs of a chart's Gekisou skill statistics besides the live: the chart's no-skill seeds, the formation's
/// seeds (the chart's seeds first), whether the ranks follow linearly, the score id and the judged notes.
pub(super) struct GkInputs<'a> {
    pub seeds: &'a [SeedStats],
    pub gk_seeds: &'a [i32],
    pub linear: bool,
    pub score_id: i64,
    pub judged: i32,
}

/// The Gekisou skill statistics of a chart.
pub(super) fn gekisou_stats(
    live: &Live<'_>,
    kinds: &[Kind],
    infos: &[RangeInfo],
    catalog: &Catalog,
    inp: &GkInputs<'_>,
) -> Result<GekisouStats, Error> {
    let plain = plain_kind(kinds);
    let mut search = Search::new(live, catalog, infos, plain.map(|p| &kinds[p]), inp.seeds, SCREEN_SEEDS)?;
    let (state, screened) = search.run()?;
    let cards = search.assign(&state).ok_or_else(|| Error::Game("the chosen formation has no members".into()))?;

    // the formation: its member cards, the snaps of each class in catalog order
    let mut next = vec![0usize; search.snaps.len()];
    let mut formation = Vec::with_capacity(5);
    let mut performers = Vec::with_capacity(5);
    for (&o, &c) in state.iter().zip(&cards) {
        let opt = &search.opts[o];
        let m = &catalog.members[c];
        let skill = m.skill.ok_or_else(|| Error::Game(format!("member card {} has no Gekisou skill", m.id)))?;
        let mut p = m.performer.clone();
        let (snap_id, support) = match opt.snap {
            Some(s) => {
                let sc = &search.snaps[s];
                let snap = &catalog.snaps[sc.snaps[next[s]]];
                next[s] += 1;
                p.gekisou_support_skills = vec![(sc.support.id, sc.support.level)];
                (Some(snap.id), Some(sc.support))
            }
            None => (None, None),
        };
        let band = support.and_then(|s| band_match(live.master, s, &m.performer));
        formation.push(FormationSlot {
            member_card_id: m.id,
            character_id: m.character_id,
            band_id: m.band_id,
            gekisou_skill: skill,
            snap_id,
            support_skill: support,
            band_match: band,
        });
        performers.push(p);
    }
    let evaluations = search.evaluations;
    drop(search);

    let mut rng = Rng(GK_CHECK_SALT ^ inp.score_id as u64);
    let mut rank_rng = Rng(GK_RANK_SALT ^ inp.score_id as u64);
    let mut out = Vec::with_capacity(inp.gk_seeds.len());
    let mut gain = 0f64;
    let figure = |score: i32, w: Option<&Vec<f64>>| score as f64 / POWER as f64 + w.map_or(0.0, |w| w.iter().sum());
    for (i, &seed) in inp.gk_seeds.iter().enumerate() {
        let s = measure_seed(live, kinds, infos, plain, inp, &performers, seed, &mut rng, &mut rank_rng)?;
        let with = figure(s.score, plain.and_then(|p| s.weights[p].as_ref()));
        let without = match inp.seeds.get(i) {
            Some(b) if b.seed == seed => figure(b.score, plain.map(|p| &b.weights[p])),
            _ => {
                let (score, w) = no_skill_figure(live, kinds, plain, seed)?;
                figure(score, w.as_ref())
            }
        };
        gain += with - without;
        out.push(s);
    }
    Ok(GekisouStats {
        formation,
        objective: gain / inp.gk_seeds.len().max(1) as f64,
        screened,
        evaluations,
        seeds: out,
    })
}

/// The no-skill score and plain weights of a seed that is not one of the chart's seeds.
fn no_skill_figure(
    live: &Live<'_>,
    kinds: &[Kind],
    plain: Option<usize>,
    seed: i32,
) -> Result<(i32, Option<Vec<f64>>), Error> {
    let none = vec![None; live.positions];
    let (score, _) = live.run_deck(&live.play, live.master, &[], &none, POWER, seed, None)?;
    let Some(p) = plain else { return Ok((score, None)) };
    let unit = kind_factor(kinds[p].effect_type, UNIT_VALUE);
    let mut w = Vec::with_capacity(live.positions);
    for k in 0..live.positions {
        let mut skills = none.clone();
        skills[k] = Some(KIND_SKILL_BASE - p as i64);
        let (s, _) = live.run_deck(&live.play, live.measure, &[], &skills, POWER, seed, None)?;
        w.push((s as f64 - score as f64) / (POWER as f64 * unit));
    }
    Ok((score, Some(w)))
}

/// The whole-live measurements of the formation on one seed.
#[allow(clippy::too_many_arguments)]
fn measure_seed(
    live: &Live<'_>,
    kinds: &[Kind],
    infos: &[RangeInfo],
    plain: Option<usize>,
    inp: &GkInputs<'_>,
    formation: &[Performer],
    seed: i32,
    rng: &mut Rng,
    rank_rng: &mut Rng,
) -> Result<GekisouSeed, Error> {
    let g = live.gekisou.as_ref().ok_or_else(|| Error::Input("Gekisou skills without Gekisou".into()))?;
    let none = vec![None; live.positions];
    let (score, gk) = live.run_deck(&live.play, live.master, formation, &none, POWER, seed, None)?;
    let (score_perfect, gkp) = live.run_deck(&g.perfect, live.master, formation, &none, POWER, seed, None)?;
    if gkp.len() != gk.len() || gk.len() != infos.len() {
        return Err(Error::Game("the formation's plays have other ranges".into()));
    }
    let rs = |r: &GekisouRange| r.end_score.wrapping_sub(r.start_score);
    let ranges: Vec<GekisouRangeResult> = gk
        .iter()
        .zip(&gkp)
        .map(|(r, p)| GekisouRangeResult {
            range_score: rs(r),
            rank_bonus: r.rank_bonus.unwrap_or(0),
            range_score_perfect: rs(p),
            max_combo: r.max_combo,
            just_count: r.just_count,
            luck_points: r.luck_points,
            lot_results: r.lot_results,
        })
        .collect();
    let mut weights: Vec<Option<Vec<f64>>> = vec![None; kinds.len()];
    let mut range_weights: Vec<Option<Vec<Vec<f64>>>> = vec![None; kinds.len()];
    if let Some(p) = plain {
        let unit = kind_factor(kinds[p].effect_type, UNIT_VALUE);
        let mut w = Vec::with_capacity(live.positions);
        let mut rw = Vec::with_capacity(live.positions);
        for k in 0..live.positions {
            let mut skills = none.clone();
            skills[k] = Some(KIND_SKILL_BASE - p as i64);
            let (s, gks) = live.run_deck(&live.play, live.measure, formation, &skills, POWER, seed, None)?;
            w.push((s as f64 - score as f64) / (POWER as f64 * unit));
            rw.push(
                gks.iter()
                    .zip(&ranges)
                    .map(|(r, r0)| (rs(r) as f64 - r0.range_score as f64) / (POWER as f64 * unit))
                    .collect(),
            );
        }
        weights[p] = Some(w);
        range_weights[p] = Some(rw);
    }
    let range_weights = inp.linear.then_some(range_weights);

    // the check deck: one of the plain kind's master values (or no skill) at each position, at another power
    let usable: Vec<usize> = plain.into_iter().collect();
    let (deck, rows) = check_deck(kinds, &usable, live.positions, rng);
    let master = Live::master_with(live.master, &rows);
    let floors = inp.judged as f64 + MAX_GEKISOU_FEVERS as f64;
    let weight = |ki: usize, k: usize| weights[ki].as_ref().map_or(f64::NAN, |w| w[k]);
    let base = score as f64 / POWER as f64;
    let c = live
        .check_with(kinds, &master, formation, &deck, seed, None, base, weight, floors, 0.0)?
        .within(|| format!("Gekisou skills, seed {seed}"))?;
    let check = Check { deck, exact: c.exact, predicted: c.predicted, bound: c.bound };

    let mut rank_check = None;
    if let Some(rw) = range_weights.as_ref().filter(|_| !ranges.is_empty()) {
        let ranks: Vec<i32> = infos.iter().map(|_| 1 + rank_rng.below(RANKS) as i32).collect();
        let confirmations: Vec<(i32, i64)> =
            ranks.iter().zip(infos).map(|(&r, info)| Ok((r, info.percent(r)?))).collect::<Result<_, Error>>()?;
        let base = score_at_ranks(score, &ranges, infos, &ranks)? as f64 / POWER as f64;
        let mut d = Vec::with_capacity(infos.len());
        for (info, &rank) in infos.iter().zip(&ranks) {
            d.push((info.percent(rank)? - info.percent(1)?) as f64 / 100.0);
        }
        let weight = |ki: usize, k: usize| match (&weights[ki], &rw[ki]) {
            (Some(w), Some(rw)) => w[k] + rw[k].iter().zip(&d).map(|(&r, &d)| r * d).sum::<f64>(),
            _ => f64::NAN,
        };
        // each range moves a weight by the difference of two floored bonuses from its rank 1 value: under 2 points
        let slack = 2.0 * infos.len() as f64;
        let c = live
            .check_with(
                kinds,
                &master,
                formation,
                &check.deck,
                seed,
                Some(&confirmations),
                base,
                weight,
                floors,
                slack,
            )?
            .within(|| format!("Gekisou skills, seed {seed} at ranks {ranks:?}"))?;
        rank_check = Some(RankCheck { ranks, exact: c.exact, predicted: c.predicted, bound: c.bound });
    }
    Ok(GekisouSeed { seed, score, score_perfect, ranges, weights, range_weights, check, rank_check })
}
