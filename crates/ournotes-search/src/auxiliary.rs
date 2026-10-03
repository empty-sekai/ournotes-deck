//! Fixed physical-deck evaluation and fixed-deck song ranking, sharing the search model.
use crate::clock::Instant;
use crate::handler::reject_unsupported_lifecycle;
use crate::search::{Completion, physical::arithmetic};
use crate::types::*;
use ournotes_sim::{Error, cards::Roster, data::DeckData};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, time::Duration};

/// Evaluate a physical deck using an already built problem, without rebuilding the pool.
/// Alternative decks are never substituted; constraints and deadlines still apply.
pub fn evaluate_built(
    built: &crate::handler::BuiltProblem<'_>,
    members: [i64; 5],
    snaps: [Option<i64>; 5],
) -> Result<RecommendationOutcome, Error> {
    crate::search::dispatch::execute(built, Some((members, snaps)), Instant::now(), 0.0)
}

/// Evaluate exactly these physical slots and paired Snaps through the same
/// context, arithmetic, root law and payoff path used by physical deck search.
/// This API does not infer missing account facts or certify the model.
/// Constraints still apply; alternative decks are never substituted.
pub fn evaluate_fixed(
    data: &DeckData,
    roster: &Roster,
    request: &RecommendationRequest,
    members: [i64; 5],
    snaps: [Option<i64>; 5],
) -> Result<RecommendationOutcome, Error> {
    let mut request = request.clone();
    request.k = 1;
    request.strategy = Strategy::Exhaustive;
    let start = Instant::now();
    let built = crate::handler::build_card_pool(data, roster, &request)?;
    crate::search::dispatch::execute(&built, Some((members, snaps)), start, start.elapsed().as_secs_f64() * 1000.0)
}

/// Explicit scenario identity for each chart; special-mode IDs are never
/// guessed from a base song or reused from the first ranking row.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SongTarget {
    pub score_id: i64,
    pub scenario: Scene,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixedSongResult {
    pub score_id: i64,
    pub evaluation: RecommendationOutcome,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixedSongRanking {
    pub completion: Completion,
    /// Populated by the strict snapshot facade, including for empty results.
    pub owned_snapshot_scope: Option<serde_json::Value>,
    /// Ranked complete rows only. A partially evaluated root law is excluded.
    pub results: Vec<FixedSongResult>,
    /// In original request order; no result is implied for these charts.
    pub remaining_score_ids: Vec<i64>,
    pub elapsed_ms: f64,
}

/// Rank one unchanged physical deck on explicitly selected songs, using the
/// same fixed evaluator and declared root law. Every song resolves power and
/// conditions afresh. Tie order is expected utility, power, then score ID.
/// The shared budget includes all songs; only complete evaluations enter rank.
/// A song-specific stream or duration requires separate evaluate_fixed calls.
pub fn rank_fixed_songs(
    data: &DeckData,
    roster: &Roster,
    request: &RecommendationRequest,
    members: [i64; 5],
    snaps: [Option<i64>; 5],
    targets: &[SongTarget],
) -> Result<FixedSongRanking, Error> {
    let start = Instant::now();
    if targets.is_empty() || targets.len() > 10_000 {
        return Err(Error::Input("song ranking requires 1..=10000 targets".into()));
    }
    let mut ids = HashSet::new();
    if targets.iter().any(|target| !ids.insert(target.score_id)) {
        return Err(Error::Input("song ranking score IDs must be unique".into()));
    }
    if matches!(request.execution, Execution::Power { .. }) {
        return Err(Error::Input("song ranking requires Skip or Live execution".into()));
    }
    let atom_count = if matches!(request.execution, Execution::Live { .. }) {
        request.seed_law.as_ref().map_or(0, |law| law.atoms.len())
    } else {
        1
    };
    if targets.len().saturating_mul(atom_count) > MAX_RESULT_ATOMS {
        return Err(Error::Capacity("song ranking exceeds bounded total atom capacity".into()));
    }
    if matches!(request.execution, Execution::Live { play: PlayPolicy::Stream { .. }, .. })
        || request.simulation.music_length_ms.is_some()
        || request.simulation.score_music_length_ms.is_some()
    {
        return Err(Error::Input("song-specific streams and durations cannot be reused across song ranking".into()));
    }
    reject_unsupported_lifecycle(
        request.network_confirmations.as_deref(),
        request.simulation.live_finished_from_frame,
    )?;
    // Validate common inputs even when no evaluation budget remains. This uses
    // the same preparation path with a zero budget, so it cannot simulate a
    // candidate or turn an invalid template/physical deck into a timeout.
    let song_request = |target: &SongTarget| {
        let mut r = request.clone();
        r.scenario = Some(target.scenario.clone());
        r.execution = match &request.execution {
            Execution::Skip { .. } => Execution::Skip { score_id: target.score_id },
            Execution::Live { gekisou, play, .. } => {
                Execution::Live { score_id: target.score_id, gekisou: *gekisou, play: play.clone() }
            }
            Execution::Power { .. } => unreachable!("rejected above"),
        };
        r
    };
    let mut preflight = song_request(&targets[0]);
    preflight.limits.time_limit_ms = Some(0);
    preflight.limits.max_candidates = Some(0);
    evaluate_fixed(data, roster, &preflight, members, snaps)?;
    let mut ranked = Vec::new();
    let mut consumed = 0u64;
    for target in targets {
        if request.limits.time_limit_ms.is_some_and(|ms| start.elapsed() >= Duration::from_millis(ms))
            || request.limits.max_candidates.is_some_and(|n| consumed >= n)
        {
            break;
        }
        let mut r = song_request(target);
        r.limits.time_limit_ms = request
            .limits
            .time_limit_ms
            .map(|ms| ms.saturating_sub(start.elapsed().as_millis().min(u64::MAX as u128) as u64));
        r.limits.max_candidates = request.limits.max_candidates.map(|n| n.saturating_sub(consumed));
        let evaluation = evaluate_fixed(data, roster, &r, members, snaps)?;
        consumed = consumed.saturating_add(evaluation.telemetry.leaves.visited);
        if evaluation.completion != Completion::Complete || evaluation.results.len() != 1 {
            break;
        }
        let deck = &evaluation.results[0];
        let numerator = deck.expected_payoff.numerator.parse::<i128>().map_err(|_| arithmetic())?;
        let denominator = deck.expected_payoff.denominator.parse::<u128>().map_err(|_| arithmetic())?;
        ranked.push((numerator, denominator, deck.power, FixedSongResult { score_id: target.score_id, evaluation }));
    }
    // The template has one metric and one root law, so all complete rows have
    // the same denominator. Compare integers directly without cross products.
    if let Some(first) = ranked.first()
        && ranked.iter().any(|row| row.1 != first.1)
    {
        return Err(Error::Domain("song ranking changed the declared probability mass".into()));
    }
    let completed = ranked.len();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.2.cmp(&a.2)).then_with(|| a.3.score_id.cmp(&b.3.score_id)));
    Ok(FixedSongRanking {
        completion: if completed == targets.len() { Completion::Complete } else { Completion::TimedOut },
        owned_snapshot_scope: None,
        results: ranked.into_iter().map(|row| row.3).collect(),
        remaining_score_ids: targets[completed..].iter().map(|target| target.score_id).collect(),
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
    })
}
