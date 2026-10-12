//! The native score expectations of many skill orders of one formation from the recordings of its five
//! rotations.
//!
//! In rotation `j` the member `(j + q) % 5` performs at position `q`, so every member performs at every position in
//! exactly one rotation. A recording holds two kinds of events: the member factor commands without a lottery
//! factor, owned by the position of their performer, and everything else (notes, combo inputs, Rush commands,
//! lottery and probe filing places, score queries and rank bonuses). When every rotation records the same events of
//! the second kind, the certificate below shows that they are the same in every order, and that a member's commands
//! are a function of the member, its position and those events alone. The schedule of an order is then the common
//! events with, at each position, the member commands of the rotation that places the order's member there; the
//! native calculator applies a frame's commands in owner order, and the commands of two positions have distinct
//! owners, so merging the positions' commands into one filing order changes no order's factor history.
//! [`super::exact_paths::ExactReplay`] follows all orders of such a merged schedule at once.
//!
//! The certificate, checked on the formation's models and rotation recordings:
//! - Gekisou combo and Just state is written only by condition skills whose predicates read no live-skill
//!   activation, and the binary32 sums of its bonuses are exact. Such a writer runs at the same times in every
//!   order, and the controller folds the writes of a frame in its later recount. The bonus changes filed between
//!   two judgements share one time, so the recount takes them together in any filing order; combo protection is
//!   only read by a judgement that breaks the combo, and the play has none when the formation protects combos.
//! - Notes deal no damage, so life stays from 1 (the initial life without skill damage) up to twice the maximum
//!   (the initial life without recovery), and every life predicate gives one answer over that range.
//! - No judgement is converted.
//! - The rotations share their calculators and agree on every event that is not a member command.
//!
//! A member's conversions likewise depend only on the member, its position and the common events, so with none in any
//! rotation no order converts a judgement. The lottery law of an order is composed from the lottery kinds of its
//! performers when the caller holds them ([`LotteryKinds::order_laws`]), and otherwise recorded once for the class of
//! orders that its recording shows to share it ([`LuckDpCache::certified_order_class`]), and per order when a
//! recording shows no class.
//!
//! Orders the certificate does not cover are evaluated one by one with [`luck_exact_score`](super::luck_exact_score).

use super::exact::{
    LuckExactProfile, LuckExactScore, Schedule, Stopwatch, check_probes, exact_model, exact_schedule, exact_score,
    has_luck, lottery_law, record_exact, rush_percent,
};
use super::exact_paths::{MAX_LANES, Orders, first_orders};
use super::*;
use crate::live::full::luck_dp::{LotteryKinds, LotteryLive, OrderClass, PAIR_POSITIONS, PositionPairs};

const MEMBERS: usize = 5;

/// Effect types that write the Gekisou controller's combo and Just state.
const GEKISOU_WRITERS: [i64; 8] = [12000, 12002, 12003, 12004, 13000, 13002, 13003, 13004];

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckExactOrders {
    /// The score of each requested order.
    pub scores: Vec<LuckExactScore>,
    /// Why the orders were evaluated one by one; absent when they came from the rotations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alone: Option<String>,
    /// Distinct lottery laws, the reduced recordings made for them, and joint replays run.
    pub laws: usize,
    pub law_recordings: usize,
    pub replays: usize,
    pub profile: LuckExactOrdersProfile,
}

/// The work of one evaluation. Timings are filled only in diagnostic builds.
#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuckExactOrdersProfile {
    /// Models of the orders and rotations, lottery laws, rotation recordings and joint evaluations.
    pub setup_ms: f64,
    pub law_ms: f64,
    pub record_ms: f64,
    pub evaluate_ms: f64,
}

impl LotteryLive<'_> {
    fn model(&self, deck: &[Performer]) -> Result<LiveModel, Error> {
        exact_model(self.master, deck, self.notes, self.events, self.params, self.setup, self.ranking)
    }

    fn law(
        &self,
        deck: &[Performer],
        curves: &mut LuckDpCache,
    ) -> Result<std::sync::Arc<LuckDpCertifiedResult>, Error> {
        let Self { master, skills, notes, events, params, setup, play, delta_times, ranking } = *self;
        lottery_law(master, skills, notes, events, params, setup, play, delta_times, deck, ranking, curves)
    }

    /// The law of `deck` with the orders of its performers that share it.
    fn order_class(&self, deck: &[Performer], curves: &mut LuckDpCache) -> Result<Option<OrderClass>, Error> {
        let Self { master, skills, notes, events, params, setup, play, delta_times, ranking } = *self;
        curves.certified_order_class(master, skills, notes, events, params, setup, play, delta_times, deck, ranking)
    }
}

/// The expected native score of each of `orders` of the five performers of `deck`, as [`luck_exact_score`](super::luck_exact_score) gives
/// it for the deck in that order; `order[q]` is the index in `deck` of the performer at position `q`.
#[allow(clippy::too_many_arguments)]
pub fn luck_exact_orders(
    master: &Master,
    skills: &LuckSkills,
    deck: &[Performer],
    notes: &[LiveNote],
    events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    ranking: Option<&[crate::replay::RankConfirmation]>,
    orders: &[[usize; MEMBERS]],
    curves: Option<&mut LuckDpCache>,
    details: bool,
) -> Result<LuckExactOrders, Error> {
    let live = LotteryLive { master, skills, notes, events, params, setup, play, delta_times, ranking };
    exact_orders(&live, deck, orders, None, curves, details)
}

/// [`luck_exact_orders`] with the lottery law of each order given: `laws[i]` is the law [`luck_order_laws`] gives
/// `orders[i]` for the same formation and live, so the evaluation records none.
#[allow(clippy::too_many_arguments)]
pub fn luck_exact_orders_with_laws(
    master: &Master,
    skills: &LuckSkills,
    deck: &[Performer],
    notes: &[LiveNote],
    events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    ranking: Option<&[crate::replay::RankConfirmation]>,
    orders: &[[usize; MEMBERS]],
    laws: &[std::sync::Arc<LuckDpCertifiedResult>],
    curves: Option<&mut LuckDpCache>,
    details: bool,
) -> Result<LuckExactOrders, Error> {
    if laws.len() != orders.len() {
        return Err(Error::Input("one lottery law per order".into()));
    }
    let live = LotteryLive { master, skills, notes, events, params, setup, play, delta_times, ranking };
    exact_orders(&live, deck, orders, Some(laws), curves, details)
}

fn exact_orders(
    live: &LotteryLive,
    deck: &[Performer],
    orders: &[[usize; MEMBERS]],
    laws: Option<&[std::sync::Arc<LuckDpCertifiedResult>]>,
    curves: Option<&mut LuckDpCache>,
    details: bool,
) -> Result<LuckExactOrders, Error> {
    if deck.len() != MEMBERS {
        return Err(Error::Input("an order evaluation takes five performers".into()));
    }
    let permutation = |order: &[usize; MEMBERS]| {
        let mut seen = [false; MEMBERS];
        order.iter().all(|&member| member < MEMBERS && !std::mem::replace(&mut seen[member], true))
    };
    if !orders.iter().all(permutation) {
        return Err(Error::Input("an order is a permutation of the five performers".into()));
    }
    let mut local = LuckDpCache::new(usize::MAX);
    let curves = curves.unwrap_or(&mut local);
    let mut profile = LuckExactOrdersProfile::default();
    let alone = match splice(live, deck, orders, laws, curves, details, &mut profile) {
        Ok(spliced) => return Ok(spliced),
        Err(error) => error.to_string(),
    };
    let mut watch = Stopwatch::start();
    let scores = orders
        .iter()
        .enumerate()
        .map(|(index, order)| {
            let law = laws.map(|laws| laws[index].clone());
            exact_score(live, &ordered(deck, order), law, Some(&mut *curves), details)
        })
        .collect::<Result<Vec<_>, _>>()?;
    profile.evaluate_ms += watch.lap();
    Ok(LuckExactOrders { scores, alone: Some(alone), laws: 0, law_recordings: 0, replays: 0, profile })
}

fn ordered(deck: &[Performer], order: &[usize; MEMBERS]) -> Vec<Performer> {
    order.iter().map(|&member| deck[member].clone()).collect()
}

/// A rotation's recording split at its common events: the common events, and the member commands with their
/// positions before each of them and after the last.
struct Split<'a> {
    common: Vec<&'a BoundsEvent>,
    members: Vec<Vec<(usize, &'a BoundsEvent)>>,
}

fn split(trace: &BoundsTrace) -> Result<Split<'_>, Error> {
    let mut split = Split { common: Vec::new(), members: vec![Vec::new()] };
    for event in &trace.events {
        match member_position(event)? {
            Some(position) => split.members.last_mut().expect("a gap").push((position, event)),
            None => {
                split.common.push(event);
                split.members.push(Vec::new());
            }
        }
    }
    Ok(split)
}

/// The performer position of a member factor command; None for any other event.
fn member_position(event: &BoundsEvent) -> Result<Option<usize>, Error> {
    let BoundsEvent::Factor { command, .. } = event else {
        return Ok(None);
    };
    if command.luck != 0 {
        if command.owner_id != -1 {
            return Err(refuse("a lottery factor command has a performer owner"));
        }
        return Ok(None);
    }
    let position = usize::try_from(command.owner_id / 100).ok().filter(|&position| position < MEMBERS);
    match position {
        Some(position) if matches!(command.owner_id % 100, 1 | 2) => Ok(Some(position)),
        _ => Err(refuse("a member factor command has no performer position")),
    }
}

/// Whether two rotations recorded the same common event. A note's score reads only whether its life is positive.
fn same_common(a: &BoundsEvent, b: &BoundsEvent) -> bool {
    use BoundsEvent as E;
    match (a, b) {
        (E::Note { frame, index, note }, E::Note { frame: f, index: i, note: n }) => {
            (frame, index, note.time_ms, note.note_id, note.note_type, note.score_type)
                == (f, i, n.time_ms, n.note_id, n.note_type, n.score_type)
                && note.life > 0
                && n.life > 0
        }
        (E::Factor { frame, command }, E::Factor { frame: f, command: c }) => frame == f && command == c,
        (E::Potential { frame, start }, E::Potential { frame: f, start: s }) => (frame, start) == (f, s),
        (E::Probe { frame, time_ms }, E::Probe { frame: f, time_ms: t }) => (frame, time_ms) == (f, t),
        (E::Query { time_ms, to }, E::Query { time_ms: t, to: o }) => (time_ms, to) == (t, o),
        (E::Combo { frame, index, ordinary, gekisou }, E::Combo { frame: f, index: i, ordinary: o, gekisou: g }) => {
            (frame, index, ordinary.to_bits(), gekisou.to_bits()) == (f, i, o.to_bits(), g.to_bits())
        }
        (E::ProbabilityReady(time), E::ProbabilityReady(t)) => time == t,
        (
            E::Rank { range, time_ms, percent, start, end },
            E::Rank { range: r, time_ms: t, percent: p, start: s, end: e },
        ) => (range, time_ms, percent, start, end) == (r, t, p, s, e),
        _ => false,
    }
}

fn same_calc(a: &LiveScoreCalculator, b: &LiveScoreCalculator) -> bool {
    let factors = |c: &LiveScoreCalculator| {
        [
            c.score_adjustment_factor,
            c.music_difficulty_factor,
            c.life_onus_factor,
            c.event_bonus_factor,
            c.assist_factor,
        ]
        .map(f32::to_bits)
    };
    factors(a) == factors(b)
        && a.converted_note_count == b.converted_note_count
        && a.note_factor_percent == b.note_factor_percent
        && a.judgement_score_factor_percent == b.judgement_score_factor_percent
        && a.state == b.state
        && a.combo_table == b.combo_table
        && a.luck_weight.is_none()
        && b.luck_weight.is_none()
}

/// Every checker of a predicate tree satisfies `test`.
fn all_checkers(checker: &Checker, test: &impl Fn(&Checker) -> bool) -> bool {
    test(checker)
        && match checker {
            Checker::And { items, .. } | Checker::Or(items) => items.iter().all(|item| all_checkers(item, test)),
            Checker::Not(inner) => all_checkers(inner, test),
            _ => true,
        }
}

/// Whether a life checker gives one answer for every life in `low..=high`.
fn life_constant(checker: &Checker, low: i64, high: i64) -> bool {
    let same = |value: &Option<i64>, test: fn(i64, i64) -> bool| {
        value.map_or(low == high, |value| test(low, value) == test(high, value))
    };
    match checker {
        Checker::LifeAtLeast(value) => same(value, |life, value| life >= value),
        Checker::LifeAtMost(value) => same(value, |life, value| life <= value),
        Checker::LifeGreater(value) => same(value, |life, value| life > value),
        Checker::LifeLess(value) => same(value, |life, value| life < value),
        Checker::LifeChanged { .. } | Checker::LifePercent { .. } | Checker::LifeDelta { .. } => low == high,
        Checker::Scripted { .. } => false,
        _ => true,
    }
}

/// The static part of the certificate, on a model of the formation before it plays: the bounds of the life of
/// every order while notes deal no damage.
fn certify(model: &LiveModel) -> Result<IntegerBounds, Error> {
    for skill in &model.live {
        if skill.effects.iter().any(|effect| GEKISOU_WRITERS.contains(&model.rows[effect.row].effect_type)) {
            return Err(refuse("a live skill writes Gekisou state"));
        }
    }
    let live_activation = |checker: &Checker| !matches!(checker, Checker::SameMemberLiveSkill(_));
    let mut bonuses = 1i64;
    for skill in &model.cond {
        for (index, effect) in skill.updater.effects().iter().enumerate() {
            let row = &model.rows[effect.row];
            if !GEKISOU_WRITERS.contains(&row.effect_type) {
                continue;
            }
            let releases = skill.updater.updaters.iter().filter(|updater| updater.effect == index);
            let mut checkers = [&effect.trigger, &effect.condition, &effect.reset]
                .into_iter()
                .flatten()
                .chain(releases.filter_map(|updater| updater.release.as_ref()));
            if !checkers.all(|checker| all_checkers(checker, &live_activation)) {
                return Err(refuse("a Gekisou state writer reads a live-skill activation"));
            }
            if matches!(row.effect_type, 12000 | 13000) {
                bonuses = bonuses.saturating_add(row.effect_value.saturating_abs());
            }
        }
    }
    if bonuses >= 1 << 24 {
        return Err(refuse("Gekisou bonus sums may round"));
    }
    let initial = i64::from(model.life.initial_life());
    let has = |effect_type: i64| model.rows.iter().any(|row| row.effect_type == effect_type);
    let low = if has(3002) { 1 } else { initial };
    let high = if has(3001) { initial.max(2 * i64::from(model.life.max_life())) } else { initial };
    let life = |checker: &Option<Checker>| {
        checker.as_ref().is_none_or(|checker| all_checkers(checker, &|checker| life_constant(checker, low, high)))
    };
    let cumulative = |cumulative: &Option<conditions::Cumulative>| {
        low == high || !matches!(cumulative, Some(conditions::Cumulative::LifePerN { .. }))
    };
    let live = model
        .live
        .iter()
        .flat_map(|skill| &skill.effects)
        .all(|effect| life(&effect.condition) && life(&effect.release) && cumulative(&effect.cumulative));
    let cond = model.cond.iter().all(|skill| {
        skill.updater.effects().iter().all(|effect| {
            life(&effect.trigger) && life(&effect.condition) && life(&effect.reset) && cumulative(&effect.cumulative)
        }) && skill.updater.updaters.iter().all(|updater| life(&updater.release))
    });
    if !live || !cond {
        return Err(refuse("a life predicate may answer differently in another order"));
    }
    let bound = |life: i64| i32::try_from(life).map_err(|_| refuse("the life range exceeds a native integer"));
    Ok(IntegerBounds { lower: bound(low)?, upper: bound(high)? })
}

/// The lottery law of each order, and how many reduced recordings it took. The laws are composed from the lottery
/// kinds of the performers when `kinds` holds them all. Otherwise each order takes the law of the first class whose
/// pairs of performers it orders as the class's order does, and an order that no class covers records its own class.
/// Once a recording shows no class, every order not yet covered records its law alone.
fn order_laws(
    live: &LotteryLive,
    deck: &[Performer],
    orders: &[[usize; MEMBERS]],
    curves: &mut LuckDpCache,
    kinds: Option<&mut LotteryKinds<'_>>,
) -> Result<(Vec<std::sync::Arc<LuckDpCertifiedResult>>, usize), Error> {
    if let Some(kinds) = kinds
        && let Some(laws) = kinds.order_laws(live, deck, orders, curves)?
    {
        return Ok((laws, 0));
    }
    let pair = |a: usize, b: usize| -> PositionPairs { 1 << (PAIR_POSITIONS * a.min(b) + a.max(b)) };
    type Class = ([usize; MEMBERS], PositionPairs, std::sync::Arc<LuckDpCertifiedResult>);
    let mut classes = Vec::<Class>::new();
    let (mut laws, mut law_recordings, mut classless) = (Vec::with_capacity(orders.len()), 0, false);
    for order in orders {
        let mut at = [0; MEMBERS];
        for (position, &member) in order.iter().enumerate() {
            at[member] = position;
        }
        let keeps = |(class_at, pairs, _): &&Class| {
            (0..MEMBERS).all(|a| {
                (a + 1..MEMBERS).all(|b| pairs & pair(a, b) == 0 || (at[a] < at[b]) == (class_at[a] < class_at[b]))
            })
        };
        if let Some((_, _, law)) = classes.iter().find(keeps) {
            laws.push(law.clone());
            continue;
        }
        let deck = ordered(deck, order);
        law_recordings += 1;
        if !classless {
            match live.order_class(&deck, curves)? {
                Some(OrderClass { law, pairs: positions }) => {
                    let mut pairs = 0;
                    for a in 0..MEMBERS {
                        for b in a + 1..MEMBERS {
                            if positions & pair(a, b) != 0 {
                                pairs |= pair(order[a], order[b]);
                            }
                        }
                    }
                    classes.push((at, pairs, law.clone()));
                    laws.push(law);
                    continue;
                }
                None => (classless, law_recordings) = (true, law_recordings + 1),
            }
        }
        laws.push(live.law(&deck, curves)?);
    }
    Ok((laws, law_recordings))
}

/// The certified lottery law of each of `orders` of a five-performer formation (`deck` in slot order, each order the
/// slots in performance order) in `live`, as [`luck_exact_orders`] takes them. `kinds` composes them from the lottery
/// kinds of the performers when it can, and keeps the kinds and laws read here.
pub fn luck_order_laws(
    live: &LotteryLive<'_>,
    deck: &[Performer],
    orders: &[[usize; MEMBERS]],
    curves: &mut LuckDpCache,
    kinds: Option<&mut LotteryKinds<'_>>,
) -> Result<Vec<std::sync::Arc<LuckDpCertifiedResult>>, Error> {
    if deck.len() != MEMBERS {
        return Err(Error::Input("an order evaluation takes five performers".into()));
    }
    let permutation = |order: &[usize; MEMBERS]| {
        let mut seen = [false; MEMBERS];
        order.iter().all(|&member| member < MEMBERS && !std::mem::replace(&mut seen[member], true))
    };
    if !orders.iter().all(permutation) {
        return Err(Error::Input("an order is a permutation of the five performers".into()));
    }
    Ok(order_laws(live, deck, orders, curves, kinds)?.0)
}

/// The probe rows in native filing order and the probe phase binding of a formation's model before it plays.
pub(super) fn order_shape(model: &mut LiveModel, skills: &LuckSkills) -> Result<(Vec<ProbeRow>, bool), Error> {
    check_recorder(model, skills)?;
    let probes = probes_in_native_order(model, skills)?;
    check_probes(&probes)?;
    model.set_luck_weights(skills, Vec::new())?;
    Ok((probes, bind_probe_phase(model, skills)))
}

/// The scores of `orders` from the rotations' recordings, or why the certificate does not cover them. `given` holds
/// the law of each order when the caller has them.
fn splice(
    live: &LotteryLive,
    deck: &[Performer],
    orders: &[[usize; MEMBERS]],
    given: Option<&[std::sync::Arc<LuckDpCertifiedResult>]>,
    curves: &mut LuckDpCache,
    details: bool,
    profile: &mut LuckExactOrdersProfile,
) -> Result<LuckExactOrders, Error> {
    let mut watch = Stopwatch::start();
    if !has_luck(live.setup) {
        return Err(refuse("the live has no LUCK range"));
    }
    // The shape of each order, on one model of the formation whose members move to each order in turn: members that
    // have not acted move to other positions exactly as a model built for the new order (see `OrderedLive`).
    let mut model = live.model(deck)?;
    let mut assign: Vec<usize> = (0..MEMBERS).collect();
    let mut shapes = Vec::with_capacity(orders.len());
    for order in orders {
        model.move_to(&mut assign, order)?;
        shapes.push(order_shape(&mut model, live.skills)?);
    }
    profile.setup_ms += watch.lap();
    let mut rotations = Vec::with_capacity(MEMBERS);
    let mut life = None;
    for rotation in 0..MEMBERS {
        let order: [usize; MEMBERS] = std::array::from_fn(|position| (rotation + position) % MEMBERS);
        let mut model = live.model(&ordered(deck, &order))?;
        check_recorder(&model, live.skills)?;
        if rotation == 0 {
            life = Some(certify(&model)?);
            // Good to Just keep the combo; waiting and passed notes leave it.
            let breaks = live
                .play
                .frames
                .iter()
                .flat_map(|frame| &frame.judged)
                .any(|note| !matches!(note.judgement, 0 | 3..=7));
            if breaks && model.rows.iter().any(|row| row.effect_type == 12004) {
                return Err(refuse("a judgement breaks a protected combo"));
            }
        }
        let probes = probes_in_native_order(&model, live.skills)?;
        let calc = model.score.calc.clone();
        model.set_luck_weights(live.skills, Vec::new())?;
        let trace = record_exact(&mut model, probes, true, live.play, live.delta_times)?;
        if model.converted_judgements() != 0 {
            return Err(refuse("a judgement is converted"));
        }
        if model.life.has_note_damage() {
            return Err(refuse("a note deals damage"));
        }
        if model.gk.as_ref().is_some_and(|gk| !gk.ctrl.bonus_changes_share_times()) {
            return Err(refuse("Gekisou bonus changes between two judgements have different times"));
        }
        rotations.push((model, calc, trace));
    }
    let splits = rotations.iter().map(|(_, _, trace)| split(trace)).collect::<Result<Vec<_>, _>>()?;
    let (first, calc, trace) = &rotations[0];
    for ((model, other_calc, other), split) in rotations.iter().zip(&splits).skip(1) {
        if !same_calc(calc, other_calc)
            || (other.frames, other.queries, model.music_length_ms)
                != (trace.frames, trace.queries, first.music_length_ms)
            || split.common.len() != splits[0].common.len()
            || !split.common.iter().zip(&splits[0].common).all(|(a, b)| same_common(a, b))
        {
            return Err(refuse("the rotations differ in an event that is not a member command"));
        }
    }
    let rush_percent = rush_percent(live.master, calc)?;
    let life = life.expect("the first rotation's certificate");
    let frame_times: Vec<_> = first.trace.iter().map(|&(time, _)| time).collect();
    profile.record_ms += watch.lap();
    let (laws, law_recordings) = match given {
        Some(laws) => (laws.to_vec(), 0),
        None => order_laws(live, deck, orders, curves, None)?,
    };
    profile.law_ms += watch.lap();
    // Orders that share one replay: the same probe phase binding, probe rows in all or none, and the same possible
    // lottery transitions.
    let mut groups = Vec::<Vec<usize>>::new();
    for (index, (rows, phase)) in shapes.iter().enumerate() {
        let shares = |other: &usize| {
            let ((other_rows, other_phase), law, other_law) = (&shapes[*other], &laws[index], &laws[*other]);
            other_phase == phase
                && other_rows.is_empty() == rows.is_empty()
                && (std::sync::Arc::ptr_eq(law, other_law)
                    || (law.probe_transitions == other_law.probe_transitions
                        && law.rush_transitions == other_law.rush_transitions))
        };
        match groups.iter_mut().find(|group| group.len() < MAX_LANES && shares(&group[0])) {
            Some(group) => group.push(index),
            None => groups.push(vec![index]),
        }
    }
    let mut scores = vec![None; orders.len()];
    for group in &groups {
        // The lanes whose order places the member of rotation `j` at position `q`, by `[j][q]`.
        let mut lanes = [[0 as Orders; MEMBERS]; MEMBERS];
        for (lane, &index) in group.iter().enumerate() {
            for (position, &member) in orders[index].iter().enumerate() {
                lanes[(member + MEMBERS - position) % MEMBERS][position] |= 1 << lane;
            }
        }
        let all = first_orders(group.len());
        let common = &splits[0].common;
        let mut events = Vec::new();
        for gap in 0..=common.len() {
            for (rotation, split) in splits.iter().enumerate() {
                for &(position, event) in &split.members[gap] {
                    let orders = lanes[rotation][position];
                    if orders != 0 {
                        events.push((event.clone(), orders));
                    }
                }
            }
            if let Some(&event) = common.get(gap) {
                events.push((event.clone(), all));
            }
        }
        // The orders of each distinct set of probe rows.
        let mut configs = Vec::<(Orders, Vec<ProbeRow>)>::new();
        for (lane, &index) in group.iter().enumerate() {
            let rows = &shapes[index].0;
            let same = |other: &Vec<ProbeRow>| {
                other.len() == rows.len()
                    && other.iter().zip(rows).all(|(a, b)| (a.owner, a.value.to_bits()) == (b.owner, b.value.to_bits()))
            };
            match configs.iter_mut().find(|(_, other)| same(other)) {
                Some((orders, _)) => *orders |= 1 << lane,
                None => configs.push((1 << lane, rows.clone())),
            }
        }
        let phase = &shapes[group[0]].1;
        let schedule = Schedule {
            events,
            frames: trace.frames,
            queries: trace.queries,
            configs,
            frame_times: frame_times.clone(),
            music_length_ms: first.music_length_ms,
            lanes: group.len(),
            final_lives: vec![life; group.len()],
        };
        let group_laws: Vec<_> = group.iter().map(|&index| laws[index].clone()).collect();
        let results = exact_schedule(
            &schedule,
            calc,
            rush_percent,
            &group_laws,
            live.play,
            *phase,
            live.setup.fevers.len(),
            live.ranking.is_none(),
            None,
            details,
            LuckExactProfile::default(),
            Stopwatch::start(),
        )?;
        for (&index, score) in group.iter().zip(results) {
            scores[index] = Some(score);
        }
    }
    profile.evaluate_ms += watch.lap();
    let mut distinct = Vec::<&std::sync::Arc<LuckDpCertifiedResult>>::new();
    for law in &laws {
        if !distinct.iter().any(|other| std::sync::Arc::ptr_eq(other, law)) {
            distinct.push(law);
        }
    }
    Ok(LuckExactOrders {
        scores: scores.into_iter().map(|score| score.expect("every order has a group")).collect(),
        alone: None,
        laws: distinct.len(),
        law_recordings,
        replays: groups.len(),
        profile: *profile,
    })
}
