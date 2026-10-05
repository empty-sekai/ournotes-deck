//! Strict, goal-scoped owned facts. This adapter does not certify an account
//! or the game model. Unknown facts never become persisted default cultivation.

use crate::auxiliary::{FixedSongRanking, SongTarget};
use crate::search::physical::ProgressHook;
use crate::search::{self, Objective, SearchOutcome, SearchRequest};
use crate::search::{SearchSession, SessionBinding};
use crate::types::{Execution, RecommendationOutcome, RecommendationRequest};
use ournotes_sim::data::DeckData;
use ournotes_sim::pool::Pool;
use ournotes_sim::{
    Error,
    cards::{OwnedMember, OwnedSnap, Player, Roster},
    master::Master,
    memory::MemoryState,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Coverage {
    Complete,
    Partial,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoalDependencies {
    Power,
    Skip,
    NormalLive,
    GekisouLive,
}

impl GoalDependencies {
    /// The facts an execution reads: Live needs ordinary member skill levels, Gekisou Live also Gekisou levels.
    pub fn of(execution: &Execution) -> Self {
        match execution {
            Execution::Power { .. } => Self::Power,
            Execution::Skip { .. } => Self::Skip,
            Execution::Live { gekisou: false, .. } => Self::NormalLive,
            Execution::Live { gekisou: true, .. } => Self::GekisouLive,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnedFacts {
    pub member_ids: Vec<i64>,
    pub snap_ids: Vec<i64>,
    pub member_coverage: Coverage,
    pub snap_coverage: Coverage,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemberFact {
    pub id: i64,
    pub level: Option<i64>,
    pub exp: Option<i64>,
    pub awake: Option<i64>,
    pub rank: Option<i64>,
    pub live_skill_level: Option<i64>,
    pub gekisou_skill_level: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapFact {
    pub id: i64,
    pub level: Option<i64>,
    pub exp: Option<i64>,
    pub rank: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EligibleCards {
    pub members: Vec<MemberFact>,
    pub snaps: Vec<SnapFact>,
}

/// Lists, rather than JSON maps, allow duplicate identities to be rejected.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdValue {
    pub id: i64,
    pub value: i64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RankFacts {
    pub coverage: Coverage,
    pub values: Vec<IdValue>,
}

/// Unknowns are preserved in saved partial inputs; owned=false is an explicit
/// player statement and cannot be inferred from a missing catalog entry.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BandItemFact {
    pub id: i64,
    pub owned: Option<bool>,
    pub level: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BandItemFacts {
    pub coverage: Coverage,
    pub values: Vec<BandItemFact>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryFacts {
    pub music_ranks: Vec<IdValue>,
    pub unlocked_members: Vec<i64>,
    pub unlocked_snaps: Vec<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerFacts {
    pub character_ranks: RankFacts,
    pub character_total_rank: Option<i64>,
    pub vip_rank: Option<i64>,
    /// Some(empty) explicitly states no active items; None is unknown.
    pub band_items: Option<Vec<IdValue>>,
    /// New manual input, mutually exclusive with the legacy complete active
    /// item list. Complete requires an explicit row for each catalog identity.
    pub band_item_facts: Option<BandItemFacts>,
    /// Some(empty fields) explicitly states zero memory progress.
    pub memory: Option<MemoryFacts>,
    pub event_ids: Option<Vec<i64>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assumption {
    pub path: String,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnedSnapshot {
    pub format: String,
    pub dataset_id: String,
    pub revision: String,
    pub owned_facts: OwnedFacts,
    pub eligible: EligibleCards,
    pub player: PlayerFacts,
    /// Values are still supplied explicitly in the corresponding fact fields.
    /// These annotations never fill an unknown field.
    pub assumptions: Vec<Assumption>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Issue {
    pub path: String,
    pub code: String,
    pub message: String,
}

/// Master-derived metadata only, not certification of skill behavior.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapSkillDerivation {
    pub snap_id: i64,
    pub rank: i64,
    pub normal: Vec<(i64, i64)>,
    pub gekisou: Vec<(i64, i64)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TotalRankOrigin {
    Observed,
    DerivedCompleteRanks,
}

#[derive(Debug)]
pub struct Resolution<'m> {
    pub missing: Vec<Issue>,
    pub errors: Vec<Issue>,
    pub resolved: Option<ResolvedOwnedSnapshot<'m>>,
}

/// The projection is private. Skill levels the goal does not read are zero-valued
/// unavailable slots; they cannot be passed by an external caller to a Live
/// evaluator as known skills.
#[derive(Debug)]
pub struct ResolvedOwnedSnapshot<'m> {
    snapshot: OwnedSnapshot,
    goal: GoalDependencies,
    pool: Pool<'m>,
    projection: Roster,
    bound_data: Option<&'m DeckData>,
    total_rank_origin: TotalRankOrigin,
}

impl ResolvedOwnedSnapshot<'_> {
    pub fn snapshot(&self) -> &OwnedSnapshot {
        &self.snapshot
    }
    pub fn goal(&self) -> GoalDependencies {
        self.goal
    }
    pub fn character_total_rank(&self) -> (i64, TotalRankOrigin) {
        (self.pool.player.character_total_rank(), self.total_rank_origin)
    }
    pub fn snap_skill_derivation(&self, snap_id: i64) -> Result<SnapSkillDerivation, Error> {
        let index = self.pool.snap_index(snap_id).ok_or_else(|| Error::Input("Snap is not eligible".into()))?;
        let snap = &self.pool.snaps[index];
        Ok(SnapSkillDerivation {
            snap_id,
            rank: snap.rank,
            normal: snap.support_skills()?,
            gekisou: snap.gekisou_support_skills()?,
        })
    }
    /// Scope declaration only, never a full-account optimality certificate.
    pub fn covers_all_declared_owned_cards(&self) -> bool {
        self.snapshot.owned_facts.member_coverage == Coverage::Complete
            && self.snapshot.owned_facts.snap_coverage == Coverage::Complete
            && self.snapshot.owned_facts.member_ids.len() == self.snapshot.eligible.members.len()
            && self.snapshot.owned_facts.snap_ids.len() == self.snapshot.eligible.snaps.len()
    }
    fn check_goal(&self, objective: &Objective) -> Result<(), Error> {
        let matches = matches!(
            (self.goal, objective.inner()),
            (GoalDependencies::Power, Objective::Power { .. }) | (GoalDependencies::Skip, Objective::SkipScore { .. })
        );
        if matches { Ok(()) } else { Err(Error::Input("objective is not authorized by this resolved snapshot".into())) }
    }
    /// Uses the existing shared evaluator and deck legality checks. Slot 2 is the leader.
    pub fn evaluate_deck(
        &self,
        members: [i64; 5],
        snaps: [Option<i64>; 5],
        objective: &Objective,
    ) -> Result<(i32, Option<i32>), Error> {
        self.check_goal(objective)?;
        let deck = self.pool.deck(members, snaps, [0, 1, 2, 3, 4])?;
        search::evaluate(&self.pool, &deck, objective)
    }
    pub fn search(&self, request: &SearchRequest) -> Result<SearchOutcome, Error> {
        self.check_goal(&request.objective)?;
        search::search(&self.pool, request)
    }

    fn check_request(&self, data: &DeckData, request: &RecommendationRequest) -> Result<(), Error> {
        let bound = self.bound_data.ok_or_else(|| {
            Error::Input("shared evaluation requires resolve_data to bind the complete dataset".into())
        })?;
        if !std::ptr::eq(bound, data) {
            return Err(Error::Input("evaluation must use the complete dataset bound by resolve_data".into()));
        }
        if self.goal != GoalDependencies::of(&request.execution) {
            return Err(Error::Input("execution is not authorized by this resolved snapshot".into()));
        }
        if let Some(context) = &request.context {
            let declared: BTreeSet<_> = context.power_snapshot.event_ids.iter().copied().collect();
            let resolved: BTreeSet<_> = self.projection.player.events.iter().copied().collect();
            if declared.len() != context.power_snapshot.event_ids.len() || declared != resolved {
                return Err(Error::Input("request event snapshot differs from resolved player facts".into()));
            }
        }
        Ok(())
    }

    fn snapshot_scope(&self) -> serde_json::Value {
        let coverage = |value| match value {
            Coverage::Complete => "complete",
            Coverage::Partial => "partial",
        };
        serde_json::json!({
            "datasetId": self.snapshot.dataset_id,
            "revision": self.snapshot.revision,
            "memberCoverage": coverage(self.snapshot.owned_facts.member_coverage),
            "snapCoverage": coverage(self.snapshot.owned_facts.snap_coverage),
            "eligibleCoversDeclaredOwned": self.covers_all_declared_owned_cards(),
            "totalRankOrigin": match self.total_rank_origin {
                TotalRankOrigin::Observed => "observed", TotalRankOrigin::DerivedCompleteRanks => "derivedCompleteRanks",
            },
            "assumptions": self.snapshot.assumptions.iter().map(|a| serde_json::json!({"path":a.path,"reason":a.reason})).collect::<Vec<_>>(),
            "bandItemInput": match &self.snapshot.player.band_item_facts {
                Some(facts) => serde_json::json!({"kind":"explicitOwnership","coverage":coverage(facts.coverage),"notOwnedIds":facts.values.iter().filter(|fact|fact.owned==Some(false)).map(|fact|fact.id).collect::<Vec<_>>(),"levelValidation":"identity + represented MasterBandItemLevel + effect row; ownership and player-rank/resource prerequisites are not certified"}),
                None => serde_json::json!({"kind":"legacyCompleteActiveItems","coverage":"completeActiveItems","ownershipCoverage":"not declared by this legacy list","levelValidation":"nonempty levels require identity + represented level + effect row"})
            },
            "playerBonusEvidence": {
                "status":"conditionalCurrentCoreProjection",
                "vipRankValidation":"positive represented MasterVip rank; bonus rows do not define the domain",
                "characterRankValidation":"represented MasterCharacterRank; total thresholds do not define the local rank domain",
                "memoryEffects":{"status":"unmodeledLatestNative","currentCoreUsesDeclaredFacts":true},
                "eventEffects":{"status":"unmodeledLatestNative","currentCoreUsesDeclaredFacts":true},
                "fullAccountLegality":"unmodeled"
            },
            "scope": "validated input projection for the selected goal; account-wide legality and native model certification remain separate"
        })
    }

    /// Diagnostics only: the goal-scoped projection, for the fixed-deck audit tools.
    #[cfg(feature = "search-diagnostics")]
    pub fn diagnostic_projection(&self) -> &Roster {
        &self.projection
    }

    fn attach_scope(&self, outcome: &mut RecommendationOutcome) {
        outcome.resolved_context["ownedSnapshot"] = self.snapshot_scope();
    }

    /// Search through the shared goal/scenario boundary without exposing the
    /// private projection or granting access to unknown Live skill fields.
    pub fn recommend(&self, data: &DeckData, request: &RecommendationRequest) -> Result<RecommendationOutcome, Error> {
        self.recommend_hooked(data, request, None)
    }

    /// [`recommend`](Self::recommend) with progress reports; each report carries the same snapshot scope.
    pub fn recommend_with_progress(
        &self,
        data: &DeckData,
        request: &RecommendationRequest,
        progress: crate::engine::Progress<'_>,
    ) -> Result<RecommendationOutcome, Error> {
        let crate::engine::Progress { interval, report } = progress;
        let mut forward = |out: RecommendationOutcome| report(&out);
        self.recommend_hooked(data, request, Some(ProgressHook { interval, report: &mut forward }))
    }

    pub(crate) fn recommend_hooked(
        &self,
        data: &DeckData,
        request: &RecommendationRequest,
        progress: Option<ProgressHook<'_>>,
    ) -> Result<RecommendationOutcome, Error> {
        self.check_request(data, request)?;
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
        let mut outcome = crate::engine::recommend_hooked(data, &self.projection, request, progress)?;
        self.attach_scope(&mut outcome);
        Ok(outcome)
    }

    /// Start a deterministic physical-deck session using this dataset-bound,
    /// goal-scoped projection. The original facts and unknown skill fields are
    /// preserved, and cannot be converted into a Live capability.
    /// The session keeps the complete dataset immutably borrowed.
    ///
    /// ```compile_fail
    /// use ournotes_sim::data::DeckData;
    /// use ournotes_search::{owned_snapshot::ResolvedOwnedSnapshot, search::SessionBinding,
    ///     types::RecommendationRequest};
    /// fn cannot_change_chart(data: &mut DeckData, resolved: &ResolvedOwnedSnapshot<'_>,
    ///     request: &RecommendationRequest, binding: SessionBinding) {
    ///     let mut session = resolved.start_search_session(data, request, binding.clone()).unwrap();
    ///     data.charts.clear();
    ///     session.progress(&binding).unwrap();
    /// }
    /// ```
    pub fn start_search_session<'m>(
        &self,
        data: &'m DeckData,
        request: &RecommendationRequest,
        binding: SessionBinding,
    ) -> Result<SearchSession<'m>, Error> {
        let started = search::session_start_clock();
        self.check_request(data, request)?;
        if binding.dataset_id != self.snapshot.dataset_id || binding.input_revision != self.snapshot.revision {
            return Err(Error::Input("session dataset/revision differs from the resolved snapshot".into()));
        }
        SearchSession::new(data, &self.projection, request, binding, self.snapshot_scope(), started)
    }

    pub fn evaluate_fixed(
        &self,
        data: &DeckData,
        request: &RecommendationRequest,
        members: [i64; 5],
        snaps: [Option<i64>; 5],
    ) -> Result<RecommendationOutcome, Error> {
        self.check_request(data, request)?;
        let mut outcome = crate::auxiliary::evaluate_fixed(data, &self.projection, request, members, snaps)?;
        self.attach_scope(&mut outcome);
        Ok(outcome)
    }

    pub fn rank_fixed_songs(
        &self,
        data: &DeckData,
        request: &RecommendationRequest,
        members: [i64; 5],
        snaps: [Option<i64>; 5],
        targets: &[SongTarget],
    ) -> Result<FixedSongRanking, Error> {
        self.check_request(data, request)?;
        let mut outcome = crate::auxiliary::rank_fixed_songs(data, &self.projection, request, members, snaps, targets)?;
        outcome.owned_snapshot_scope = Some(self.snapshot_scope());
        for row in &mut outcome.results {
            self.attach_scope(&mut row.evaluation);
        }
        Ok(outcome)
    }
}

impl<'m> Resolution<'m> {
    fn new() -> Self {
        Self { missing: Vec::new(), errors: Vec::new(), resolved: None }
    }
    fn error(&mut self, path: impl Into<String>, code: &str, message: impl Into<String>) {
        self.errors.push(Issue { path: path.into(), code: code.into(), message: message.into() });
    }
    fn missing(&mut self, path: impl Into<String>) {
        self.missing.push(Issue {
            path: path.into(),
            code: "missing".into(),
            message: "required by the selected goal".into(),
        });
    }
    fn required(&mut self, path: &str, value: Option<i64>) -> Option<i64> {
        if value.is_none() {
            self.missing(path);
        }
        value
    }
    fn ids(&mut self, path: &str, ids: &[i64]) -> BTreeSet<i64> {
        let mut out = BTreeSet::new();
        for &id in ids {
            if id <= 0 {
                self.error(path, "invalid_id", "IDs must be positive integers");
            }
            if !out.insert(id) {
                self.error(path, "duplicate_id", format!("duplicate ID {id}"));
            }
        }
        out
    }
    fn values(&mut self, path: &str, values: &[IdValue], minimum: i64) -> BTreeMap<i64, i64> {
        let mut out = BTreeMap::new();
        for fact in values {
            if fact.id <= 0 || fact.value < minimum {
                self.error(path, "invalid_value", "invalid ID or value");
            }
            if out.insert(fact.id, fact.value).is_some() {
                self.error(path, "duplicate_id", format!("duplicate ID {}", fact.id));
            }
        }
        out
    }
}

impl OwnedSnapshot {
    /// Bind the full data object immutably for shared evaluation, rather than
    /// borrowing only Master while allowing charts/provenance to change. The
    /// loader remains responsible for verifying the supplied dataset identity.
    ///
    /// ```compile_fail
    /// use ournotes_sim::data::DeckData;
    /// use ournotes_search::owned_snapshot::{OwnedSnapshot, GoalDependencies};
    /// fn change_chart(snapshot: &OwnedSnapshot, data: &mut DeckData) {
    ///     let resolved = snapshot.resolve_data(data, "dataset", GoalDependencies::Power).resolved.unwrap();
    ///     data.charts.clear();
    ///     let _ = resolved.snapshot();
    /// }
    /// ```
    pub fn resolve_data<'m>(
        &self,
        data: &'m DeckData,
        expected_dataset_id: &str,
        goal: GoalDependencies,
    ) -> Resolution<'m> {
        let mut result = self.resolve(&data.master, expected_dataset_id, goal);
        if let Some(resolved) = result.resolved.as_mut() {
            resolved.bound_data = Some(data);
        }
        result
    }

    /// Typed deserialization preserves integer tokens and rejects duplicate and
    /// unknown fields at every schema level. It never round-trips through Value.
    pub fn from_json(text: &str) -> Result<Self, Error> {
        serde_json::from_str(text).map_err(|error| Error::Input(format!("owned snapshot: {error}")))
    }

    pub fn resolve<'m>(&self, master: &'m Master, expected_dataset_id: &str, goal: GoalDependencies) -> Resolution<'m> {
        let mut out = Resolution::new();
        if self.format != "ournotes.owned-snapshot/1" {
            out.error("format", "unsupported_format", "expected ournotes.owned-snapshot/1");
        }
        if self.dataset_id.is_empty() || self.dataset_id != expected_dataset_id {
            out.error("datasetId", "dataset_mismatch", "snapshot must name the supplied dataset");
        }
        if self.revision.is_empty() {
            out.error("revision", "missing_identity", "revision must not be empty");
        }
        for assumption in &self.assumptions {
            if assumption.path.is_empty() || assumption.reason.trim().is_empty() {
                out.error("assumptions", "invalid_assumption", "explicit field path and reason are required");
            }
        }
        let owned_members = out.ids("ownedFacts.memberIds", &self.owned_facts.member_ids);
        let owned_snaps = out.ids("ownedFacts.snapIds", &self.owned_facts.snap_ids);
        for &id in &owned_members {
            if master.member_card(id).is_none() {
                out.error("ownedFacts.memberIds", "unknown_id", format!("unknown member {id}"));
            }
        }
        for &id in &owned_snaps {
            if master.support_card(id).is_none() {
                out.error("ownedFacts.snapIds", "unknown_id", format!("unknown Snap {id}"));
            }
        }
        let ranks = out.values("player.characterRanks.values", &self.player.character_ranks.values, 1);
        for (&id, &rank) in &ranks {
            if master.character(id).is_none() {
                out.error("player.characterRanks", "unknown_id", format!("unknown character {id}"));
            }
            if !master.character_ranks.iter().any(|r| r.rank == rank) {
                out.error("player.characterRanks", "master_row_missing", format!("no character rank row for {rank}"));
            }
        }
        let sum = ranks.values().try_fold(0i64, |sum, rank| sum.checked_add(*rank));
        if sum.is_none() {
            out.error("player.characterRanks", "overflow", "rank sum exceeds i64");
        }
        let total = match self.player.character_total_rank {
            Some(total) => Some(total),
            None if self.player.character_ranks.coverage == Coverage::Complete => sum,
            None => {
                out.missing("player.characterTotalRank");
                None
            }
        };
        if let Some(total) = total {
            if total <= 0 {
                out.error("player.characterTotalRank", "invalid_value", "total rank must be positive");
            }
            if sum.is_some_and(|sum| total < sum) {
                out.error("player.characterTotalRank", "rank_conflict", "total is below the known local-rank sum");
            }
            let absent = master.characters.iter().filter(|row| !ranks.contains_key(&row.id)).count();
            let minimum_rank = master.character_ranks.iter().map(|row| row.rank).filter(|rank| *rank > 0).min();
            if let (Some(sum), Some(minimum_rank)) = (sum, minimum_rank) {
                let minimum = i64::try_from(absent)
                    .ok()
                    .and_then(|n| n.checked_mul(minimum_rank))
                    .and_then(|n| n.checked_add(sum));
                if minimum.is_none_or(|minimum| total < minimum) {
                    out.error(
                        "player.characterTotalRank",
                        "rank_conflict",
                        "total cannot cover known ranks plus the minimum ranks of unobserved characters",
                    );
                }
            }
            let maximum_rank = master.character_ranks.iter().map(|row| row.rank).max();
            if let (Some(sum), Some(maximum_rank)) = (sum, maximum_rank) {
                let maximum = i64::try_from(absent)
                    .ok()
                    .and_then(|n| n.checked_mul(maximum_rank))
                    .and_then(|n| n.checked_add(sum));
                if maximum.is_none_or(|maximum| total > maximum) {
                    out.error(
                        "player.characterTotalRank",
                        "rank_conflict",
                        "total exceeds the sum permitted by the supplied rank table",
                    );
                }
            }
        }
        if self.player.character_ranks.coverage == Coverage::Complete {
            if master.characters.iter().any(|row| !ranks.contains_key(&row.id)) {
                out.error("player.characterRanks", "coverage_conflict", "complete coverage is missing characters");
            }
            if let (Some(sum), Some(total)) = (sum, total) {
                if sum != total {
                    out.error(
                        "player.characterTotalRank",
                        "rank_conflict",
                        "complete local-rank sum differs from the independent total",
                    );
                }
            }
        }
        let vip = out.required("player.vipRank", self.player.vip_rank);
        if let Some(rank) = vip {
            if rank <= 0 {
                out.error("player.vipRank", "invalid_value", "VIP rank must be positive");
            } else if master.vip_ranks.is_empty() {
                out.error("player.vipRank", "unsupported_master", "VIP rank requires MasterVip, not bonus rows");
            } else if master.vip_rank(rank).is_none() {
                out.error("player.vipRank", "invalid_value", "VIP rank is not represented in MasterVip");
            }
        }
        let items = resolve_band_items(master, &self.player, &mut out);
        let events = match &self.player.event_ids {
            None => {
                out.missing("player.eventIds");
                Vec::new()
            }
            Some(ids) => {
                out.ids("player.eventIds", ids);
                for &id in ids {
                    if !master.events.iter().any(|row| row.id == id) {
                        out.error("player.eventIds", "unknown_id", format!("unknown event {id}"));
                    }
                }
                ids.clone()
            }
        };
        let memory = match &self.player.memory {
            None => {
                out.missing("player.memory");
                None
            }
            Some(facts) => {
                let music_ranks = out.values("player.memory.musicRanks", &facts.music_ranks, 0);
                for &id in music_ranks.keys() {
                    if !master.memory_musics.iter().any(|r| r.id == id) {
                        out.error("player.memory.musicRanks", "unknown_id", format!("unknown memory music {id}"));
                    }
                }
                let unlocked_members = out.ids("player.memory.unlockedMembers", &facts.unlocked_members);
                let unlocked_supports = out.ids("player.memory.unlockedSnaps", &facts.unlocked_snaps);
                if !unlocked_members.is_subset(&owned_members) || !unlocked_supports.is_subset(&owned_snaps) {
                    out.error(
                        "player.memory",
                        "ownership_conflict",
                        "unlocked memory cards must be present in owned facts, even when excluded from eligibility",
                    );
                }
                Some(MemoryState { music_ranks, unlocked_members, unlocked_supports, ..MemoryState::default() })
            }
        };
        let mut members = Vec::new();
        out.ids("eligible.members", &self.eligible.members.iter().map(|m| m.id).collect::<Vec<_>>());
        for fact in &self.eligible.members {
            let path = format!("eligible.members[{}]", fact.id);
            if !owned_members.contains(&fact.id) {
                out.error(&path, "not_owned", "eligible card must be in owned facts");
            }
            let Some(row) = master.member_card(fact.id) else {
                out.error(&path, "unknown_id", "unknown member");
                continue;
            };
            if !ranks.contains_key(&row.character_id) {
                out.missing(format!("player.characterRanks[{}]", row.character_id));
            }
            let awake = out.required(&format!("{path}.awake"), fact.awake);
            let rank = out.required(&format!("{path}.rank"), fact.rank);
            if awake.is_some_and(|value| value <= 0) || rank.is_some_and(|value| value <= 0) {
                out.error(&path, "invalid_value", "awake and owned member rank must be positive");
            }
            let level = resolve_level(master, row.level_group, fact.level, fact.exp, false, &path, &mut out);
            if let Some(awake) = awake {
                let cap = master
                    .member_card_level_limits
                    .iter()
                    .find(|cap| cap.rarity == row.rarity && cap.awake_count == awake);
                match cap {
                    None => out.error(
                        format!("{path}.level"),
                        "unsupported_master",
                        "member rarity/awake level cap is missing",
                    ),
                    Some(cap) if level.is_some_and(|level| level > cap.limit_level) => {
                        out.error(format!("{path}.level"), "level_cap", "member level exceeds its current awake cap")
                    }
                    _ => {}
                }
            }
            let live = matches!(goal, GoalDependencies::NormalLive | GoalDependencies::GekisouLive);
            let live_skill_level = if live && row.live_skill_id != 0 {
                out.required(&format!("{path}.liveSkillLevel"), fact.live_skill_level)
            } else {
                None
            };
            let gekisou_skill_level = if goal == GoalDependencies::GekisouLive && row.gekisou_skill_id != 0 {
                out.required(&format!("{path}.gekisouSkillLevel"), fact.gekisou_skill_level)
            } else {
                None
            };
            for (name, value) in
                [("liveSkillLevel", fact.live_skill_level), ("gekisouSkillLevel", fact.gekisou_skill_level)]
            {
                if value.is_some_and(|value| value <= 0) {
                    out.error(format!("{path}.{name}"), "invalid_value", "known skill level must be positive");
                }
            }
            // A level without effect rows would silently play as no skill.
            if let Some(level) = live_skill_level.filter(|level| *level > 0)
                && !master.live_skill_effects.iter().any(|r| r.live_skill_id == row.live_skill_id && r.level == level)
            {
                out.error(
                    format!("{path}.liveSkillLevel"),
                    "master_row_missing",
                    "no live skill effect row for this level",
                );
            }
            if let Some(level) = gekisou_skill_level.filter(|level| *level > 0)
                && !master.gekisou_skill_effects.iter().any(|r| r.skill_id == row.gekisou_skill_id && r.level == level)
            {
                out.error(
                    format!("{path}.gekisouSkillLevel"),
                    "master_row_missing",
                    "no Gekisou skill effect row for this level",
                );
            }
            if let (Some(level), Some(awake), Some(rank)) = (level, awake, rank) {
                // Skill slots the goal does not read are deliberately unavailable, never facts.
                members.push(OwnedMember {
                    id: fact.id,
                    level: Some(level),
                    exp: fact.exp,
                    awake,
                    rank,
                    live_skill_level: live_skill_level.unwrap_or(0),
                    gekisou_skill_level: gekisou_skill_level.unwrap_or(0),
                });
            }
        }
        let mut snaps = Vec::new();
        out.ids("eligible.snaps", &self.eligible.snaps.iter().map(|s| s.id).collect::<Vec<_>>());
        for fact in &self.eligible.snaps {
            let path = format!("eligible.snaps[{}]", fact.id);
            if !owned_snaps.contains(&fact.id) {
                out.error(&path, "not_owned", "eligible Snap must be in owned facts");
            }
            let Some(row) = master.support_card(fact.id) else {
                out.error(&path, "unknown_id", "unknown Snap");
                continue;
            };
            let rank = out.required(&format!("{path}.rank"), fact.rank);
            if rank.is_some_and(|value| value <= 0) {
                out.error(&path, "invalid_value", "owned Snap rank must be positive");
            }
            let level = resolve_level(master, row.level_group, fact.level, fact.exp, true, &path, &mut out);
            if let Some(rank) = rank {
                match master.support_card_ranks.iter().find(|r| r.group == row.rank_group && r.rank == rank) {
                    None => out.error(
                        format!("{path}.rank"),
                        "master_row_missing",
                        "Snap rank row and derived skills are missing",
                    ),
                    Some(row) if level.is_some_and(|level| level > row.limit_level) => {
                        out.error(format!("{path}.level"), "level_cap", "Snap level exceeds rank cap")
                    }
                    _ => {}
                }
            }
            if let (Some(level), Some(rank)) = (level, rank) {
                snaps.push(OwnedSnap { id: fact.id, level: Some(level), exp: fact.exp, rank });
            }
        }
        if out.errors.is_empty() && out.missing.is_empty() {
            let player = Player {
                character_ranks: ranks,
                explicit_character_total_rank: total,
                band_items: items,
                vip_rank: vip.expect("checked"),
                events,
                memory,
                owned_member_card_ids: Some(owned_members),
                owned_support_card_ids: Some(owned_snaps),
            };
            let projection = Roster { player, members, snaps };
            match Pool::new(master, &projection) {
                Ok(pool) => {
                    out.resolved = Some(ResolvedOwnedSnapshot {
                        snapshot: self.clone(),
                        goal,
                        pool,
                        projection,
                        bound_data: None,
                        total_rank_origin: if self.player.character_total_rank.is_some() {
                            TotalRankOrigin::Observed
                        } else {
                            TotalRankOrigin::DerivedCompleteRanks
                        },
                    })
                }
                Err(error) => out.error("eligible", "core_resolution", error.to_string()),
            }
        }
        out
    }
}

fn check_band_item_level(master: &Master, id: i64, level: i64, path: &str, out: &mut Resolution<'_>) {
    if master.band_item(id).is_none() {
        out.error(path, "unknown_id", format!("unknown band item {id}"));
        return;
    }
    if level <= 0 || master.band_item_level(id, level).is_none() {
        out.error(path, "invalid_level", format!("{id}/{level} is not a represented MasterBandItemLevel; effect-only reserved rows do not authorize levels"));
        return;
    }
    if !master.band_item_effects.iter().any(|row| row.band_item_id == id && row.level == level) {
        out.error(path, "master_row_missing", format!("no item effect row for {id}/{level}"));
    }
}

fn resolve_band_items(master: &Master, player: &PlayerFacts, out: &mut Resolution<'_>) -> BTreeMap<i64, i64> {
    if player.band_item_facts.is_some() && player.band_items.is_some() {
        out.error(
            "player.bandItemFacts",
            "input_conflict",
            "bandItemFacts and the legacy bandItems declaration are mutually exclusive",
        );
    }
    let needs_catalog =
        player.band_item_facts.is_some() || player.band_items.as_ref().is_some_and(|items| !items.is_empty());
    if needs_catalog && (master.band_items.is_empty() || master.band_item_levels.is_empty()) {
        let path = if player.band_item_facts.is_some() { "player.bandItemFacts" } else { "player.bandItems" };
        out.error(path, "unsupported_master", "MasterBandItem and MasterBandItemLevel are required; legacy effect-only datasets do not authorize this input capability");
        return BTreeMap::new();
    }
    if let Some(facts) = &player.band_item_facts {
        let ids = out.ids("player.bandItemFacts.values", &facts.values.iter().map(|fact| fact.id).collect::<Vec<_>>());
        let mut items = BTreeMap::new();
        for fact in &facts.values {
            let path = format!("player.bandItemFacts.values[{}]", fact.id);
            if master.band_item(fact.id).is_none() {
                out.error(&path, "unknown_id", "band item is not in the bound catalog");
                continue;
            }
            match fact.owned {
                None => {
                    out.missing(format!("{path}.owned"));
                    if let Some(level) = fact.level {
                        check_band_item_level(master, fact.id, level, &format!("{path}.level"), out);
                    }
                }
                Some(false) => {
                    if fact.level.is_some() {
                        out.error(
                            format!("{path}.level"),
                            "cultivation_conflict",
                            "an explicitly unowned item must have unknown/absent level, not level 0 or 1",
                        );
                    }
                }
                Some(true) => {
                    if let Some(level) = out.required(&format!("{path}.level"), fact.level) {
                        check_band_item_level(master, fact.id, level, &format!("{path}.level"), out);
                        items.insert(fact.id, level);
                    }
                }
            }
        }
        for row in &master.band_items {
            if !ids.contains(&row.id) {
                let path = format!("player.bandItemFacts.values[{}].owned", row.id);
                if facts.coverage == Coverage::Complete {
                    out.error(
                        path,
                        "coverage_conflict",
                        "complete item coverage must explicitly state ownership for every catalog ID",
                    );
                } else {
                    out.missing(path);
                }
            }
        }
        items
    } else {
        match &player.band_items {
            None => {
                out.missing("player.bandItems");
                BTreeMap::new()
            }
            Some(facts) => {
                let items = out.values("player.bandItems", facts, 1);
                for (&id, &level) in &items {
                    check_band_item_level(master, id, level, &format!("player.bandItems[{id}]"), out);
                }
                items
            }
        }
    }
}

fn resolve_level(
    master: &Master,
    group: i64,
    level: Option<i64>,
    exp: Option<i64>,
    snap: bool,
    path: &str,
    out: &mut Resolution<'_>,
) -> Option<i64> {
    if level.is_none() && exp.is_none() {
        out.missing(format!("{path}.levelOrExp"));
        return None;
    }
    if level.is_some_and(|x| x <= 0) || exp.is_some_and(|x| x < 0) {
        out.error(format!("{path}.levelOrExp"), "invalid_value", "level must be positive and exp nonnegative");
        return None;
    }
    let by_level =
        level.and_then(|n| if snap { master.support_level(group, n) } else { master.member_level(group, n) });
    let by_exp = exp
        .and_then(|n| if snap { master.support_level_by_exp(group, n) } else { master.member_level_by_exp(group, n) });
    if level.is_some() && by_level.is_none() || exp.is_some() && by_exp.is_none() {
        out.error(format!("{path}.levelOrExp"), "master_row_missing", "cultivation has no matching level row");
        return None;
    }
    if let (Some(a), Some(b)) = (by_level, by_exp) {
        if a.level != b.level {
            out.error(format!("{path}.levelOrExp"), "cultivation_conflict", "level and exp describe different levels");
        }
    }
    by_level.or(by_exp).map(|r| r.level)
}
