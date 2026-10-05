//! Accounts for the search entries: an account input (`ournotes.account/1`, see [`ournotes_sim::account`]) resolved
//! for the goal of a request and bound to the deck data it names.
//!
//! The page entry ([`crate::engine::recommend_account`]) reports every account problem in its answer. Tools that
//! search a stored account directly use [`BoundAccount`]: it serves only requests whose execution reads the facts the
//! account was resolved for, and attaches the account's scope to every result.

use crate::auxiliary::{FixedSongRanking, SongTarget};
use crate::search::physical::ProgressHook;
use crate::search::{self, Objective, SearchOutcome, SearchRequest, SearchSession, SessionBinding};
use crate::types::{Execution, RecommendationOutcome, RecommendationRequest};
use ournotes_sim::Error;
use ournotes_sim::account::{AccountInput, Exclusions, Goal, Issue, ResolvedAccount};
use ournotes_sim::cards::Roster;
use ournotes_sim::data::DeckData;
use ournotes_sim::pool::Pool;

/// The facts an execution reads: a live reads the live skill levels, a Gekisou live also the Gekisou skill levels.
pub fn goal_of(execution: &Execution) -> Goal {
    match execution {
        Execution::Power { .. } => Goal::Power,
        Execution::Skip { .. } => Goal::Skip,
        Execution::Live { gekisou: false, .. } => Goal::NormalLive,
        Execution::Live { gekisou: true, .. } => Goal::GekisouLive,
    }
}

/// Issues as one message, at most ten.
pub(crate) fn issue_list(issues: &[Issue]) -> String {
    let mut shown: Vec<String> =
        issues.iter().take(10).map(|i| format!("{} ({}): {}", i.path, i.code, i.message)).collect();
    if issues.len() > 10 {
        shown.push(format!("and {} more", issues.len() - 10));
    }
    shown.join("; ")
}

/// An account resolved for one goal, bound to the deck data it was resolved against.
pub struct BoundAccount<'m> {
    data: &'m DeckData,
    account: ResolvedAccount,
    pool: Pool<'m>,
}

impl<'m> BoundAccount<'m> {
    /// Binds an account resolved against `data` (the account must name it).
    pub fn new(data: &'m DeckData, account: ResolvedAccount) -> Result<Self, Error> {
        if data.sha256.as_deref() != Some(account.dataset_id()) {
            return Err(Error::Input("the account was resolved against another deck data".into()));
        }
        let pool = Pool::new(&data.master, account.roster())?;
        Ok(Self { data, account, pool })
    }

    /// Parses and resolves `account_json` for `goal` without exclusions. Every missing fact and every error of the
    /// account is listed in the Input error.
    pub fn resolve(data: &'m DeckData, account_json: &str, goal: Goal) -> Result<Self, Error> {
        let input = AccountInput::from_json(account_json)?;
        let resolution = input.resolve(data, goal, &Exclusions::default());
        let Some(account) = resolution.resolved else {
            let issues: Vec<Issue> = resolution.errors.into_iter().chain(resolution.missing).collect();
            return Err(Error::Input(format!("account: {}", issue_list(&issues))));
        };
        Self::new(data, account)
    }

    pub fn account(&self) -> &ResolvedAccount {
        &self.account
    }

    pub fn roster(&self) -> &Roster {
        self.account.roster()
    }

    /// The account's scope, as attached to results under `resolvedContext.account`.
    pub fn scope(&self) -> serde_json::Value {
        self.account.scope()
    }

    fn check(&self, request: &RecommendationRequest) -> Result<(), Error> {
        if goal_of(&request.execution) != self.account.goal() {
            return Err(Error::Input("the account was resolved for another goal than this execution".into()));
        }
        Ok(())
    }

    fn check_objective(&self, objective: &Objective) -> Result<(), Error> {
        let matches = matches!(
            (self.account.goal(), objective.inner()),
            (Goal::Power, Objective::Power { .. }) | (Goal::Skip, Objective::SkipScore { .. })
        );
        if matches { Ok(()) } else { Err(Error::Input("the account was resolved for another goal".into())) }
    }

    fn attach_scope(&self, outcome: &mut RecommendationOutcome) {
        outcome.resolved_context["account"] = self.scope();
    }

    /// [`crate::engine::recommend`] with this account's roster; the result carries the account's scope.
    pub fn recommend(&self, request: &RecommendationRequest) -> Result<RecommendationOutcome, Error> {
        self.recommend_hooked(request, None)
    }

    /// [`Self::recommend`] with progress reports; each report carries the same scope.
    pub fn recommend_with_progress(
        &self,
        request: &RecommendationRequest,
        progress: crate::engine::Progress<'_>,
    ) -> Result<RecommendationOutcome, Error> {
        let crate::engine::Progress { interval, report } = progress;
        let mut forward = |out: RecommendationOutcome| report(&out);
        self.recommend_hooked(request, Some(ProgressHook { interval, report: &mut forward }))
    }

    pub(crate) fn recommend_hooked(
        &self,
        request: &RecommendationRequest,
        progress: Option<ProgressHook<'_>>,
    ) -> Result<RecommendationOutcome, Error> {
        self.check(request)?;
        let mut scoped;
        let progress = match progress {
            Some(ProgressHook { interval, report }) => {
                scoped = move |mut out: RecommendationOutcome| {
                    self.attach_scope(&mut out);
                    report(out)
                };
                Some(ProgressHook { interval, report: &mut scoped })
            }
            None => None,
        };
        let mut outcome = crate::engine::recommend_hooked(self.data, self.roster(), request, progress)?;
        self.attach_scope(&mut outcome);
        Ok(outcome)
    }

    /// A resumable physical-deck session over this account's roster. The binding must name the account's deck data
    /// and revision.
    pub fn start_search_session(
        &self,
        request: &RecommendationRequest,
        binding: SessionBinding,
    ) -> Result<SearchSession<'m>, Error> {
        let started = search::session_start_clock();
        self.check(request)?;
        if binding.dataset_id != self.account.dataset_id() || binding.input_revision != self.account.revision() {
            return Err(Error::Input("session dataset/revision differs from the account".into()));
        }
        SearchSession::new(self.data, self.roster(), request, binding, self.scope(), started)
    }

    /// [`crate::auxiliary::evaluate_fixed`] with this account's roster.
    pub fn evaluate_fixed(
        &self,
        request: &RecommendationRequest,
        members: [i64; 5],
        snaps: [Option<i64>; 5],
    ) -> Result<RecommendationOutcome, Error> {
        self.check(request)?;
        let mut outcome = crate::auxiliary::evaluate_fixed(self.data, self.roster(), request, members, snaps)?;
        self.attach_scope(&mut outcome);
        Ok(outcome)
    }

    /// [`crate::auxiliary::rank_fixed_songs`] with this account's roster.
    pub fn rank_fixed_songs(
        &self,
        request: &RecommendationRequest,
        members: [i64; 5],
        snaps: [Option<i64>; 5],
        targets: &[SongTarget],
    ) -> Result<FixedSongRanking, Error> {
        self.check(request)?;
        let mut outcome =
            crate::auxiliary::rank_fixed_songs(self.data, self.roster(), request, members, snaps, targets)?;
        outcome.account_scope = Some(self.scope());
        for row in &mut outcome.results {
            self.attach_scope(&mut row.evaluation);
        }
        Ok(outcome)
    }

    /// The power and score of one physical deck (slot 2 leads) under a power or skip objective.
    pub fn evaluate_deck(
        &self,
        members: [i64; 5],
        snaps: [Option<i64>; 5],
        objective: &Objective,
    ) -> Result<(i32, Option<i32>), Error> {
        self.check_objective(objective)?;
        let deck = self.pool.deck(members, snaps, [0, 1, 2, 3, 4])?;
        search::evaluate(&self.pool, &deck, objective)
    }

    /// The canonical power/skip search over this account's roster.
    pub fn search(&self, request: &SearchRequest) -> Result<SearchOutcome, Error> {
        self.check_objective(&request.objective)?;
        search::search(&self.pool, request)
    }
}

/// The roster of `account_json` for the facts `execution` reads, for tools that run internal requests on a stored
/// account. Every account problem is listed in the Input error.
pub fn roster(data: &DeckData, account_json: &str, execution: &Execution) -> Result<Roster, Error> {
    Ok(BoundAccount::resolve(data, account_json, goal_of(execution))?.roster().clone())
}
