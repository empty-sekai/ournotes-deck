//! Resumable deterministic physical-deck DFS. This contract is not a browser
//! performance or latest-native model certificate.

use super::*;
use crate::search::budget::{SearchBudget, now};
use crate::search::telemetry::Phase;

pub const SESSION_FORMAT: &str = "ournotes-deck.search-session-progress/2";
pub const GOAL_SPEC_VERSION: &str = "ournotes-deck.deterministic-physical-goal/1";

/// Caller-verified transport identity. The strict snapshot facade checks the
/// dataset/revision; the loader must verify dataset and objective content hashes.
/// Changing ANY field invalidates the old session, including completed results.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionBinding {
    pub job_id: String,
    pub input_revision: String,
    pub dataset_id: String,
    pub objective_hash: String,
}

/// A versioned, explicit output problem. v1 preserves the current physical
/// evaluator's public tie order. It does not mean canonicalMemberSet Top-K.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalSpec {
    pub version: &'static str,
    pub strategy: &'static str,
    pub result_identity: &'static str,
    pub top_k: usize,
    pub ranking: [&'static str; 4],
    pub legal_domain: &'static str,
}
impl Default for GoalSpec {
    fn default() -> Self {
        Self {
            version: GOAL_SPEC_VERSION,
            strategy: "exhaustive",
            result_identity: "physicalDeck",
            top_k: 5,
            ranking: [
                "exact payoff descending (common deterministic mass 1)",
                "power descending",
                "physical member public IDs lexicographically ascending",
                "paired Snap public IDs lexicographically ascending; None before every ID",
            ],
            legal_domain: "fixed eligible cultivation; five distinct characters; slot 2 leader; unique paired Snap IDs or None; declared include/exclude/leader constraints; full account lifecycle legality is not certified",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionStatus {
    Running,
    /// Cooperative cancellation preserves the entire DFS frontier and Top-K.
    Cancelled,
    /// The entire declared physical domain was traversed without lost branches.
    Exhausted,
    TimeLimit,
    CandidateLimit,
    /// A changed binding permanently invalidates and clears all old results.
    Stale,
    /// A model/input error occurred during evaluation; never a certificate.
    Failed,
}

/// Work counts cursor advances, including rejected choices/backtracking, and
/// one indivisible complete candidate evaluation. A slice deadline starts at
/// each step; the session deadline always starts before preparation and also
/// counts time spent yielded/cancelled. Neither budget participates in scoring.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StepBudget {
    pub max_work_units: u64,
    pub time_slice_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionProgress {
    pub format: &'static str,
    pub binding: SessionBinding,
    pub goal_spec: GoalSpec,
    pub status: SessionStatus,
    /// Only exhaustive traversal emits Complete. Cancellation, yielding and
    /// budget exhaustion have their distinct statuses and no Complete value.
    pub completion: Option<Completion>,
    pub optimality: Optimality,
    pub metric: Metric,
    pub player_goal: GoalDescription,
    pub resolved_context: serde_json::Value,
    pub proof_scope: &'static str,
    /// Complete, exactly evaluated candidates only, in the declared tie order.
    pub results: Vec<RecommendedDeck>,
    /// The same document as a finished recommendation's, for the work so far.
    pub telemetry: Telemetry,
    pub last_step_work_units: u64,
    pub elapsed_ms: f64,
}

enum Advance {
    Traversed,
    Candidate(PhysicalDeck),
    Exhausted,
}

// Iterative counterpart of members_rec/snaps_rec. One advance never scans an
// unbounded list or recursively consumes a branch. Each choice/backtrack is
// accounted for even when no legal candidate can be produced.
struct Cursor {
    members: Vec<usize>,
    snaps: Vec<usize>,
    required: Vec<usize>,
    leader: Option<usize>,
    physical: PhysicalDeck,
    depth: usize,
    next: [usize; 10],
    exhausted: bool,
}
impl Cursor {
    fn new(domain: &crate::domain::CandidateDomain) -> Self {
        Self {
            members: domain.members().to_vec(),
            snaps: domain.snaps().to_vec(),
            required: domain.required().to_vec(),
            leader: domain.leader(),
            physical: PhysicalDeck { members: [0; 5], snaps: [None; 5] },
            depth: 0,
            next: [0; 10],
            exhausted: !domain.is_feasible(),
        }
    }
    fn advance(&mut self, pool: &Pool) -> Advance {
        if self.exhausted {
            return Advance::Exhausted;
        }
        if self.depth == 10 {
            self.depth = 9;
            return Advance::Candidate(self.physical);
        }
        let depth = self.depth;
        let count = if depth < 5 { self.members.len() } else { self.snaps.len() + 1 };
        let choice = self.next[depth];
        if choice == count {
            if depth == 0 {
                self.exhausted = true;
                return Advance::Exhausted;
            }
            self.depth -= 1;
            return Advance::Traversed;
        }
        self.next[depth] += 1;
        if depth < 5 {
            let member = self.members[choice];
            let character = pool.members[member].character_id;
            if (depth == 2 && self.leader.is_some_and(|leader| leader != member))
                || self.physical.members[..depth].iter().any(|&i| pool.members[i].character_id == character)
                || self.required.iter().any(|&i| i != member && pool.members[i].character_id == character)
            {
                return Advance::Traversed;
            }
            self.physical.members[depth] = member;
            if self.required.iter().filter(|i| !self.physical.members[..=depth].contains(i)).count() > 4 - depth {
                return Advance::Traversed;
            }
        } else {
            let slot = depth - 5;
            let snap = if choice == 0 { None } else { Some(self.snaps[choice - 1]) };
            if snap.is_some() && self.physical.snaps[..slot].contains(&snap) {
                return Advance::Traversed;
            }
            self.physical.snaps[slot] = snap;
        }
        self.depth += 1;
        if self.depth < 10 {
            self.next[self.depth] = 0;
        }
        Advance::Traversed
    }

    /// Position-based share of the domain decided: every choice before the current path is done.
    fn fraction(&self) -> f64 {
        if self.exhausted {
            return 1.0;
        }
        let (mut done, mut scale) = (0.0, 1.0);
        for d in 0..=self.depth.min(9) {
            scale /= self.count(d) as f64;
            let index = if d < self.depth { self.next[d] - 1 } else { self.next[d] };
            done += index as f64 * scale;
        }
        done
    }

    fn count(&self, depth: usize) -> usize {
        if depth < 5 { self.members.len() } else { self.snaps.len() + 1 }
    }
}

/// Immutable inputs and an in-memory resumable frontier. Dropping the session
/// releases its pool/cache/results; persistence across Worker termination is
/// intentionally not provided. Construction is via ResolvedOwnedSnapshot.
pub struct SearchSession<'m> {
    // Borrow the entire dataset, not just Master: no chart/provenance mutation
    // can occur while a safe-Rust session is still in use.
    _data: &'m DeckData,
    prepared: BuiltProblem<'m>,
    request: RecommendationRequest,
    binding: SessionBinding,
    cursor: Cursor,
    started: Instant,
    budget: SearchBudget,
    status: SessionStatus,
    tel: Telemetry,
    top: Vec<Entry>,
    seen: HashSet<PhysicalDeck>,
    fifo: VecDeque<PhysicalDeck>,
    last_step_work_units: u64,
    song: Option<ournotes_sim::cards::SongView>,
    event: bool,
    skip: Option<ournotes_sim::live::skip::SkipEvaluator>,
}
impl<'m> SearchSession<'m> {
    pub(crate) fn new(
        data: &'m DeckData,
        roster: &Roster,
        request: &RecommendationRequest,
        binding: SessionBinding,
        owned_scope: serde_json::Value,
        started: Instant,
    ) -> Result<Self, Error> {
        let budget = SearchBudget::new(started, request.limits.time_limit_ms.map(Duration::from_millis))?;
        if [&binding.job_id, &binding.input_revision, &binding.dataset_id, &binding.objective_hash]
            .iter()
            .any(|field| field.trim().is_empty())
        {
            return Err(Error::Input("session binding fields must be nonempty".into()));
        }
        if !matches!(
            (&request.execution, &request.metric),
            (Execution::Power { .. }, Metric::Power)
                | (Execution::Skip { .. }, Metric::Score | Metric::ScoreAtLeast { .. } | Metric::CappedScore { .. })
        ) || matches!(request.strategy, Strategy::Candidate { .. })
        {
            return Err(Error::Unsupported(
                "SearchSession v1 requires deterministic Power/Skip with exhaustive physicalDeck identity".into(),
            ));
        }
        let request = request.clone();
        let mut prepared = build_card_pool(data, roster, &request)?;
        if request.network_confirmations.is_some()
            || request.simulation.music_length_ms.is_some()
            || request.simulation.score_music_length_ms.is_some()
            || request.simulation.live_finished_from_frame.is_some()
        {
            return Err(Error::Input("simulation/network inputs apply only to played Live".into()));
        }
        // The handler has already validated and compiled this execution, including empty domains.
        let song = prepared.context.plan.song.clone();
        let event = prepared.context.plan.event;
        let skip = prepared.context.plan.skip.clone();
        let cursor = Cursor::new(prepared.domain());
        let scope_key = if owned_scope.get("server").is_some() && owned_scope.get("cards").is_some() {
            "account"
        } else {
            "ownedSnapshot"
        };
        prepared.context.resolved_context[scope_key] = owned_scope;
        #[cfg(test)]
        crate::search::budget::test_clock::stage("session_prepare");
        let status = if budget.expired() { SessionStatus::TimeLimit } else { SessionStatus::Running };
        let mut tel = Telemetry::default();
        let env = &mut tel.environment;
        env.data = Some(prepared.context.data.clone());
        env.route = Some(prepared.context.route);
        env.traversal = Traversal::Session;
        env.k = request.k;
        env.time_limit_ms = request.limits.time_limit_ms;
        env.max_candidates = request.limits.max_candidates;
        env.cache_entries = request.limits.cache_entries;
        let domain = prepared.domain();
        env.domain = Some(telemetry::Domain {
            members: domain.members().len(),
            snaps: domain.snaps().len(),
            required: domain.required().len(),
            leader_fixed: domain.leader().is_some(),
        });
        tel.phases.push(Phase {
            name: "prepare",
            label: None,
            start_ms: 0.0,
            wall_ms: now().saturating_duration_since(started).as_secs_f64() * 1000.0,
            nodes: 0,
            candidates: 0,
            simulations: 0,
        });
        Ok(Self {
            _data: data,
            prepared,
            request,
            binding,
            cursor,
            started,
            budget,
            status,
            tel,
            top: Vec::new(),
            seen: HashSet::new(),
            fifo: VecDeque::new(),
            last_step_work_units: 0,
            song,
            event,
            skip,
        })
    }

    pub fn binding(&self) -> &SessionBinding {
        &self.binding
    }

    fn check_binding(&mut self, current: &SessionBinding) {
        if &self.binding != current {
            self.status = SessionStatus::Stale;
            self.top.clear();
            self.seen.clear();
            self.fifo.clear();
            self.cursor.exhausted = true;
        }
    }
    fn refresh_deadline(&mut self) {
        if matches!(self.status, SessionStatus::Running | SessionStatus::Cancelled) && self.budget.expired() {
            self.status = SessionStatus::TimeLimit;
        }
    }

    /// All result reads require the currently active input identity. A late
    /// result cannot be recovered by later supplying its former binding.
    pub fn progress(&mut self, current: &SessionBinding) -> Result<SessionProgress, Error> {
        self.check_binding(current);
        self.refresh_deadline();
        self.snapshot()
    }

    pub fn cancel(&mut self, current: &SessionBinding) -> Result<SessionProgress, Error> {
        self.check_binding(current);
        self.refresh_deadline();
        if self.status == SessionStatus::Running {
            self.status = SessionStatus::Cancelled;
        }
        self.snapshot()
    }

    /// Resume only this exact cancelled input, with its original total deadline
    /// and candidate budget. Terminal budget/stale/failed states do not restart.
    pub fn resume(&mut self, current: &SessionBinding) -> Result<SessionProgress, Error> {
        self.check_binding(current);
        self.refresh_deadline();
        match self.status {
            SessionStatus::Cancelled => self.status = SessionStatus::Running,
            SessionStatus::Running => {}
            _ => return Err(Error::Input("only a current, unexpired cancelled session can resume".into())),
        }
        self.snapshot()
    }

    pub fn step(&mut self, current: &SessionBinding, slice: StepBudget) -> Result<SessionProgress, Error> {
        let slice_started = now();
        self.check_binding(current);
        self.refresh_deadline();
        self.last_step_work_units = 0;
        if self.status != SessionStatus::Running {
            return self.snapshot();
        }
        if self.request.limits.max_candidates == Some(0) {
            self.status = SessionStatus::CandidateLimit;
            return self.snapshot();
        }
        let slice_budget = SearchBudget::new(slice_started, slice.time_slice_ms.map(Duration::from_millis))?;
        if self.tel.phases.len() == 1 {
            self.tel.phases.push(Phase {
                name: "search",
                label: None,
                start_ms: slice_started.saturating_duration_since(self.started).as_secs_f64() * 1000.0,
                wall_ms: 0.0,
                nodes: 0,
                candidates: 0,
                simulations: 0,
            });
        }
        let before = (self.tel.nodes, self.tel.leaves.visited, self.tel.leaves.simulations);
        let mut engine = Engine {
            pool: &self.prepared.pool,
            request: &self.prepared.context.request,
            metric: &self.request.metric,
            event_input: self.prepared.context.context_input.event_payoff.as_ref(),
            simulation: &self.request.simulation,
            limits: &self.request.limits,
            budget: self.budget,
            stop: None,
            tel: std::mem::take(&mut self.tel),
            rec: Recorder::new(self.started),
            correlated: false,
            resource: false,
            bound_scratch: super::super::snaps::JointScratch::default(),
            bonus_scratch: super::super::joint::BonusScratch::default(),
            order_steps: Default::default(),
            top: std::mem::take(&mut self.top),
            certified: None, // This incremental cursor evaluates deterministic Power/Skip only.
            lottery_free: None,
            seen: std::mem::take(&mut self.seen),
            fifo: std::mem::take(&mut self.fifo),
            team_scores: super::team_scores::TeamScores::new(0),
            programs: super::program_cache::ProgramCache::new(0),
            song: self.song.as_ref(),
            event: self.event,
            skip: self.skip.as_ref(),
            live: false,
            orders: Vec::new(),
            positions: Vec::new(),
            seeded: HashSet::new(),
            root_order: None,
            warm: None,
            progress: None,
            offered: None,
        };
        let run = (|| -> Result<(), Error> {
            while self.last_step_work_units < slice.max_work_units {
                if self.budget.expired() {
                    self.status = SessionStatus::TimeLimit;
                    break;
                }
                if slice_budget.expired() {
                    break;
                }
                self.last_step_work_units += 1;
                engine.tel.nodes += 1;
                match self.cursor.advance(&self.prepared.pool) {
                    Advance::Traversed => {}
                    Advance::Exhausted => {
                        self.status = SessionStatus::Exhausted;
                        break;
                    }
                    Advance::Candidate(physical) => {
                        if !engine.consider(physical)? {
                            self.status = match engine.stop {
                                Some(ExitReason::CandidateLimit) => SessionStatus::CandidateLimit,
                                _ => SessionStatus::TimeLimit,
                            };
                            break;
                        }
                        #[cfg(test)]
                        crate::search::budget::test_clock::stage("session_candidate");
                    }
                }
                #[cfg(test)]
                crate::search::budget::test_clock::stage("session_work");
            }
            Ok(())
        })();
        engine.rec.clock.add_to(&mut engine.tel.time);
        self.tel = engine.tel;
        let search = &mut self.tel.phases[1];
        search.wall_ms += now().saturating_duration_since(slice_started).as_secs_f64() * 1000.0;
        search.nodes += self.tel.nodes - before.0;
        search.candidates += self.tel.leaves.visited - before.1;
        search.simulations += self.tel.leaves.simulations - before.2;
        self.top = engine.top;
        self.seen = engine.seen;
        self.fifo = engine.fifo;
        if let Err(error) = run {
            self.status = SessionStatus::Failed;
            return Err(error);
        }
        self.refresh_deadline();
        self.snapshot()
    }

    fn snapshot(&self) -> Result<SessionProgress, Error> {
        let complete = self.status == SessionStatus::Exhausted;
        let mut telemetry = self.tel.clone();
        telemetry.memory = crate::search::telemetry::Memory::now();
        let numerator = |e: &Entry| e.evaluation.expected_payoff.numerator;
        let best = self.top.first().map_or(0, numerator);
        let kth = (self.top.len() == self.request.k).then(|| numerator(self.top.last().expect("K decks")));
        Recorder::new(self.started).close_timeline(&mut telemetry, best, kth, self.top.len());
        let proof = &mut telemetry.proof;
        (proof.complete, proof.parts, proof.parts_done) = (complete, 1, u64::from(complete));
        // A stale session's cursor no longer describes the current input.
        if self.status != SessionStatus::Stale {
            let c = &self.cursor;
            proof.fraction = Some(c.fraction());
            if !complete {
                let done = if c.depth > 0 { c.next[0] - 1 } else { c.next[0] };
                (proof.top_level_done, proof.top_level_total) = (Some(done as u64), Some(c.count(0) as u64));
            }
        }
        proof.best = self.top.first().map(|e| numerator(e).to_string());
        proof.kth = kth.map(|v| v.to_string());
        Ok(SessionProgress {
            format: SESSION_FORMAT,
            binding: self.binding.clone(),
            goal_spec: GoalSpec { top_k: self.request.k, ..GoalSpec::default() },
            status: self.status,
            completion: complete.then_some(Completion::Complete),
            optimality: if complete { Optimality::Proven } else { Optimality::Unproven },
            metric: self.request.metric.clone(),
            player_goal: self.prepared.context.player_goal.clone(),
            resolved_context: self.prepared.context.resolved_context.clone(),
            proof_scope: "conditional exhaustive physical Top-K under the declared deterministic model, fixed eligible cultivation and v1 tie; no full-account, current-native or browser performance certificate",
            results: self
                .top
                .iter()
                .cloned()
                .map(|entry| entry.wire(&self.request.metric))
                .collect::<Result<_, _>>()?,
            telemetry,
            last_step_work_units: self.last_step_work_units,
            elapsed_ms: now().saturating_duration_since(self.started).as_secs_f64() * 1000.0,
        })
    }
}

#[cfg(test)]
#[path = "session/tests.rs"]
mod tests;
