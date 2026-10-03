# Search telemetry (`ournotes-deck.telemetry/1`)

[中文](telemetry.md)

Recommendation results (`ournotes-deck.recommendation-result/2`) and search session progress
(`ournotes-deck.search-session-progress/2`) both carry `telemetry` with the same structure: session progress
reports the work so far, a recommendation result reports the whole request.

## Conventions

- **Every key ending in `Ms` is milliseconds**: measured wall time, or a time limit from the request. With those keys
  removed, the rest depends only on the input and where the search stopped: a completed search gives the same content
  every time, and so does a search stopped by the same candidate limit.
- **Payoff numerators** are decimal strings over `environment.law.totalWeight`, like `results[].expectedPayoff`.
- **Depth**: the joint search places members leader first; depth `d` means `d` members placed (0 is the root, 5 a
  complete deck). Arrays of length 6 are indexed by depth.
- The "Use" column: **page** fields suit display to players, **dev** fields help locate performance and correctness
  problems.

## Top level

| Field | Meaning | Use |
|---|---|---|
| `format` | `ournotes-deck.telemetry/1` | — |
| `environment` | Program, data, request budget and bound setup | page (data identity), dev |
| `proof` | Proven or not, progress, the bound of the unexplored part and the gap | page |
| `incumbents` | The Top-K over time | page |
| `phases` | Phases in time order | page (simplified), dev |
| `time` | Exclusive time of the search loop by activity | dev |
| `nodes` | Search tree nodes of every traversal | page, dev |
| `leaves` | Candidate deck evaluation, simulations and cutoffs | dev |
| `joint` | Joint search nodes and bound checks by depth | dev |
| `composition` | Member-composition traversal counters | dev |
| `candidate` | Heuristic candidate strategy counters | dev |
| `luckReplay` | LUCK replay queries, hits and unavailability reasons | dev |
| `caches` | Lookups and hits of each cache | dev |

## `environment`

| Field | Meaning |
|---|---|
| `crateVersion` | Library version |
| `commit` | The `OURNOTES_DECK_COMMIT` environment variable at build time; null when not declared |
| `features` | Enabled features (such as `search-diagnostics`) |
| `arch`, `os` | Target architecture and system (`wasm32` for WASM) |
| `optimized` | Built without debug assertions |
| `data` | Data identity: `region`, `masterVersion`, `clientVersion`, `resourceVersion` from the data file's provenance (null when absent), and `sha256`, the SHA-256 of the data file's JSON text |
| `route` | Solver route: `canonicalPowerSkip`, `physicalExhaustive`, `physicalBranchAndBound`, `physicalCandidate` |
| `traversal` | Traversal run: `none`, `canonical`, `fixed`, `joint`, `composition`, `exhaustive`, `candidate`, `session` |
| `k` | K of the Top-K |
| `timeLimitMs`, `maxCandidates`, `cacheEntries` | Time left when the search started (problem construction deducted), candidate limit, deduplication cache capacity |
| `law` | Random roots: `atoms`, `orders` (distinct performance orders), `totalWeight` (the payoff denominator) |
| `domain` | Legal domain: `members`, `snaps`, `required` (required members), `leaderFixed` |
| `bounds.compiled`, `bounds.fallback`, `bounds.compileMs` | Joint bounds compiled, why branch-and-bound fell back to full enumeration, compile time |
| `bounds.choices` | Branching of every joint depth (member and Snap choice pairs) |
| `bounds.correlated`, `bounds.resource` | Correlated and resource bounds enabled (in the last search part) |
| `bounds.fine`, `bounds.rush`, `bounds.luckOracle`, `bounds.classSearch` | Fine bounds present, LUCK Rush refinement applies, LUCK replay available, class search used |
| `bounds.ptRegime` | PT tightening after the warm start: `membersRemoved`, `fallback`, `compileMs`; null when not run |
| `bounds.conversion` | Gekisou score split by converting Snaps: `snaps` (converting Snaps), `parts`, `fallback`, `compileMs`; null when not split |

## `proof`

| Field | Meaning |
|---|---|
| `complete` | The search was exhaustive (the results are the proven Top-K) |
| `fraction` | Position-based share decided, 0 to 1; 1 when complete. Every branch before the current path has been evaluated, pruned or skipped. Branches differ in size, so this indicates progress, not remaining time. Null for traversals without a tracked position |
| `parts`, `partsDone` | Search parts run one after another, and those finished (Gekisou score conversion parts; 1 otherwise) |
| `topLevelDone`, `topLevelTotal` | Top-level branches decided in the current part, of the total (depth-0 joint choices, or leaders of the composition traversal); given when not complete |
| `best`, `kth` | Payoff numerators of the best and the K-th deck; `best` is null with an empty Top-K, `kth` while it is not full |
| `upperBound` | Payoff bound (numerator) of **the part not yet explored** at the stop |
| `bestGap`, `kthGap` | `(upperBound − x) / x`; 0 when the bound does not exceed x; null when x is not positive |
| `boundMs` | Time spent computing `upperBound` after the stop (outside the search deadline) |

`upperBound` is a true upper bound: the best deck of the whole domain pays at most `max(best, upperBound)`, and every
deck of the true Top-K that was not kept pays at most `max(upperBound, kth)`. A stopped search can therefore show
"not proven; the optimum exceeds the current best by at most `bestGap`".

How it is computed: after the stop, for the remaining branches at every level of the current path, take the bound the
search itself would check there (node bounds and tail bounds; at the root, the depth-1 node bound of each remaining
choice, which is the first remaining one when the root follows that bound's descending order; for conversion parts not
yet started, the pool-wide bound), and take the maximum. When the stop happens in the
PT warm start, the prefixes the warm start skipped by bonus are unexplored too, and the whole-domain root bound is used.

Nulls: a complete search needs no bound; the `exhaustive`, `candidate`, `canonical` and `session` traversals have no
bound (and `bestGap` is null); when a bounded traversal stops with nothing left unexplored, `upperBound` is null and
the gaps are 0.

## `incumbents`

| Field | Meaning |
|---|---|
| `updates` | Top-K insertions |
| `stride` | The timeline records every `stride`-th insertion; at 256 entries every other one is dropped and the stride doubles. The last insertion is always recorded |
| `timeline[]` | `update` ordinal, `atMs` (from the request start), the `nodes`, `candidates` and `simulations` at that point, `filled` (decks in the Top-K), `best`, `kth`, `fraction` |
| `firstFull` | The entry at which the Top-K first filled (fields as in `timeline[]`; the thinned timeline may lack it); null while never full |
| `warmStart.evaluations`, `warmStart.leafBoundChecks` | Decks the warm start before the joint traversal (phase `seed`) evaluated exactly, and the leaf bounds its local search and polishing computed |
| `warmStart.kth` | The K-th payoff numerator after the warm start; null when the Top-K was not full |
| `warmStart.finalTopK` | Decks of the final Top-K that the warm start or polishing evaluated first |
| `warmStart.polishRounds`, `warmStart.polishEvaluations`, `warmStart.polishMs` | Polishing around strictly better best decks found by the traversal: rounds, exact evaluations, time (inside the search phases) |

The warm start and polishing only put exactly evaluated legal decks into the Top-K and prune nothing; the traversal
treats them as evaluated when it reaches them.

A page can plot `best`/`kth` against `atMs` as a convergence curve.

## `phases`

In time order, without overlap; small gaps between phases belong to none. Each entry: `name`, `label`, `startMs`
(from the request start), `wallMs`, and the `nodes`, `candidates` and `simulations` added during the phase.

| `name` | Meaning |
|---|---|
| `prepare` | Resolve the scenario, growth and goal, build the legal domain (bound compilation excluded); absent when running an already built problem |
| `boundCompile` | Compile the joint bounds |
| `setup` | LUCK replay preparation, deciding correlated and resource bounds |
| `ptWarmStart` | PT: find a Top-K in the maximum-bonus regime first |
| `ptRegimeCompile` | PT: drop members that cannot reach the current K-th and recompile the bounds |
| `conversionCompile` | Gekisou score: compile the bounds of each conversion part |
| `seed` | Warm start before the joint traversal (see `incumbents.warmStart`) |
| `search` | Main search; with conversion parts one entry per part, `label` being `free`, `snap <Snap ID> slot <slot>` or `pair slots <i>,<j>` |
| `evaluate` | Evaluate the requested deck |
| `warmStart`, `proposals` | The two stages of the heuristic candidate strategy |
| `verify` | Result check of the canonical Power/Skip search |
| `finish` | Assemble the results |

## `time`

Exclusive time (milliseconds) by activity from the start of solving to its end; the entries sum to that span:

| Field | Meaning |
|---|---|
| `depthMs[d]` | Joint node work at depth `d`: bounds, choice loops and bookkeeping, excluding the entries below |
| `compositionMs` | Member-composition traversal node work |
| `fineBoundMs` | Fine bounds (including their LUCK replay) |
| `cutoffTableMs` | Building simulation cutoff tables (including their LUCK replay) |
| `simulationMs` | Whole-live simulations run to the end |
| `stoppedSimulationMs` | Whole-live simulations stopped by the cutoff |
| `rushPrefixMs` | Rush prefix bounds (including their LUCK replay) |
| `warmStartMs` | Warm start and polishing, excluding the simulations and cutoff tables they run |
| `otherMs` | The rest: preparation, compilation, result assembly, other traversals, and computing `proof.upperBound` after a stop (`proof.boundMs`) |

`luckReplay.replayMs` is included in `fineBoundMs`, `cutoffTableMs` and `rushPrefixMs`.

## `leaves`

| Field | Meaning |
|---|---|
| `proposed` | Candidate decks handed to evaluation (deduplication cache hits included, see `caches.candidates`) |
| `visited` | New candidates (what `maxCandidates` counts) |
| `evaluated` | Candidates with every atom computed |
| `partial` | Candidates whose evaluation a stop interrupted |
| `duplicateAtoms` | Atoms reusing the result of the same random root |
| `atomBoundPruned` | Candidates dropped after some atoms: their exact atoms plus the metric's cap stay below the K-th |
| `simulations` | Whole-live simulations run to the end |
| `cutoff.tables` | Candidates simulated with a cutoff table |
| `cutoff.unavailable` | Candidates without a finite cutoff table for any order |
| `cutoff.stopped` | Simulations stopped early (the candidate cannot reach the Top-K) |
| `cutoff.stoppedAt[i]` | Stops where the played frames were in `[i/10, (i+1)/10)` of the chart |
| `cutoff.relaxedPowerAbove` | Cutoff tables (one per performance order) whose exact power is below the relaxed power the leaf fine bound read; a relaxed power below the exact one is an error |
| `peakRetained` | Most decks the Top-K held |

## `joint`

Per-depth counters of the joint (Gekisou) search, arrays indexed by depth. Node bounds count at the depth of the node
checked; `tail` and `pair` count at the parent's depth (they check its children). Each bound is
`{checks: [6], pruned: [6]}`.

| Field | Meaning |
|---|---|
| `nodes` | Nodes at each depth |
| `branch` | Basic node bound |
| `assignment` | Power assignment cap, on payoff ties with the K-th |
| `correlated`, `resource` | Correlated and resource bounds |
| `bonus`, `bonusUnavailable` | PT bonus bound and the times it was unavailable |
| `raw`, `fine` | Raw and fine bounds of complete decks |
| `tail`, `tailChoicesSkipped` | Tail bound excluding a run of remaining branches at once, and the branches it skipped |
| `pair` | Bound of a single child branch |
| `nodeTies`, `pairTies` | Bounds equal to the K-th payoff, kept on power |
| `seedBonusSkipped` | PT warm start: prefixes outside the maximum-bonus regime |
| `rushPrefix` | Rush prefix bound at depth 4: `checks`, `pruned`, `unavailable`, `variants`, `choicesPruned` |
| `carriers` | Gekisou score on a chart with a combo range: cheap bounds by the number of combo carriers (a member and Snap bringing Gekisou combo bonus windows; a node with `c` carriers placed and `r` slots to fill reads level `c + r`): `levels` compiled apart from the pool-wide bounds (the largest over the search parts), `nodes[n]` the bounded nodes reading level `n` (0 to 5) |
| `rootOrder` | Root children visited in descending order of their depth-1 bound: `skipped` counts root children left once one was strictly inferior to the K-th (each would be pruned at depth 1), `traversalsPruned` the traversals (the whole domain, or one Gekisou conversion part with its slot rules) whose best root child already was |

## `composition`

Member composition → layout → Snap traversal of regular Lives: `memberNodes` (by members placed), `snapNodes`,
`classNodes`, `bindingNodes`, `compositions` (member sets reached); the bounds `composition`, `layout`, `fine`,
`class`, `classBinding`, each `{checks, pruned}`; `classInfeasible`, `classResourceChecks`, `classResourceTightened`;
seed candidates `seeds.{preseed, layout, weighted, class, powerFrontier}`; `powerFrontierClosed`.

## `candidate`

Heuristic candidate strategy: `warmupMemberSets`, `warmupProposals`, `explorationProposals`.

## `luckReplay`

| Field | Meaning |
|---|---|
| `enabled` | LUCK replay was enabled for this search |
| `queries`, `cacheHits`, `runs`, `replayMs` | Queries, cache hits, roots actually replayed, replay time |
| `unavailable.total` | Queries answered without masks, by reason: `noVariant` (a member/Snap pair of the deck has no LUCK signature), `noRoots` (no random root at these positions), `declined` (a root replay exceeded the branch budget or is unsupported), `rootShape` (root results differ in shape and cannot be united), `cached` (cache hits of an earlier unavailable answer) |
| `sites.{fine, cutoff, prefix, branch}` | `queries` and `unavailable` by caller (fine bounds, cutoff tables, Rush prefix, per-branch fine bounds) |
| `branchBound` | Leaves past the fine bound whose performance-order buckets have several replay branches: `checks` of the fine bound taken at its maximum over each bucket's branches instead of their union, and the leaves it `pruned` |
| `diagnostics` | `search-diagnostics` builds only: queries by caller and outcome, leaf mask coverage, declined replays with their reasons, a sample of the leaves that survived the fine bound |

## `caches`

`candidates` (decks already evaluated or pruned), `luckReplay`, `luckBranches` (LUCK replay branch lists), `bonusRows`
(row tables of the PT bonus bound) and `rushWindows` (Rush entry windows), each `{lookups, hits, evictions, peakEntries}`; `evictions` counts entries
dropped (all of them when a cache is cleared). `bonusRowsRefused` counts lookups that gave up the bound because the
table was full.

## Suggested page fields

- Status: `proof.complete`; when not complete and `proof.bestGap` is not null, show "not proven; the optimum exceeds
  the current best by at most x%".
- Progress: `proof.fraction` (labelled as position-based), or `proof.topLevelDone / proof.topLevelTotal`.
- Convergence: `best` and `kth` against `atMs` from `incumbents.timeline`.
- Time breakdown: `name`, `startMs`, `wallMs` of `phases`.
- Data identity: `environment.data` (region, master version, data SHA-256).
- Scale: `nodes`, `leaves.visited`, `leaves.simulations`.

The other fields serve development.
