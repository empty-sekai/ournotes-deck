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
signed zero. NaN-bearing and opaque states use independent evaluation. Identity
construction is bounded to 512 KiB. The session retains at most 64 complete laws
and 32 MiB of identity and atom payload, evicting the oldest entries at capacity.

Every retained law has already passed the exhaustive traversal and mass-one
check. A hit therefore consumes zero additional replay segments or frames and
is available even when the execution allowance is exhausted. Cancellation is
checked before either replay or reuse. A session with zero entries evaluates
each order independently.

## Work and completion

`LuckExactBudget` bounds the number of replay attempts and executed frames across candidate orders. Each request starts with 240,000 replay segments and 8,000,000 executed frames. Each order admits at most 32,768 replay segments and 32 nontrivial outcome choices along a path. Pending paths are bounded to 32,768, and stochastic path depth bounds the retained ancestor checkpoints. The production search admits a chart when one playback fits the default frame allowance; its note count is independent of this admission decision.

Cancellation is checked during playback. Budget exhaustion, unsupported random draws and arithmetic capacity return a declined attempt while preserving the existing certified candidate enclosure. A completed law can refine score expectations, threshold probabilities, capped scores or joint score/life payoffs. The interval frontier establishes the resulting rank independently of whether every expectation has an exact rational representation.

## Synthetic checks

```sh
cargo test --release --locked -p ournotes-sim --lib luck_exact
cargo test --release --locked -p ournotes-sim --lib nominal_tests
cargo test --release --locked -p ournotes-search --lib refinement_tests
cargo test --release --locked --test adapter_fixture_export luck_refinement
```

The Cartesian tests compare all terminal score/life atoms and exact masses, including a long deterministic prefix, separated draws with intermediate checkpoints, external rank confirmations, partial-work interruption and draw-counter preservation.

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
