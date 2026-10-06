# Played-live correctness matrix

The played-live matrix exercises the public scene and payoff contracts using synthetic input tables, card progress and judged notes. Each case enumerates every legal team in its declared domain and evaluates all 120 performance orders.

| Dimension | Values |
| --- | --- |
| Scene | Free, Mission, Challenge, Battle, Arena |
| Play | Theoretical-best play (explicit Perfect stream for terminal-life targets); a fixed stream containing Perfect, Great and Miss judgements |
| Payoff | Score, score threshold, capped score, score with terminal-life threshold, client event points, selected client event items |
| Additional ordinary-scene payoff | Client challenge points for Free, Mission, Battle and Arena |
| Ranking size | K = 1 and K = 5 |
| Ownership | Six member cards across five characters, one optional unique Snap |
| Leader | One fixed leader |
| Network context | Explicit rank arrivals and a three-player same-score policy for Battle and Arena |
| Constraints | Either alternate card of one character required |
| Stop | A deterministic one-candidate budget |

The scene/payoff test contains 68 combinations. Each combination has twelve legal teams. Exhaustive enumeration requests all teams; branch-and-bound results must equal the corresponding canonical prefix for each K. Comparison includes the returned payoff, power, member IDs, Snap IDs and per-order outcomes.

The constraint test contains ten combinations with six legal teams each. It compares the complete ranking with exhaustive enumeration and checks the result status and returned values after a candidate-budget stop. A reported complete-domain upper bound must cover the exhaustive optimum.

The payoff is computed for each performance order before averaging. In particular, point rewards, threshold indicators and capped scores retain their own payoff maps. Terminal-life thresholds refer to final life under the declared input stream.

## Reproduction

```sh
cargo test --release --locked -p ournotes-search --test adapter_fixture_export scenario_completion
cargo test --release --locked -p ournotes-search --features search-diagnostics --test adapter_fixture_export scenario_completion
```

The matrix runs as part of the ordinary adapter suite and its diagnostic build. Additional suites cover LUCK probability refinement, numerical boundaries, resource assignments, arbitrary leader selection and the independent bounded-domain search harness. Each suite establishes correctness for its declared input domain and model.
