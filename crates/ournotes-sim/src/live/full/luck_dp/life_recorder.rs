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

/// The effect type and targets of each judgement conversion effect (12006, 13005) the performers hold.
fn held_conversions<'m>(master: &'m Master, deck: &[Performer]) -> Vec<(i64, &'m [i64])> {
    let converts = |effect| matches!(effect, 12006 | 13005);
    let mut held = Vec::new();
    for p in deck {
        if let Some((id, lv)) = p.live_skill {
            held.extend(
                master
                    .live_skill_effects
                    .iter()
                    .filter(|r| r.live_skill_id == id && r.level == lv && converts(r.skill_effect_type))
                    .map(|r| (r.skill_effect_type, r.skill_target_ids.as_slice())),
            );
        }
        for &(id, lv) in &p.support_skills {
            held.extend(
                master
                    .support_skill_effects
                    .iter()
                    .filter(|r| r.support_skill_id == id && r.level == lv && converts(r.skill_effect_type))
                    .map(|r| (r.skill_effect_type, r.skill_target_ids.as_slice())),
            );
        }
        if let Some((id, lv)) = p.gekisou_skill {
            held.extend(
                master
                    .gekisou_skill_effects
                    .iter()
                    .filter(|r| r.skill_id == id && r.level == lv && converts(r.skill_effect_type))
                    .map(|r| (r.skill_effect_type, r.skill_target_ids.as_slice())),
            );
            for &(id, lv) in &p.gekisou_support_skills {
                held.extend(
                    master
                        .gekisou_support_skill_effects
                        .iter()
                        .filter(|r| r.skill_id == id && r.level == lv && converts(r.skill_effect_type))
                        .map(|r| (r.skill_effect_type, r.skill_target_ids.as_slice())),
                );
            }
        }
    }
    held
}

/// Whether a conversion effect the performers hold can change a judgement of `play`. The converter changes a
/// judgement only when the effect's target judgements hold it; a conversion to Just (13005) also leaves Just alone, and
/// the notes of a judgement type that has judgement timings but no Just timing. Another conversion is taken to change
/// every judgement its targets hold, and a target missing from the master any judgement.
pub(crate) fn luck_play_converts(master: &Master, deck: &[Performer], notes: &[LiveNote], play: &LivePlay) -> bool {
    const JUST: i64 = 6;
    let held = held_conversions(master, deck);
    if held.is_empty() {
        return false;
    }
    let timings = &master.live_judgement_timings;
    let just: FxHashSet<i64> =
        timings.iter().filter(|r| r.note_simulate_judgement == JUST).map(|r| r.note_judgement_type).collect();
    let no_just = |kind: i64| timings.iter().any(|r| r.note_judgement_type == kind) && !just.contains(&kind);
    let kinds: FxHashMap<i32, i64> = notes.iter().map(|note| (note.note_id, i64::from(note.judgement_type))).collect();
    let mut judged = FxHashSet::default();
    for note in play.frames.iter().flat_map(|frame| &frame.judged) {
        let Some(&kind) = kinds.get(&note.note_id) else { return true };
        judged.insert((i64::from(note.judgement), kind));
    }
    held.iter().any(|&(effect, target_ids)| {
        let Some(targets) =
            target_ids.iter().map(|&t| master.skill_target(t).map(|row| row.judgement)).collect::<Option<Vec<_>>>()
        else {
            return true;
        };
        judged.iter().any(|&(judgement, kind)| {
            targets.contains(&judgement) && (effect != 13005 || (judgement != JUST && !no_just(kind)))
        })
    })
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
