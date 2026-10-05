//! Fixed physical-deck evaluation and fixed-deck song ranking, sharing the search model.
use crate::clock::Instant;
use crate::handler::reject_unsupported_lifecycle;
use crate::search::Completion;
use crate::types::*;
use ournotes_sim::{Error, cards::Roster, data::DeckData};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, time::Duration};

mod song_rank;

/// Evaluate a physical deck using an already built problem, without rebuilding the pool.
/// Alternative decks are never substituted; constraints and deadlines still apply.
pub fn evaluate_built(
    built: &crate::handler::BuiltProblem<'_>,
    members: [i64; 5],
    snaps: [Option<i64>; 5],
) -> Result<RecommendationOutcome, Error> {
    crate::search::dispatch::execute(built, Some((members, snaps)), Instant::now(), 0.0, None)
}

/// Evaluate exactly these physical slots and paired Snaps through the same
/// context, arithmetic, performance orders and payoff path used by deck search.
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
    let build_ms = start.elapsed().as_secs_f64() * 1000.0;
    crate::search::dispatch::execute(&built, Some((members, snaps)), start, build_ms, None)
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
    /// Account scope when called through BoundAccount.
    pub account_scope: Option<serde_json::Value>,
    /// Proved ordering among the completed evaluations. Unevaluated songs can still outrank this prefix.
    pub results: Vec<FixedSongResult>,
    /// Completed evaluations whose mutual ordering still needs narrower certificates, in request order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unranked_results: Vec<FixedSongResult>,
    /// In original request order; no result is implied for these charts.
    pub remaining_score_ids: Vec<i64>,
    pub elapsed_ms: f64,
}

/// Rank one unchanged physical deck on explicitly selected songs, using the
/// same fixed evaluator and performance orders. Every song resolves power and
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
    let mut evaluated = Vec::new();
    let mut values = Vec::new();
    let mut interrupted = Completion::TimedOut;
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
        if evaluation.completion != Completion::Complete {
            interrupted = evaluation.completion;
            break;
        }
        if evaluation.results.len() != 1 {
            return Err(Error::Domain("complete fixed song evaluation must return exactly one deck".into()));
        }
        values.push(song_rank::SongValue::from_deck(target.score_id, &evaluation.results[0])?);
        evaluated.push(Some(FixedSongResult { score_id: target.score_id, evaluation }));
    }
    let completed = evaluated.len();
    let prefix = song_rank::ranked_prefix(&values)?;
    let mut results = Vec::with_capacity(prefix.len());
    for index in prefix {
        results.push(evaluated[index].take().expect("each ranked song appears once"));
    }
    let unranked_results: Vec<_> = evaluated.into_iter().flatten().collect();
    let completion = if completed < targets.len() {
        interrupted
    } else if unranked_results.is_empty() {
        Completion::Complete
    } else {
        Completion::RefinementRequired
    };
    Ok(FixedSongRanking {
        completion,
        owned_snapshot_scope: None,
        account_scope: None,
        results,
        unranked_results,
        remaining_score_ids: targets[completed..].iter().map(|target| target.score_id).collect(),
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
    })
}
