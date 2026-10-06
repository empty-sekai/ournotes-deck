# Exact nominal LUCK refinement

The refinement provider evaluates the complete probability law for one specified performance order under the declared independent nominal LUCK draws. Each semantic draw uses the integer weights produced by the calculation model. Duplicate outcomes with the same result share their combined mass.

## Frame checkpoints

A pending branch contains a selected outcome prefix, its exact rational mass, a complete model checkpoint and the index of the next frame. Checkpoints are taken before a frame after a bounded number of completed frames. Sibling branches share an immutable checkpoint and clone it for execution.

A continuation extends the checkpoint's selected outcome prefix while preserving its consumed-choice cursor and draw counters. Frames before the checkpoint already have the state produced by that prefix. Replaying the suffix therefore gives the same state transitions as playback from the beginning. An interrupted frame is replayed from the latest complete checkpoint, including every action before the draw within that frame.

Each discovered draw partitions a pending branch into mutually exclusive outcomes whose masses sum to the incoming mass. Rational multiplication assigns each child its conditional mass. Terminal paths with the same score and final life are summed. The provider publishes a law once all pending branches terminate and the accumulated mass is exactly one.

## Work and completion

`LuckExactBudget` bounds the number of replay attempts and executed frames across candidate orders. Branch depth and per-order attempts are bounded independently. The production search admits a chart when one playback fits the default frame allowance; its note count is independent of this admission decision.

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
