//! Rank bonus expectations from the residue law of their range scores.
//!
//! A rank bonus truncates the product of its range score `D` and its percentage `p` toward zero:
//! `trunc(D p / 100) = D p / 100 - C`, where the correction `C` depends only on the sign of `D` and on `D` modulo
//! `m = 100 / gcd(p, 100)` ([`RankRemainder`]). The bonus expectation is therefore `E[D] p / 100 - E[C]`, and the law
//! of `D` modulo `m` gives `E[C]` exactly. `D` is the difference of two score snapshots, a signed sum of note scores
//! and of earlier rank bonuses. When every note of that sum scores a single integer in each lottery class it may read,
//! and every earlier bonus in it has a single value, `D` modulo `m` is a function of the classes the notes read at
//! their read points. The certified lottery law propagated with that residue in its state
//! ([`LotteryRecording::residues`]) then encloses the probability of each residue.

use super::exact::{ClassRead, Observers, RankDifference, RankReads, RankTerms, exact_score, has_luck};
use super::*;
use crate::live::certified::{RankRemainder, RankResidueMass, rank_mean_with_residues};
use crate::live::full::luck_dp::{ClassResidue, LotteryRecording, ResiduePlan};

/// [`luck_exact_score`] with every rank bonus whose range score the residue law covers enclosed around its exact
/// expectation instead of within its truncation gap. The other bonuses keep the enclosure of [`luck_exact_score`].
#[allow(clippy::too_many_arguments)]
pub fn luck_exact_score_refined(
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
) -> Result<LuckExactScore, Error> {
    if !has_luck(setup) {
        return Err(refuse("the live has no LUCK range"));
    }
    let live = LotteryLive { master, skills, notes, events, params, setup, play, delta_times, ranking };
    // The evaluation and the residue laws read the one propagation of this recording.
    let recording =
        LotteryRecording::record(master, skills, notes, events, params, setup, play, delta_times, deck, ranking)?;
    let mut reads = RankReads::default();
    let observers = Observers { reads: Some(&mut reads), twin: None };
    let mut score = exact_score(&live, deck, Some(recording.law()), None, false, observers)?;
    refine_rank_bonuses(&mut score, &reads, &recording)?;
    Ok(score)
}

/// Replaces each rank bonus enclosure of `score` that a residue law refines by its intersection with the refined one,
/// and the final mean by the note mean plus the refined bonuses times their final coefficients, intersected with the
/// final mean before. Every enclosure intersected holds the true value, so their intersection does.
fn refine_rank_bonuses(
    score: &mut LuckExactScore,
    reads: &RankReads,
    recording: &LotteryRecording,
) -> Result<(), Error> {
    if reads.ranks.len() != score.ranges.len() || reads.coefficients.len() != reads.ranks.len() {
        return Err(refuse("the rank reads do not match the rank bonuses"));
    }
    let mut rank_mean = F64Interval::ZERO;
    for ((rank, &coefficient), range) in reads.ranks.iter().zip(&reads.coefficients).zip(&mut score.ranges) {
        let mut bonus = rank.bonus;
        if let Some(refined) = residue_bonus(rank, recording)? {
            bonus = bonus.intersect(refined).ok_or_else(|| refuse("a rank residue law contradicts its bonus"))?;
            range.bonus_mean = bonus.into();
        }
        rank_mean = rank_mean.add(bonus.scale_integer(i128::from(coefficient)));
    }
    let old = |bounds: RealBounds| F64Interval::new(bounds.lower, bounds.upper);
    let rank_mean = rank_mean.intersect(old(score.final_rank_mean)?).ok_or_else(|| refuse("rank means disagree"))?;
    let support = I32Interval::new(score.final_support.lower, score.final_support.upper)?.as_real();
    let mean = old(score.final_note_mean)?
        .add(rank_mean)
        .intersect(old(score.final_mean)?)
        .and_then(|mean| mean.intersect(support))
        .ok_or_else(|| refuse("the refined final mean leaves its enclosure"))?;
    score.final_rank_mean = rank_mean.into();
    score.final_mean = mean.into();
    Ok(())
}

/// The expectation of a rank bonus from the residue law of its range score, or None when the law does not cover it:
/// an earlier bonus in the range score has several values, the range score may take both signs, the percentage
/// truncates nothing, or a note in it may read a class without a single integer score.
fn residue_bonus(rank: &RankDifference, recording: &LotteryRecording) -> Result<Option<F64Interval>, Error> {
    let Some(terms) = &rank.terms else {
        return Ok(None);
    };
    let (lower, upper) = (rank.support.lower(), rank.support.upper());
    let modulus = RankRemainder::new(rank.percent).modulus();
    if rank.percent == 0 || modulus < 2 || lower == upper || (lower < 0 && upper > 0) {
        return Ok(None);
    }
    let masses = match recording.residues(&residue_plan(terms, modulus)) {
        Ok(masses) => masses,
        Err(Error::Unsupported(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    if !masses.iter().fold(F64Interval::ZERO, |total, mass| total.add(mass.interval())).contains(1.0) {
        return Ok(None);
    }
    // The native truncation reads the residue of the magnitude: for a range score of at most zero, that of its
    // negation. A zero range score has no correction under either sign.
    let negative = upper <= 0;
    let m = i64::from(modulus);
    let bins: Vec<_> = masses
        .into_iter()
        .enumerate()
        .map(|(residue, mass)| {
            let residue = (residue as i64 + terms.constant).rem_euclid(m);
            let residue = if negative { (m - residue) % m } else { residue };
            RankResidueMass { negative_score: negative, residue: residue as u8, mass }
        })
        .collect();
    Ok(Some(rank_mean_with_residues(rank.mean, rank.support, rank.percent, &bins)?))
}

/// The residue plan of a range score's notes: at each read point, for each lottery class, the residue modulo
/// `modulus` of the scores the point's notes add in that class, times their coefficients.
fn residue_plan(terms: &RankTerms, modulus: u8) -> ResiduePlan {
    let m = i64::from(modulus);
    let zero = [ClassResidue::Value(0); 4];
    let mut plan = ResiduePlan { modulus, ..ResiduePlan::default() };
    for &(read, buckets, coefficient) in &terms.notes {
        let slot = match read {
            ClassRead::Kept(time) => plan.kept.entry(time).or_insert(zero),
            ClassRead::Query { query_ms, phase } => &mut plan.queries.entry(query_ms).or_insert([zero; 3])[phase],
        };
        for (slot, bucket) in slot.iter_mut().zip(buckets) {
            let class = match bucket {
                None => ClassResidue::Unreached,
                Some((lo, hi)) if lo != hi => ClassResidue::Several,
                Some((value, _)) => ClassResidue::Value((i64::from(value) * coefficient).rem_euclid(m) as u8),
            };
            *slot = match (*slot, class) {
                (ClassResidue::Unreached, _) | (_, ClassResidue::Unreached) => ClassResidue::Unreached,
                (ClassResidue::Several, _) | (_, ClassResidue::Several) => ClassResidue::Several,
                (ClassResidue::Value(a), ClassResidue::Value(b)) => {
                    ClassResidue::Value(((i64::from(a) + i64::from(b)) % m) as u8)
                }
            };
        }
    }
    plan
}
