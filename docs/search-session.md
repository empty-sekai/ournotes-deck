# Deterministic SearchSession (contract v1)

`ResolvedOwnedSnapshot::start_search_session` adds cooperative, in-memory stepping to the dataset-bound
Power/Skip projection. It accepts only exhaustive Power, Skip score, Skip score-at-least and capped Skip score.
Live, event metrics, candidate strategies, network/finished inputs, foreign datasets and mismatched snapshot
revisions are rejected before traversal. Unknown skill fields remain unknown. This does not certify the
latest-native model, every account prerequisite, browser execution or mobile performance.

## Frozen goal and identity

Each response carries `GoalSpec.version = ournotes-deck.deterministic-physical-goal/1`, the requested Top-K,
`strategy = exhaustive`, and `resultIdentity = physicalDeck`. Decisions are the five physical member slots and
paired Snap IDs at fixed resolved cultivation. The domain enforces five distinct characters, slot 2 as leader,
unique nonempty Snap IDs, and include/exclude/leader constraints. Eligible and owned facts stay separate through
the private projection. These checks are not full account lifecycle legality.

The total ordering is exact deterministic payoff descending, power descending, physical member public IDs
lexicographically ascending, then paired Snap public IDs lexicographically ascending. **v1 places None before
every actual Snap ID**, preserving the physical evaluator's comparator. This is an explicit versioned choice; a
different tie preference, such as None last, would be a new contract version. Pool indexes, import order and work-slice sizes do not define ties.

The session enumerates all physical permutations and Snap injections. It does not fold equivalent positions or
use the existing `canonicalMemberSet` fast search. Several results may share one member set; physical Top-K does
not mean member-set Top-K. This conservative DFS closes the state contract before an independently proven
resumable fast path or large-pool quality measurement.

## One total deadline, separate active steps

The total monotonic deadline starts before preparation, includes validation/precomputation and every step,
and continues while yielded or cancelled. Resume keeps that deadline and cumulative candidate budget. Native
and WASM use the same search clock abstraction; the clock never enters numeric scoring.
The session clock starts at `start_search_session`; upstream dataset loading, original JSON parsing and
`resolve_data` precede that call. A Worker must also keep its earlier job deadline and pass the remaining budget
into session construction. This API alone does not budget those earlier phases.

`step(current_binding, StepBudget { max_work_units, time_slice_ms })` also bounds this call by cursor work and
an optional slice deadline starting at the call. Each choice, rejection, backtrack and complete candidate
evaluation consumes one work unit. The DFS frontier and Top-K/cache remain in memory across calls. A zero slice
performs no traversal. Preparation is synchronous and cannot yield during dataset/resolver validation.

Checks are cooperative: one atomic evaluation or response materialization can overrun a slice/total deadline.
An atom that finishes may be retained; the session then reports TimeLimit with no Complete certificate if the
remaining frontier was not proven empty. No new traversal starts after expiry.
Candidate limits count attempts cumulatively. A limit reached before another candidate is
CandidateLimit; if the cursor exhausts without needing another candidate, exact-cardinality exhaustion proves K.

## Cancellation, result validity and cleanup

`SessionBinding` carries `jobId`, `inputRevision`, `datasetId`, and `objectiveHash`. The facade checks dataset and
revision against the strict snapshot and the exact bound DeckData object. The entire DeckData remains immutably
borrowed; safe Rust cannot mutate charts/provenance while using the session. Numerical request inputs are private
clones. The loader/Worker must verify dataset identity and hash the **original UTF-8 request bytes**; core binding
strings are transport labels, not content-hash verification or provenance evidence.

Every step/read/cancel/resume supplies the current binding. Any changed field permanently marks the old session
Stale, clears old results/cache and invalidates its frontier, even after exhaustion. Supplying its former binding
cannot recover the results. The Worker must still discard obsolete messages when delivering responses.

`cancel` between steps marks Cancelled and preserves the frontier/complete candidates. Cancelled steps do no work.
`resume` only reactivates this unchanged unexpired input; terminal budget, Failed, Stale and Exhausted states do
not restart. Evaluation errors propagate and leave Failed, never Complete. Drop releases pool/frontier/cache/
results. This version does not serialize session state or resume after Worker termination/page reloads.

Only Exhausted carries `completion = Complete` and `optimality = proven`, conditional on the frozen model/domain/
tie. All other statuses carry `completion = null` and `optimality = unproven`. Ranked rows are complete evaluations.
Power rows have no fabricated score distribution. Owned coverage, assumptions and total-rank origin stay in scope.

```rust,ignore
let resolved = snapshot.resolve_data(&data, verified_dataset_id, GoalDependencies::Skip)
    .resolved.expect("handle missing/errors first");
let current = SessionBinding { job_id, input_revision, dataset_id, objective_hash };
let mut session = resolved.start_search_session(&data, &request, current.clone())?;
let progress = session.step(&current, StepBudget { max_work_units: 256, time_slice_ms: Some(8) })?;
// Yield to the Worker event loop and retain the same session.
let cancelled = session.cancel(&current)?;
let resumed = session.resume(&current)?;
```

## Validation scope

The tiny-domain oracle independently uses flat Cartesian enumeration, full-deck filtering and sorting, with
the shared point evaluator. It uses no production cursor/Top-K/matching/cache/pruning. Tests cover four supported
goals, physical permutations, unique Snap injections, constraints, public-ID ties, import reordering, work slices,
cancel/resume, stale bindings and deadline/candidate/empty-domain boundaries. Test-only monotonic clock injection
separates preparation, active slice, cancelled wait and atomic evaluation. These are behavioral/search checks,
not native model comparisons.
