# Roadmap

[中文](roadmap.md)

Only unfinished work is listed; finished items are removed.

## In progress

- An exact expectation over LUCK range lotteries, using their probabilities; LUCK charts are unsupported until then.
- Exact proofs within a time budget on every objective and scene: tighter interior bounds on Gekisou charts, cheaper exact leaf evaluation (performance orders sharing their common start), and initial decks from a fast heuristic.
- Tighter bounds on Gekisou charts (bonus bounds on combo charts).

## Gekisou rank parameter

Every Gekisou range is currently scored as first place. The rank will become a request parameter. The rank bonus is the range score times a percentage that differs a lot by rank in JP data (for one mission pattern, +250% for first and +100% for fourth and fifth). The rank changes how much in-range scoring weighs, so it changes the optimal deck.

What changes:

- Simulator: the range rank bonus and the confirmed rank fed to the "reach rank N or better" skill condition (7012) use the requested rank.
- Bounds: the per-range rank percentages use the same rank. The handling of condition 7012 in the bounds needs its own check.
- Results and caches: the rank is part of the request identity.
- Validation: finite-domain oracles for several ranks, comparing Top-K with exhaustive search.
- Consistency with the multiplayer rank input of Gekisou PT (`multiplayerRanks`).

Steps:

1. One rank for all Gekisou ranges, matching the chart page's rank option.
2. One rank per range.
3. A rank distribution, folded into the expectation together with the member order and lotteries.

The real rank depends on the other players in the room, so the parameter is the player's declared assumption and the result says so.

## Later

- A compact search data layout: dense small card indices in column-oriented tables, bit sets for used characters and Snaps, fixed-size packed tables of power and per-position gain for each member and Snap, and precomputed prefix sums so interior nodes look values up instead of recomputing them.
- nnnotes produces chart-only data in advance, bound to the model and data identity, with an error on mismatch.
- Joint chart and deck search; requirements below.

## Joint chart and deck search

The goal is to choose the chart (including its difficulty) and the member/Snap construction together; a fixed chart is an optional constraint. Four modes:

1. Fixed chart, fixed team: exact evaluation (available).
2. Fixed chart, variable construction: exact recommendation within the owned pool (available).
3. Variable chart, fixed team: exact chart ranking for that construction. The fixed-deck song ranking evaluates charts one by one, without a global certificate.
4. Variable chart and construction: exact global chart/deck Top-K.

Requirements:

- Across charts, keep one objective, declared player facts and cultivation, activity and LB consumption, play assumptions and the uniform member-order target. Each chart resolves its own scene, song attributes and tags, missions, timing, power factors and score-rank/PT rules; a judgement stream of one chart is never reused for another. Candidate availability is an explicit domain fact; missing unlock information is not proof of eligibility.
- Results identify both chart and team; the same team on different charts gives distinct results. The global canonical order is explicit and keeps the within-chart tie contract; PT is still computed per performance order before averaging.
- The outer search may prioritize charts with prepared data and admissible request-specific upper bounds, but never truncates charts by a static song ranking or the best score seen so far. The inner search shares the global cutoff only through a proven lower-bound contract; an unfinished inner incumbent is a feasible proposal, not a certificate.
- Global Complete requires every admitted chart domain to be exhausted or excluded by an admissible bound that also preserves secondary ranking and canonical ties. Timeout, unsupported input, numeric/capacity failure and an unfinished chart stay explicit, with the remaining chart domains and their certificate state.
- Validation: exhaustive joint chart/deck oracles on bounded fixtures, fixed-deck cross-chart comparison, charts with different missions/bonuses/rank thresholds, canonical ties, and native/WASM comparisons.
