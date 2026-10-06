# Exact nominal LUCK refinement

The refinement provider evaluates the complete probability law for one specified performance order under the declared independent nominal LUCK draws. Each semantic draw uses the integer weights produced by the calculation model. Duplicate outcomes with the same result share their combined mass.

## Frame checkpoints

A pending branch contains a selected outcome prefix, its exact rational mass, a complete model checkpoint and the index of the next frame. Checkpoints are taken before a frame after a bounded number of completed frames. Sibling branches share an immutable checkpoint and clone it for execution.

A continuation extends the checkpoint's selected outcome prefix while preserving its consumed-choice cursor and draw counters. Frames before the checkpoint already have the state produced by that prefix. Replaying the suffix therefore gives the same state transitions as playback from the beginning. An interrupted frame is replayed from the latest complete checkpoint, including every action before the draw within that frame.

Each discovered draw partitions a pending branch into mutually exclusive outcomes whose masses sum to the incoming mass. Rational multiplication assigns each child its conditional mass. Terminal paths with the same score and final life are summed. The provider publishes a law once all pending branches terminate and the accumulated mass is exactly one.

## Equal initialized models

`LuckExactSession` borrows one immutable master, chart, frame schedule, timing
sequence and rank timeline. It compiles each performer order into a full initial
model, including formation predicates and cumulative counts. Orders with equal
initialized state share a complete terminal law in this fixed context.

The in-process identity contains the complete derived model state. The two score
lookup tables have sorted entries; other model maps use deterministic hashing.
Finite floating values retain their round-tripping representation, including
signed zero. Empty note/factor frame lists and arrays of bitwise-positive-zero
score diffs have a lossless representation containing their lengths. Nonempty
lists and any nonzero or negative-zero diff retain their complete contents.
The calculator and diff views destructure every field exhaustively, so adding
a field requires updating the identity. NaN-bearing and opaque states use
independent evaluation. Identity
construction is bounded to 512 KiB. The session retains at most 64 complete laws
and 32 MiB of identity and atom payload, evicting the oldest entries at capacity.

Every retained law has already passed the exhaustive traversal and mass-one
check. A hit therefore consumes zero additional replay segments or frames and
is available even when the execution allowance is exhausted. Cancellation is
checked before either replay or reuse. A session with zero entries evaluates
each order independently.

## Certified summary reuse

`LuckScoreSession` fixes the master and classified skills, chart, parameters,
frame and timing schedule, and rank arrivals. Before weighting or replay, it
can identify the complete initialized model with the same lossless identity.
Equal initialized models in this context have the same nominal terminal law.
Any completed all-path score, support and life certificate for one therefore
also encloses the other. Only completed summaries enter this session cache.

The cache retains at most 64 summaries and 8 MiB of identity and result payload,
further limited by the supplied curve-cache byte allowance. A zero allowance or
no curve cache clears and disables summary reuse. Cancellation is checked
before initialization and before returning a retained certificate. Every
performance order still contributes its own equally weighted, distinct label
to the 120-order aggregate. A summary hit skips recorder and bound replay;
actual probability propagation counters count only work that executes.

The factor replay also has exact identity transitions. A query of the current
score frame with no mandatory or possible lottery filing changes no replay
state. A frame with no ordinary float commands and no probe filings leaves
the factor and probe classes unchanged, records zero diffs, and immediately
undoes to those same factor values. Its notes still retain or join their
executed-state certificates according to whether every path executes that
frame. Later filings and rank rewinds use the ordinary replay rules.

## Reusing factor histories at another power

An admitted recorder can also compile the completed factor and combo histories into
an immutable score-bound program. The recorder's dependency check establishes that
ordinary effects, cumulative values, converted judgements, life, query times and
rank arrivals do not read initial total power or score. Power enters only the note
calculator. Nonzero power commands are rejected by the existing admission checks.
Solo rank is fixed; external ranks retain their declared arrival timeline.

The program identifies the complete initialized model with only initial total power
normalized. Chart notes and skill events are retained once in an exact shared scope:
the constructor copies these same values into the model. That scope also includes
every classified skill, play frame, judgement, seed and binary32 delta-time bit.
All other initialized fields remain in the model identity. Reuse additionally
requires the same retained certified probability-curve object. Oversized identities
or an unavailable admission certificate use the ordinary evaluation path.

Each interned note kernel keeps the note type, judgement, life-positive predicate
and every binary32 endpoint of the executed factor and combo enclosures. Note uses
separately keep their original probability-curve index and whether lottery commands
were already filed at that query. A new power reevaluates the original binary32
operations and integer floors; it does not scale a previous score. Note means are
added in their original filing order. Signed range differences, integer rank
percentages and shared fixed-bonus coefficients retain the same arithmetic.

Only rank-observed and terminal score queries need arithmetic on reuse. Their
unchanged-prefix cancellation thresholds are compiled from **all** intervening
queries, including unmeasured ones. A pending rank bonus is filed by its actual next
query; several confirmations before that query retain the native last-pending-value
rule. The final note expectation keeps its separate, fully probability-linked
observations instead of substituting an earlier query's mean.

This cache belongs to `LuckDpCache` and holds at most 128 programs and 32 MiB,
further limited by its configured byte allowance. It counts retained container
capacity, keys, kernels and references, plus each shared run scope and retained
curve allocation once. Zero capacity disables reuse. Only completed programs enter
the cache. Each evaluation checks cancellation and numeric admissibility again;
an overflow or interrupted evaluation supplies no completed bound. A cache hit is
still an all-path enclosure, and does not itself prove an exact payoff or ranking.

## Work and completion

`LuckExactBudget` bounds the number of replay attempts and executed frames across candidate orders. Each request starts with 240,000 replay segments and 8,000,000 executed frames. Each order admits at most 32,768 replay segments and 32 nontrivial outcome choices along a path. Pending paths are bounded to 32,768, and stochastic path depth bounds the retained ancestor checkpoints. The production search admits a chart when one playback fits the default frame allowance; its note count is independent of this admission decision.

Cancellation is checked during playback. Budget exhaustion, unsupported random draws and arithmetic capacity return a declined attempt while preserving the existing certified candidate enclosure. A completed law can refine score expectations, threshold probabilities, capped scores or joint score/life payoffs. The interval frontier establishes the resulting rank independently of whether every expectation has an exact rational representation.

## Synthetic checks

```sh
cargo test --release --locked -p ournotes-sim --lib luck_exact
cargo test --release --locked -p ournotes-sim --lib nominal_tests
cargo test --release --locked -p ournotes-sim --lib program_tests
cargo test --release --locked -p ournotes-search --lib refinement_tests
cargo test --release --locked --test adapter_fixture_export luck_refinement
```

The Cartesian tests compare all terminal score/life atoms and exact masses, including a long deterministic prefix, separated draws with intermediate checkpoints, external rank confirmations, partial-work interruption and draw-counter preservation.

Program tests compare fresh and reused enclosures bit for bit at several powers,
including binary32 integer-precision boundaries, and cover all 120 performer orders
at a second power. They also check complete weighted probability branches, rank
rewinds and simultaneous confirmations, run-scope and paired-Snap changes,
cancellation, numeric refusal, zero capacity and eviction.

## Boundary-candidate order storage

The ranking frontier retains certified score and payoff intervals for every relevant candidate. Small charts
retain detailed order state eagerly. For other charts, refinement reconstructs that state for one ambiguous
boundary candidate at a time from the immutable request and complete performer identities.

Each completed order law narrows its payoff and the aggregate expectation. Installed frontier bounds survive
releasing detailed order rows. A stopped probability tree preserves the previous certificate. Canonical ties
and the complete-domain ranking certificate determine the returned order independently of this storage policy.

The synthetic `long_stream_refinement_materializes_the_boundary_candidate` test exercises this path through
the public recommendation entry point. The storage-policy unit test distinguishes eager storage from backend
admission and checks that the ordinary long-chart domain remains eligible for bounded refinement.

## Refinement scheduling

Ambiguous candidates with smaller current certified payoff upper bounds receive refinement first, with
candidate identifiers breaking ties. Their upper bounds are closer to exclusion by a competitor's proved
lower bound. This priority uses the complete frontier and preserves every existing certificate.

Among the orders requiring refinement, the search first evaluates those with the widest payoff enclosure.
Every order has the same weight in the uniform-order objective. Canonical order indices break priority ties.
The interval frontier establishes each returned rank from complete certificates independently of this work order.

Additional synthetic checks cover several draws in one frame, conditional-state isolation, bounded stochastic
depth, initialized-model identity, complete-law reuse, and a complete threshold ranking over a 601-frame schedule
with request cache capacities of zero and 64.

## Initial proposals

A certified request evaluates at most `min(K, 3)` proposals during warm start when Snaps are available. A domain
with a single empty Snap binding admits up to 16 proposals. Capped-score and score/life threshold targets admit
up to 16 proposals. The subsequent traversal retains the complete requested domain and uses the
certified lower cutoff when enough candidates are available.
This proposal allowance controls search order, independently of the request's total work and time limits.
The bounded targets' finite shortlist uses the request's full deadline; other certified score targets begin
new seed proposals only in the first quarter of the remaining time.

The `certified_seed_budget_preserves_the_complete_canonical_ranking` test checks the canonical result against
exhaustive score search at several K and cache capacities, including the full candidate count.
