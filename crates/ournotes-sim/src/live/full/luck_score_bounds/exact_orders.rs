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
//! rotation no order converts a judgement. The lottery law of an order is recorded once for all orders when that
//! recording shows that it does not depend on the order ([`LuckDpCache::certified_order_free`]), and per order
//! otherwise.
//!
//! Orders the certificate does not cover are evaluated one by one with [`luck_exact_score`].

use super::exact::{
    LuckExactProfile, LuckExactScore, Schedule, Stopwatch, check_probes, exact_model, exact_schedule, has_luck,
    lottery_law, luck_exact_score, record_exact, rush_percent,
};
use super::exact_paths::{MAX_LANES, Orders, first_orders};
use super::*;

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

/// The live every order plays.
struct Live<'a> {
    master: &'a Master,
    skills: &'a LuckSkills,
    notes: &'a [LiveNote],
    events: &'a [(i32, i32)],
    params: LiveParams,
    setup: &'a GekisouSetup,
    play: &'a LivePlay,
    delta_times: &'a [f32],
    ranking: Option<&'a [crate::replay::RankConfirmation]>,
}

impl Live<'_> {
    fn model(&self, deck: &[Performer]) -> Result<LiveModel, Error> {
        exact_model(self.master, deck, self.notes, self.events, self.params, self.setup, self.ranking)
    }

    fn law(
        &self,
        deck: &[Performer],
        curves: &mut LuckDpCache,
    ) -> Result<std::sync::Arc<LuckDpCertifiedResult>, Error> {
        let Live { master, skills, notes, events, params, setup, play, delta_times, ranking } = *self;
        lottery_law(master, skills, notes, events, params, setup, play, delta_times, deck, ranking, curves)
    }

    /// The law of `deck` when it is the law of every order of its performers.
    fn shared_law(
        &self,
        deck: &[Performer],
        curves: &mut LuckDpCache,
    ) -> Result<Option<std::sync::Arc<LuckDpCertifiedResult>>, Error> {
        let Live { master, skills, notes, events, params, setup, play, delta_times, ranking } = *self;
        curves.certified_order_free(master, skills, notes, events, params, setup, play, delta_times, deck, ranking)
    }
}

/// The expected native score of each of `orders` of the five performers of `deck`, as [`luck_exact_score`] gives
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
    let live = Live { master, skills, notes, events, params, setup, play, delta_times, ranking };
    let mut profile = LuckExactOrdersProfile::default();
    let alone = match splice(&live, deck, orders, curves, details, &mut profile) {
        Ok(spliced) => return Ok(spliced),
        Err(error) => error.to_string(),
    };
    let mut watch = Stopwatch::start();
    let scores = orders
        .iter()
        .map(|order| {
            let deck = ordered(deck, order);
            luck_exact_score(
                master,
                skills,
                &deck,
                notes,
                events,
                params,
                setup,
                play,
                delta_times,
                ranking,
                Some(&mut *curves),
                details,
            )
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

/// The static part of the certificate, on a model of the formation before it plays.
fn certify(model: &LiveModel) -> Result<(), Error> {
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
    Ok(())
}

/// The scores of `orders` from the rotations' recordings, or why the certificate does not cover them.
fn splice(
    live: &Live,
    deck: &[Performer],
    orders: &[[usize; MEMBERS]],
    curves: &mut LuckDpCache,
    details: bool,
    profile: &mut LuckExactOrdersProfile,
) -> Result<LuckExactOrders, Error> {
    let mut watch = Stopwatch::start();
    if !has_luck(live.setup) {
        return Err(refuse("the live has no LUCK range"));
    }
    // The probe rows and phase binding of each order.
    let mut shapes = Vec::with_capacity(orders.len());
    for order in orders {
        let mut model = live.model(&ordered(deck, order))?;
        check_recorder(&model, live.skills)?;
        let probes = probes_in_native_order(&model, live.skills)?;
        check_probes(&probes)?;
        model.set_luck_weights(live.skills, Vec::new())?;
        shapes.push((probes, bind_probe_phase(&model, live.skills)));
    }
    profile.setup_ms += watch.lap();
    let mut rotations = Vec::with_capacity(MEMBERS);
    for rotation in 0..MEMBERS {
        let order: [usize; MEMBERS] = std::array::from_fn(|position| (rotation + position) % MEMBERS);
        let mut model = live.model(&ordered(deck, &order))?;
        check_recorder(&model, live.skills)?;
        if rotation == 0 {
            certify(&model)?;
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
    let frame_times: Vec<_> = first.trace.iter().map(|&(time, _)| time).collect();
    profile.record_ms += watch.lap();
    let (laws, law_recordings) = match live.shared_law(deck, curves)? {
        Some(law) => (vec![law; orders.len()], 1),
        None => {
            let laws = orders.iter().map(|order| live.law(&ordered(deck, order), curves));
            (laws.collect::<Result<Vec<_>, _>>()?, orders.len() + 1)
        }
    };
    profile.law_ms += watch.lap();
    // Orders that share one replay: the same probe rows, probe phase binding and possible lottery transitions.
    let mut groups = Vec::<Vec<usize>>::new();
    for (index, (rows, phase)) in shapes.iter().enumerate() {
        let shares = |other: &usize| {
            let ((other_rows, other_phase), law, other_law) = (&shapes[*other], &laws[index], &laws[*other]);
            other_phase == phase
                && other_rows.len() == rows.len()
                && other_rows
                    .iter()
                    .zip(rows)
                    .all(|(a, b)| (a.owner, a.value.to_bits()) == (b.owner, b.value.to_bits()))
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
        let (probes, phase) = &shapes[group[0]];
        let schedule = Schedule {
            events,
            frames: trace.frames,
            queries: trace.queries,
            probes: probes.clone(),
            frame_times: frame_times.clone(),
            music_length_ms: first.music_length_ms,
            lanes: group.len(),
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
