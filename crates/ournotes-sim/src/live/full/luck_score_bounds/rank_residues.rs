//! Native rank-snapshot residue certificates. Reward buckets come from the complete factor-history replay at
//! the actual end query. Only its unchanged prefix may cancel; terminal note values cannot replace that query.

use super::*;
use crate::live::certified::{RankRemainder, RankResidueMass, rank_mean_with_partial_residues};
use std::collections::BTreeMap;

pub(super) struct Context<'a> {
    pub master: &'a Master,
    pub skills: &'a LuckSkills,
    pub deck: &'a [Performer],
    pub notes: &'a [LiveNote],
    pub events: &'a [(i32, i32)],
    pub params: LiveParams,
    pub setup: &'a GekisouSetup,
    pub play: &'a LivePlay,
    pub delta_times: &'a [f32],
    pub ranking: Option<&'a [crate::replay::RankConfirmation]>,
}

/// Prepare one window's chart-time rewards. A non-singleton native integer bucket remains unresolved. At one
/// chart time all notes observe the same joint Rush/probe class after that time's complete lottery group.
pub(super) fn request(
    window: &rank_trace::RankWindow,
    range: &LuckRangeScoreBounds,
    queries: &[QueryParts],
    notes: &FxHashMap<usize, Vec<(i32, LuckNoteBounds)>>,
) -> Option<luck_dp::rank_residues::Request> {
    if window.final_coefficient == 0
        || window.start.ordinal >= queries.len()
        || window.end.ordinal >= queries.len()
        || range.support.lower < 0
        || range.range != window.range
        || range.start_query != Some(window.start.ordinal)
        || range.end_query != window.end.ordinal
        || range.percent != window.percent
        || kept_prefix(Some(window.start.ordinal), window.end.ordinal, queries) != Some(window.start.to)
    {
        return None;
    }
    let modulus = RankRemainder::new(window.percent).modulus();
    if modulus == 1 {
        return None;
    }
    let recorded = notes.get(&window.end.ordinal).map_or(&[][..], Vec::as_slice);
    // Both collections were formed from every filed note in the same measured query. A missing or incomplete
    // detailed collection cannot certify an empty reward window.
    if recorded.len() != queries.get(window.end.ordinal)?.notes.as_ref()?.len() {
        return None;
    }
    let mut rewards = BTreeMap::<i32, [Option<u8>; 4]>::new();
    for (frame, note) in recorded {
        if *frame <= window.start.to || *frame > window.end.to {
            continue;
        }
        if note.time_ms > window.end.probability_ready || note.probability.is_none() {
            return None;
        }
        let group = rewards.entry(note.time_ms).or_insert([Some(0); 4]);
        for (class, bucket) in note.buckets.iter().enumerate() {
            group[class] = group[class].zip(*bucket).and_then(|(sum, score)| {
                (score.lower == score.upper).then(|| {
                    let residue = score.lower.rem_euclid(i32::from(modulus)) as u16;
                    ((u16::from(sum) + residue) % u16::from(modulus)) as u8
                })
            });
        }
    }
    Some(luck_dp::rank_residues::Request { modulus, rewards })
}

/// Return a complete replacement enclosure, or None only for cancellation. Structural refusals keep the
/// original certificate. The original integer support and LIFE proof are not changed by this expectation step.
pub(super) fn refine(
    context: Context<'_>,
    trace: &BoundsTrace,
    queries: &[QueryParts],
    notes: &FxHashMap<usize, Vec<(i32, LuckNoteBounds)>>,
    ranges: &mut [LuckRangeScoreBounds],
    current: F64Interval,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Option<F64Interval>, Error> {
    if cancelled() {
        return Ok(None);
    }
    let plan = match rank_trace::compile(trace, &mut *cancelled) {
        Ok(Some(plan)) if plan.ranks.len() == ranges.len() => plan,
        Ok(None) => return Ok(None),
        _ => return Ok(Some(current)),
    };
    // These are identities of native note filings, not just chart times. Restrict this optional projection to
    // one declared note and one judgement per identity; repeated or aliased inputs retain the ordinary replay.
    let mut chart_ids = FxHashSet::default();
    for (index, note) in context.notes.iter().enumerate() {
        if index.is_multiple_of(64) && cancelled() {
            return Ok(None);
        }
        if !chart_ids.insert(note.note_id) {
            return Ok(Some(current));
        }
    }
    let mut judged_ids = FxHashSet::default();
    for (index, note) in context.play.frames.iter().flat_map(|frame| &frame.judged).enumerate() {
        if index.is_multiple_of(64) && cancelled() {
            return Ok(None);
        }
        if !judged_ids.insert(note.note_id) {
            return Ok(Some(current));
        }
    }
    let mut ids = Vec::new();
    let mut requests = Vec::new();
    for window in &plan.ranks {
        if cancelled() {
            return Ok(None);
        }
        if let Some(request) = request(window, &ranges[window.id], queries, notes) {
            ids.push(window.id);
            requests.push(request);
        }
    }
    if requests.is_empty() {
        return Ok(Some(current));
    }
    #[cfg(feature = "search-diagnostics")]
    let started = std::time::Instant::now();
    let result = luck_dp::rank_residues::probabilities(
        context.master,
        context.skills,
        context.notes,
        context.events,
        context.params,
        context.setup,
        context.play,
        context.delta_times,
        context.deck,
        context.ranking,
        &requests,
        &mut *cancelled,
    );
    #[cfg(feature = "search-diagnostics")]
    {
        let output = result.as_ref().ok().and_then(Option::as_ref);
        profile::record(LuckScoreProfile {
            rank_residue_attempts: 1,
            rank_residue_windows: output.map_or(0, |out| out.laws.iter().flatten().count() as u64),
            rank_residue_unresolved_windows: output.map_or(0, |out| {
                out.laws.iter().flatten().filter(|law| law.unresolved.interval().upper() > 0.0).count() as u64
            }),
            rank_residue_peak_states: output.map_or(0, |out| out.peak_states),
            rank_residue_transitions: output.map_or(0, |out| out.transitions),
            rank_residue_ms: started.elapsed().as_secs_f64() * 1e3,
            ..Default::default()
        });
    }
    let output = match result {
        Ok(Some(output)) => output,
        Ok(None) => return Ok(None),
        Err(Error::Unsupported(_) | Error::Capacity(_)) => return Ok(Some(current)),
        Err(error) => return Err(error),
    };
    #[cfg(not(feature = "search-diagnostics"))]
    let _ = (output.peak_states, output.transitions);
    if output.laws.len() != ids.len() {
        return Err(Error::Domain("rank residue result changed its request identities".into()));
    }
    for (id, law) in ids.into_iter().zip(output.laws) {
        if cancelled() {
            return Ok(None);
        }
        let Some(law) = law else { continue };
        let range = &mut ranges[id];
        if law.residues.len() != usize::from(RankRemainder::new(range.percent).modulus()) {
            return Err(Error::Domain("rank residue result changed its modulus".into()));
        }
        let bins: Vec<_> = law
            .residues
            .into_iter()
            .enumerate()
            .map(|(residue, mass)| RankResidueMass { negative_score: false, residue: residue as u8, mass })
            .collect();
        let mean = F64Interval::new(range.mean.lower, range.mean.upper)?;
        let support = I32Interval::new(range.support.lower, range.support.upper)?;
        let refined = rank_mean_with_partial_residues(mean, support, range.percent, &bins, law.unresolved)?;
        let previous = F64Interval::new(range.bonus_mean.lower, range.bonus_mean.upper)?;
        range.bonus_mean = previous
            .intersect(refined)
            .ok_or_else(|| Error::Domain("conflicting native rank residue certificates".into()))?
            .into();
    }
    // Rebuild from bonus identities and their proved terminal coefficients. Subtracting a wide old interval
    // from the old total would lose its dependency and add that uncertainty a second time.
    let mut sum = F64Interval::ZERO;
    for window in &plan.ranks {
        let range = &ranges[window.id];
        let bonus = F64Interval::new(range.bonus_mean.lower, range.bonus_mean.upper)?;
        sum = sum.add(bonus.scale_integer(i128::from(window.final_coefficient)));
    }
    if cancelled() {
        return Ok(None);
    }
    current
        .intersect(sum)
        .map(Some)
        .ok_or_else(|| Error::Domain("conflicting terminal rank residue certificates".into()))
}
