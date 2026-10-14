//! Judgement conversions of a play and the deterministic life recorder of a formation.

use super::*;

/// These appliers are the dependency closure of life under a fixed judgement stream: life commands,
/// judgement conversion (which changes note damage), and extensions of live effects. Score/rank/lottery
/// appliers do not mutate any of them. All retained checkers must be independent of lottery outcomes.
fn life_dependency(effect: i64) -> bool {
    // Combo/Just writers also belong to the closure: deterministic life/conversion predicates and cumulative
    // counters can read them. Dropping them would change the life recorder's native trace.
    matches!(effect, 3000..=3004 | 4004 | 12000 | 12002..=12004 | 12006 | 13000 | 13002..=13005 | 15000)
}

fn deterministic_life_checker(checker: &Checker) -> bool {
    match checker {
        Checker::Fixed(_)
        | Checker::LifeAtLeast(_)
        | Checker::LifeAtMost(_)
        | Checker::LifeGreater(_)
        | Checker::LifeLess(_)
        | Checker::LifeChanged { .. }
        | Checker::LifePercent { .. }
        | Checker::LifeDelta { .. }
        | Checker::LiveComboMultiple { .. }
        | Checker::LiveComboAtLeast(_)
        | Checker::ElapsedTime { .. }
        | Checker::SameMemberLiveSkill(_)
        | Checker::NoteJudgementMatch { .. }
        | Checker::NoteJudgementCount { .. }
        | Checker::ComboAtLeast { .. }
        | Checker::GkInterval { .. }
        | Checker::JustAtLeast { .. }
        | Checker::GkJustEdge { .. }
        | Checker::GkReachRank { .. }
        | Checker::RangeStart { .. }
        | Checker::RangePlaying { .. }
        | Checker::RangeComplete
        | Checker::SnapGekisouStart { .. }
        | Checker::GkLiveStart(_) => true,
        Checker::And { items, .. } | Checker::Or(items) => items.iter().all(deterministic_life_checker),
        Checker::Not(inner) => deterministic_life_checker(inner),
        _ => false,
    }
}

/// Whether a held effect can convert the declared judgements before the LUCK controller consumes them.
/// Even ordinary live/Snap conversions make a Rush curve dependent on the deck and skill-event timeline.
#[doc(hidden)]
pub fn luck_has_judgement_conversion(master: &Master, deck: &[Performer]) -> bool {
    !held_conversions(master, deck).is_empty()
}

/// Calls `each` with the effect type, the effect value and the target ids of every effect row of the skills
/// `performer` holds in a live with Gekisou: its live, support and Gekisou skills, and its Gekisou support skills beside
/// a Gekisou skill.
fn held_effects<'m>(master: &'m Master, performer: &Performer, mut each: impl FnMut(i64, i64, &'m [i64])) {
    if let Some((id, lv)) = performer.live_skill {
        for r in master.live_skill_effects.iter().filter(|r| r.live_skill_id == id && r.level == lv) {
            each(r.skill_effect_type, r.effect_value, &r.skill_target_ids);
        }
    }
    for &(id, lv) in &performer.support_skills {
        for r in master.support_skill_effects.iter().filter(|r| r.support_skill_id == id && r.level == lv) {
            each(r.skill_effect_type, r.effect_value, &r.skill_target_ids);
        }
    }
    if let Some((id, lv)) = performer.gekisou_skill {
        for r in master.gekisou_skill_effects.iter().filter(|r| r.skill_id == id && r.level == lv) {
            each(r.skill_effect_type, r.effect_value, &r.skill_target_ids);
        }
        for &(id, lv) in &performer.gekisou_support_skills {
            for r in master.gekisou_support_skill_effects.iter().filter(|r| r.skill_id == id && r.level == lv) {
                each(r.skill_effect_type, r.effect_value, &r.skill_target_ids);
            }
        }
    }
}

/// The effect type, the effect value and the targets of each judgement conversion effect (12006, 13005) the performers
/// hold.
fn held_conversions<'m>(master: &'m Master, deck: &[Performer]) -> Vec<(i64, i64, &'m [i64])> {
    let mut held = Vec::new();
    for performer in deck {
        held_effects(master, performer, |effect, value, targets| {
            if matches!(effect, 12006 | 13005) {
                held.push((effect, value, targets));
            }
        });
    }
    held
}

/// The life range `(initial, 2 × initial)` of `play` when none of its judgements deals note damage: the life stays in
/// it at every time, for a formation that changes no judgement of the play but Perfect and Just into one another
/// ([`play_conversions`]) and holds no performer that [`leaves_life_range`].
///
/// The life starts at the initial life, which is also its maximum, and only the commands of the life controller change
/// it. A note files damage only for a judgement whose damage is positive, and every judgement reaches the controller as
/// the play declares it or as a Perfect or Just that deals no note damage. A skill files damage only by the effect 3002,
/// and only the life limit 3000 changes the maximum. That leaves the recovery 3001: it sets a positive life to its sum
/// with a positive amount, or to twice the maximum when the sum is not below it, which is a life of the range when the
/// sum does not wrap. Guards and damage reductions change no life.
pub(crate) fn play_life_range(master: &Master, play: &LivePlay) -> Option<(i32, i32)> {
    let (initial, damage) = life::life_settings(master).ok()?;
    let range = (initial, initial.checked_mul(2)?);
    let harmless = |note: &JudgedNote| life::harmless_judgement(&damage, note.judgement);
    (initial > 0 && play.frames.iter().flat_map(|frame| &frame.judged).all(harmless)).then_some(range)
}

/// Whether a skill of `performer` can take the life out of `range` ([`play_life_range`]): a life limit (3000), a skill
/// damage (3002), or a recovery (3001) whose sum with a life of the range wraps.
pub(crate) fn leaves_life_range(master: &Master, performer: &Performer, range: (i32, i32)) -> bool {
    let mut leaves = false;
    held_effects(master, performer, |effect, value, _| {
        leaves |= match effect {
            3000 | 3002 => true,
            3001 => (value as i32).checked_add(range.1).is_none(),
            _ => false,
        };
    });
    leaves
}

/// Whether `checker` reads only what is the same in every formation of a live until a judgement of its play is
/// converted: the declared judgements and the live combo they make, the frame times and the chart's skill events of its
/// holder's position, the range states, which follow the fevers and the frame times, and its holder, which decides a
/// fixed answer.
fn formation_blind(checker: &Checker) -> bool {
    match checker {
        Checker::Fixed(_)
        | Checker::ElapsedTime { .. }
        | Checker::LiveComboMultiple { .. }
        | Checker::LiveComboAtLeast(_)
        | Checker::SameMemberLiveSkill(_)
        | Checker::NoteJudgementMatch { .. }
        | Checker::NoteJudgementCount { .. }
        | Checker::RangeStart { .. }
        | Checker::RangePlaying { .. }
        | Checker::RangeComplete
        | Checker::SnapGekisouStart { .. }
        | Checker::GkLiveStart(_) => true,
        Checker::And { items, .. } | Checker::Or(items) => items.iter().all(formation_blind),
        Checker::Not(inner) => formation_blind(inner),
        _ => false,
    }
}

/// Whether the [`formation_blind`] checker `checker` answers alike for judgements that differ only by Perfects and
/// Justs turned into one another: it reads no judgement, or its judgement match takes Perfect and Just alike. The live
/// combo goes on through both.
///
/// A judgement match answers for each judged note apart, a hit or none or a comparison error, from the note's judgement
/// alone. A judgement count adds one for each of its targets that a note's judgement is, and a consecutive count also
/// starts over for each target it is not.
fn relabel_blind(checker: &Checker) -> bool {
    match checker {
        Checker::NoteJudgementMatch { kind, targets, .. } => {
            let answer = |judgement: i64| -> Result<bool, ()> {
                for &target in targets {
                    let hit = if *kind == 1000 {
                        target == judgement
                    } else if !(1..=6).contains(&target) {
                        return Err(());
                    } else if *kind == 1010 {
                        judgement >= target
                    } else {
                        judgement <= target
                    };
                    if hit {
                        return Ok(true);
                    }
                }
                Ok(false)
            };
            answer(PERFECT) == answer(JUST)
        }
        Checker::NoteJudgementCount { consecutive, targets, .. } => {
            let count = |judgement: i64| targets.iter().filter(|&&target| target == judgement).count();
            count(PERFECT) == count(JUST) && !(*consecutive && count(PERFECT) != 0)
        }
        Checker::And { items, .. } | Checker::Or(items) => items.iter().all(relabel_blind),
        Checker::Not(inner) => relabel_blind(inner),
        checker => formation_blind(checker),
    }
}

/// Whether the judgement conversions of `performer` change no judgement of `play` in a formation whose other
/// performers' conversions change none: None when a conversion effect reads something other than what
/// [`formation_blind`] lets it read or has a cumulative condition, the performer's life recorder ([`life_recorder`])
/// cannot be built or run, or it changes a judgement when the performer plays alone at some position, after blank
/// performers. Otherwise whether every conversion effect reads only what [`relabel_blind`] lets it read: such
/// conversions change none either in a formation whose other performers' conversions change none or only turn Perfects
/// and Justs into one another ([`PlayConversions::Relabel`]).
///
/// No formation whose performers all pass this check changes a judgement of the play. Until the first judgement such
/// a formation changes, its life recorder plays the declared judgements, so each conversion effect of a performer
/// reads what it reads in the performer's run alone at the same position, and its converter is registered, counts and
/// is released as it is there. The converter list holds the converters of that performer in the same order, so the
/// first judgement the formation changes is one the performer changes alone.
///
/// Beside performers that only turn Perfects and Justs into one another, the same holds up to such changes for
/// performers whose conversion effects read only what [`relabel_blind`] lets them read. Until the first other change,
/// the recorder plays the declared judgements up to such changes, which each of their conversion effects answers as in
/// the performer's run alone. Each judgement reaches the converters as the play declares it, and none of their
/// converters, registered as in that run, changes it there; so none converts, counts or is released for its count, and
/// no other change ever comes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn quiet_conversions(
    master: &Master,
    performer: &Performer,
    notes: &[LiveNote],
    events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    play: &LivePlay,
    delta_times: &[f32],
    ranking: Option<&[crate::replay::RankConfirmation]>,
) -> Option<bool> {
    let converts = |row: &EffectRow| matches!(row.effect_type, 12006 | 13005);
    let mut relabels = true;
    let mut blind = |cumulative: Option<&conditions::Cumulative>, checkers: &[&Option<Checker>]| {
        let checkers = checkers.iter().filter_map(|checker| checker.as_ref());
        relabels &= checkers.clone().all(relabel_blind);
        cumulative.is_none() && checkers.clone().all(formation_blind)
    };
    for position in 0..5 {
        let mut deck = vec![Performer::default(); position];
        deck.push(performer.clone());
        let mut model = life_recorder(master, &deck, notes, events, params, setup, ranking).ok()?;
        for effect in model.live.iter().flat_map(|skill| &skill.effects) {
            if converts(&model.rows[effect.row])
                && !blind(effect.cumulative.as_ref(), &[&effect.condition, &effect.release])
            {
                return None;
            }
        }
        for skill in &model.cond {
            let effects = skill.updater.effects();
            for effect in effects {
                let checkers = [&effect.trigger, &effect.condition, &effect.reset];
                if converts(&model.rows[effect.row]) && !blind(effect.cumulative.as_ref(), &checkers) {
                    return None;
                }
            }
            for updater in &skill.updater.updaters {
                if converts(&model.rows[effects[updater.effect].row]) && !blind(None, &[&updater.release]) {
                    return None;
                }
            }
        }
        if play.frames.len() != delta_times.len() {
            return None;
        }
        for (frame, &delta) in play.frames.iter().zip(delta_times) {
            if model.frame_timed(frame.time_ms, &frame.judged, delta).is_err()
                || model.draws() != 0
                || model.converted_judgements() != 0
            {
                return None;
            }
        }
    }
    Some(relabels)
}

/// The life range of the formation `deck` through `play` ([`play_life_range`]) when it holds, for a formation that
/// changes no judgement of the play but Perfect and Just into one another.
pub(crate) fn life_range(master: &Master, deck: &[Performer], play: &LivePlay) -> Option<(i32, i32)> {
    let range = play_life_range(master, play)?;
    (!deck.iter().any(|performer| leaves_life_range(master, performer, range))).then_some(range)
}

/// The Perfect and Just judgements, which the lottery takes alike ([`luck::luck_judgement_class`]).
const PERFECT: i64 = 5;
const JUST: i64 = 6;

/// What the judgement conversions of a formation can do to the judgements of a play, whatever their conditions
/// ([`play_conversions`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlayConversions {
    /// They change no judgement.
    None,
    /// They change only Perfects and Justs, each into the other, which deals no note damage.
    Relabel,
    /// They can make another change.
    Change,
}

/// What the judgement conversions the performers of `deck` hold can do to the judgements of `play`. Each judgement
/// reaches the converters as the play declares it, and the first converter that changes it makes the only change. A
/// converter changes a judgement only when the effect's target judgements hold it and it is not the judgement the
/// effect converts to, which none is for a conversion to a judgement outside 1..=6; a conversion to Just also leaves
/// alone the notes of a judgement type that has judgement timings but no Just timing. A target missing from the master
/// may be any judgement.
pub(crate) fn play_conversions(
    master: &Master,
    deck: &[Performer],
    notes: &[LiveNote],
    play: &LivePlay,
) -> PlayConversions {
    let held = held_conversions(master, deck);
    if held.is_empty() {
        return PlayConversions::None;
    }
    let timings = &master.live_judgement_timings;
    let just: FxHashSet<i64> =
        timings.iter().filter(|r| r.note_simulate_judgement == JUST).map(|r| r.note_judgement_type).collect();
    let no_just = |kind: i64| timings.iter().any(|r| r.note_judgement_type == kind) && !just.contains(&kind);
    let kinds: FxHashMap<i32, i64> = notes.iter().map(|note| (note.note_id, i64::from(note.judgement_type))).collect();
    let mut judged = FxHashSet::default();
    for note in play.frames.iter().flat_map(|frame| &frame.judged) {
        let Some(&kind) = kinds.get(&note.note_id) else { return PlayConversions::Change };
        judged.insert((i64::from(note.judgement), kind));
    }
    let damage = life::life_settings(master).ok().map(|(_, damage)| damage);
    let relabel = |judgement: i64, to: i64| {
        let harmless = damage.as_ref().is_some_and(|damage| life::harmless_judgement(damage, to as i32));
        matches!((judgement, to), (PERFECT, JUST) | (JUST, PERFECT)) && harmless
    };
    let mut out = PlayConversions::None;
    for &(effect, value, target_ids) in &held {
        let to = i64::from(crate::live::full::convert::resolve_convert_to(effect, value));
        if to == -1 {
            continue;
        }
        let Some(targets) =
            target_ids.iter().map(|&t| master.skill_target(t).map(|row| row.judgement)).collect::<Option<Vec<_>>>()
        else {
            return PlayConversions::Change;
        };
        for &(judgement, kind) in &judged {
            if targets.contains(&judgement) && judgement != to && !(to == JUST && no_just(kind)) {
                if !relabel(judgement, to) {
                    return PlayConversions::Change;
                }
                out = PlayConversions::Relabel;
            }
        }
    }
    out
}

/// Whether a conversion effect the performers hold can change a judgement of `play` beyond turning Perfects and Justs
/// into one another ([`play_conversions`]).
#[doc(hidden)]
pub fn luck_play_changes_judgements(master: &Master, deck: &[Performer], notes: &[LiveNote], play: &LivePlay) -> bool {
    play_conversions(master, deck, notes, play) == PlayConversions::Change
}

/// Whether a conversion effect the performers hold can change a judgement of `play` ([`play_conversions`]).
pub(crate) fn luck_play_converts(master: &Master, deck: &[Performer], notes: &[LiveNote], play: &LivePlay) -> bool {
    play_conversions(master, deck, notes, play) != PlayConversions::None
}

#[allow(clippy::too_many_arguments)]
pub(super) fn life_recorder(
    master: &Master,
    deck: &[Performer],
    notes: &[LiveNote],
    events: &[(i32, i32)],
    params: LiveParams,
    setup: &GekisouSetup,
    ranking: Option<&[crate::replay::RankConfirmation]>,
) -> Result<LiveModel, Error> {
    let mut relevant = master.clone();
    // A live skill's parent/pool lifecycle depends on all its effects. Keep complete groups whenever a
    // group can change life or a judgement, even when another effect of the group only changes score.
    let live_groups: FxHashSet<_> = master
        .live_skill_effects
        .iter()
        .filter(|r| life_dependency(r.skill_effect_type))
        .map(|r| (r.live_skill_id, r.level))
        .collect();
    relevant.live_skill_effects.retain(|r| live_groups.contains(&(r.live_skill_id, r.level)));
    relevant.support_skill_effects.retain(|r| life_dependency(r.skill_effect_type));
    relevant.gekisou_skill_effects.retain(|r| life_dependency(r.skill_effect_type));
    relevant.gekisou_support_skill_effects.retain(|r| life_dependency(r.skill_effect_type));
    let mut model =
        LiveModel::build(&relevant, deck, notes, events, params, Some(setup), ranking.is_some(), None, None)?;
    if model.rows.iter().any(|row| row.effect_type == 3000) {
        return Err(Error::Unsupported(
            "LUCK life DP: dynamic life maximum needs the complete native reader/cache history".into(),
        ));
    }
    if let Some(ranking) = ranking {
        model.set_rank_confirmation_timeline(ranking)?;
    }
    let check = |row: &EffectRow,
                 cumulative: Option<&conditions::Cumulative>,
                 checkers: &[&Option<Checker>]|
     -> Result<(), Error> {
        if cumulative.is_some_and(|c| !super::luck_score_bounds::deterministic_cumulative(c))
            || checkers.iter().any(|c| c.as_ref().is_some_and(|c| !deterministic_life_checker(c)))
        {
            return Err(unsupported(row, "life/judgement dependency is not deterministic under the declared play"));
        }
        Ok(())
    };
    for skill in &model.live {
        for effect in &skill.effects {
            check(&model.rows[effect.row], effect.cumulative.as_ref(), &[&effect.condition, &effect.release])?;
        }
    }
    for skill in &model.cond {
        for effect in skill.updater.effects() {
            check(
                &model.rows[effect.row],
                effect.cumulative.as_ref(),
                &[&effect.trigger, &effect.condition, &effect.reset],
            )?;
        }
        for updater in &skill.updater.updaters {
            check(&model.rows[skill.updater.effect(updater.effect).row], None, &[&updater.release])?;
        }
    }
    model.gk.as_mut().expect("Gekisou life recorder").ctrl.luck_weighted = true;
    model.phase_life = Some([0; 2]);
    Ok(model)
}
