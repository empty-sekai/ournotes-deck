//! Exact score-rank statistics for a declared play with 120 equally weighted skill orders.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{ReplayRequest, ReplaySession, json};
use crate::Error;
use crate::live::full::ScoreProgram;

pub const REQUEST_FORMAT: &str = "ournotes.replay-rank/1";
pub const RESULT_FORMAT: &str = "ournotes.replay-rank-result/1";
const ORDER_COUNT: usize = 120;
const MAX_POWER: i32 = 20_000_000;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PowerDomain {
    pub min: i32,
    pub max: i32,
}

impl Default for PowerDomain {
    fn default() -> Self {
        Self { min: 1, max: MAX_POWER }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RankTarget {
    /// A score threshold from the same chart data and the caller's declared result scope.
    Score { threshold: i32 },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RankAnalysisRequest {
    pub format: String,
    pub replay: ReplayRequest,
    pub target: RankTarget,
    #[serde(default)]
    pub power_domain: PowerDomain,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum RequiredPower {
    /// The first integer power in the declared domain whose mean score reaches the threshold.
    Exact {
        power: i32,
        score_sum: i64,
        previous_score_sum: Option<i64>,
    },
    OutsideDomain,
    /// Current-power statistics remain valid when a power-domain certificate is unavailable.
    Unproven {
        reason: String,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankAnalysisResult {
    pub score_id: i64,
    pub power: i32,
    pub threshold: i32,
    pub order_model: &'static str,
    /// Scores in lexicographic order of the five performer-index permutations, including equal scores.
    pub order_scores: Vec<i32>,
    pub score_sum: i64,
    pub order_count: usize,
    pub min_score: i32,
    pub max_score: i32,
    pub target_hit_count: usize,
    pub need: RequiredPower,
    pub power_domain: PowerDomain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RankAnalysisStatus {
    Running,
    Complete,
    Unsupported,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankAnalysisProgress {
    pub format: &'static str,
    pub status: RankAnalysisStatus,
    pub completed_orders: usize,
    pub total_orders: usize,
    pub result: Option<RankAnalysisResult>,
    pub code: Option<&'static str>,
    pub reason: Option<String>,
}

/// A bounded single-chart job. Between calls to `advance`, the caller can yield or drop the job.
/// The parsed session data is shared; recorded programs are released at a terminal state.
pub struct RankAnalysisJob {
    session: ReplaySession,
    request: RankAnalysisRequest,
    order: [usize; 5],
    programs: Vec<Arc<ScoreProgram>>,
    progress: RankAnalysisProgress,
}

impl ReplaySession {
    pub fn start_rank_analysis(&self, mut request: RankAnalysisRequest) -> Result<RankAnalysisJob, Error> {
        if request.format != REQUEST_FORMAT {
            return Err(Error::Input(format!("unsupported rank analysis format {}", request.format)));
        }
        let domain = request.power_domain;
        if domain.min < 1 || domain.min > domain.max || domain.max > MAX_POWER {
            return Err(Error::Input(
                "rank analysis powerDomain must be a nonempty integer interval in 1..=20000000".into(),
            ));
        }
        let RankTarget::Score { threshold } = request.target;
        if threshold < 0 || !(1..=MAX_POWER).contains(&request.replay.power) {
            return Err(Error::Input(
                "rank analysis needs a nonnegative score threshold and power in 1..=20000000".into(),
            ));
        }
        if request.replay.performers.len() != 5 || !request.replay.complete {
            return Err(Error::Input("rank analysis requires five performers and a complete replay".into()));
        }
        // This job supplies the skill order and collects terminal scores rather than per-frame traces.
        request.replay.trace = false;
        request.replay.skill_order = vec![0, 1, 2, 3, 4];
        let mut job = RankAnalysisJob {
            session: self.clone(),
            request,
            order: [0, 1, 2, 3, 4],
            programs: Vec::with_capacity(ORDER_COUNT),
            progress: RankAnalysisProgress {
                format: RESULT_FORMAT,
                status: RankAnalysisStatus::Running,
                completed_orders: 0,
                total_orders: ORDER_COUNT,
                result: None,
                code: None,
                reason: None,
            },
        };
        if job.request.replay.raw_runtime.is_some() {
            job.unsupported("raw-runtime", "Rank analysis requires declared judged-stream input".into());
        }
        Ok(job)
    }

    pub fn start_rank_analysis_json(&self, request_json: &str) -> Result<RankAnalysisJob, Error> {
        let request =
            serde_json::from_str(request_json).map_err(|error| Error::Input(format!("rank analysis JSON: {error}")))?;
        self.start_rank_analysis(request)
    }
}

impl RankAnalysisJob {
    pub fn status(&self) -> &RankAnalysisProgress {
        &self.progress
    }

    pub fn status_json(&self) -> Result<String, Error> {
        json(&self.progress)
    }

    /// Run at most `max_orders` complete permutations. A partial prefix is progress, not a distribution.
    pub fn advance(&mut self, max_orders: usize) -> Result<&RankAnalysisProgress, Error> {
        if !(1..=ORDER_COUNT).contains(&max_orders) {
            return Err(Error::Input("rank analysis step size must be 1..=120".into()));
        }
        if self.progress.status != RankAnalysisStatus::Running {
            return Ok(&self.progress);
        }
        let end = (self.progress.completed_orders + max_orders).min(ORDER_COUNT);
        while self.progress.completed_orders < end {
            self.request.replay.skill_order.copy_from_slice(&self.order);
            let (_, program) = match self.session.run_inner(&self.request.replay, true) {
                Ok(value) => value,
                Err(Error::Unsupported(reason)) => {
                    self.unsupported("unsupported-domain", reason);
                    return Ok(&self.progress);
                }
                Err(error) => return Err(error),
            };
            let program =
                program.ok_or_else(|| Error::Game("rank analysis completed without a score program".into()))?;
            self.programs.push(program);
            self.progress.completed_orders += 1;
            if self.progress.completed_orders < ORDER_COUNT {
                next_order(&mut self.order);
            }
        }
        if self.progress.completed_orders == ORDER_COUNT {
            self.progress.result = Some(self.summarize());
            self.progress.status = RankAnalysisStatus::Complete;
            self.programs.clear();
        }
        Ok(&self.progress)
    }

    pub fn advance_json(&mut self, max_orders: usize) -> Result<String, Error> {
        self.advance(max_orders)?;
        self.status_json()
    }

    fn unsupported(&mut self, code: &'static str, reason: String) {
        self.progress.status = RankAnalysisStatus::Unsupported;
        self.progress.code = Some(code);
        self.progress.reason = Some(reason);
        self.programs.clear();
    }

    fn summarize(&self) -> RankAnalysisResult {
        let RankTarget::Score { threshold } = self.request.target;
        let power = self.request.replay.power;
        let mut scratch = Vec::new();
        let order_scores: Vec<_> =
            self.programs.iter().map(|program| program.evaluate_into(power, &mut scratch)).collect();
        let score_sum = order_scores.iter().map(|&score| i64::from(score)).sum();
        let min_score = *order_scores.iter().min().expect("all 120 orders are complete");
        let max_score = *order_scores.iter().max().expect("all 120 orders are complete");
        let target_hit_count = order_scores.iter().filter(|&&score| score >= threshold).count();
        let need = required_power(&self.programs, threshold, self.request.power_domain);
        RankAnalysisResult {
            score_id: self.request.replay.score_id,
            power,
            threshold,
            order_model: "uniformSkillOrder120",
            order_scores,
            score_sum,
            order_count: ORDER_COUNT,
            min_score,
            max_score,
            target_hit_count,
            need,
            power_domain: self.request.power_domain,
        }
    }
}

fn next_order(order: &mut [usize; 5]) {
    let pivot = (0..4).rev().find(|&index| order[index] < order[index + 1]).expect("another order remains");
    let next = (pivot + 1..5).rev().find(|&index| order[index] > order[pivot]).expect("a greater suffix entry exists");
    order.swap(pivot, next);
    order[pivot + 1..].reverse();
}

fn required_power(programs: &[Arc<ScoreProgram>], threshold: i32, domain: PowerDomain) -> RequiredPower {
    if programs.iter().any(|program| program.certify_nondecreasing(domain.min, domain.max).is_none()) {
        return RequiredPower::Unproven {
            reason: "The declared power domain has no nondecreasing score certificate".into(),
        };
    }
    let target = i128::from(threshold) * ORDER_COUNT as i128;
    let mut scratch = Vec::new();
    let mut sum_at =
        |power| programs.iter().map(|program| i128::from(program.evaluate_into(power, &mut scratch))).sum::<i128>();
    if sum_at(domain.max) < target {
        return RequiredPower::OutsideDomain;
    }
    let (mut low, mut high) = (domain.min, domain.max);
    while low < high {
        let middle = low + (high - low) / 2;
        if sum_at(middle) >= target {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    let score_sum = sum_at(low);
    let previous_score_sum = (low > domain.min).then(|| sum_at(low - 1));
    debug_assert!(score_sum >= target && previous_score_sum.is_none_or(|value| value < target));
    // Every program returns i32 and there are exactly 120, so these sums fit i64 and exact JSON integers.
    RequiredPower::Exact {
        power: low,
        score_sum: score_sum as i64,
        previous_score_sum: previous_score_sum.map(|value| value as i64),
    }
}
