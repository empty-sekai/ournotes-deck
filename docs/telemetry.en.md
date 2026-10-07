# Search telemetry (`ournotes-deck.telemetry/1`)

[中文](telemetry.md)

Recommendation results (`ournotes-deck.recommendation-result/3`) and search session progress
(`ournotes-deck.search-session-progress/2`) both carry `telemetry` with the same structure: session progress
reports the work so far, a recommendation result reports the whole request.

## Conventions

- **Every key ending in `Ms` is milliseconds**: measured wall time, or a time limit from the request. With those keys
  removed, the rest depends only on the input and where the search stopped: a completed search gives the same content
  every time, and so does a search stopped by the same candidate limit.
- **Payoff numerators** are decimal strings over `environment.target.denominator` (120 for played lives: the sum
  over the performance orders), like `results[].expectedPayoff`.
- **Depth**: the joint search places members leader first; depth `d` means `d` members placed (0 is the root, 5 a
  complete team). Arrays of length 6 are indexed by depth.
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
| `leaves` | Team evaluation over the performance orders, simulations and cutoffs | dev |
| `joint` | Joint search nodes and bound checks by depth | dev |
| `composition` | Member-composition traversal counters | dev |
| `candidate` | Heuristic candidate strategy counters | dev |
| `caches` | Lookups and hits of each cache | dev |
| `memory` | Peak memory of the program | page, dev |
| `lotteryRefinement` | Work spent refining complete nominal LUCK paths | dev |

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
| `target` | Played lives: `orders` (120 performance orders, each equally likely) and `denominator` (the payoff denominator); null for power and Skip |
| `domain` | Legal domain: `members`, `snaps`, `required` (required members), `leaderFixed` |
| `bounds.compiled`, `bounds.fallback`, `bounds.compileMs` | Joint bounds compiled, why branch-and-bound fell back to full enumeration, compile time |
| `bounds.choices` | Branching of every joint depth (member and Snap choice pairs) |
| `bounds.correlated`, `bounds.resource` | Correlated and resource bounds enabled (in the last search part) |
| `bounds.fine`, `bounds.classSearch` | Fine bounds present, class search used |
| `bounds.familyTemplate` | Optional Score-family reward template admission, first refusal and numeric factor-envelope inputs; null when the plan does not attempt the template |
| `bounds.ptRegime` | PT tightening after the warm start: `membersRemoved`, `fallback`, `compileMs`; null when not run |
| `bounds.conversion` | Gekisou score split by converting Snaps: `snaps` (converting Snaps), `parts`, `fallback`, `compileMs`; null when not split |

## `proof`

| Field | Meaning |
|---|---|
| `complete` | The search was exhaustive (the results are the proven Top-K) |
| `fraction` | Position-based share decided, 0 to 1; 1 when complete. Every branch before the current path has been evaluated, pruned or skipped. Branches differ in size, so this indicates progress, not remaining time. Null for traversals without a tracked position |
| `parts`, `partsDone` | Search parts run one after another, and those finished (Gekisou score conversion parts; 1 otherwise) |
| `topLevelDone`, `topLevelTotal` | Top-level branches decided in the current part, of the total (depth-0 joint choices, or leaders of the composition traversal); given when not complete |
| `best`, `kth` | Exact payoff numerators of the best and the K-th deck; `best` is null with an empty Top-K, `kth` while it is not full. Both are null on the LUCK interval path; see each result's `payoffInterval` |
| `upperBound` | Payoff bound (numerator) of **the part not yet explored** at the stop |
| `globalUpperBound` | Bound of the best payoff numerator over the whole domain, covering retained decks and every open branch; nonincreasing once known. A completed deterministic physical search sets it to `best`, including exhaustive and multipart conversion searches. Null for a completed empty domain, the LUCK interval path, and traversals that do not record this field |
| `bestGap`, `kthGap` | `(upperBound − x) / x`; 0 when the bound does not exceed x; null when x is not positive |
| `boundMs` | Time spent computing `upperBound` after the stop (outside the search deadline) |

For deterministic searches, `upperBound` is a true upper bound: the best deck of the whole domain pays at most `max(best, upperBound)`, and every
deck of the true Top-K that was not kept pays at most `max(upperBound, kth)`. A stopped search can therefore show
"not proven; the optimum exceeds the current best by at most `bestGap`".

How it is computed: after the stop, for the remaining branches at every level of the current path, take the bound the
search itself would check there (node bounds and tail bounds; at the root, the depth-1 node bound of each remaining
choice, which is the first remaining one when the root follows that bound's descending order; for conversion parts not
yet started, the pool-wide bound), and take the maximum. When the stop happens in the
PT warm start, the prefixes the warm start skipped by bonus are unexplored too, and the whole-domain root bound is used.

One running conversion part cannot supply a whole-domain bound on its own, so `globalUpperBound` may be null then.
At a stop, the retained best and the `upperBound` covering all remaining parts supply a whole-domain bound;
on completion every part has closed. Evaluated LUCK candidates live on the interval frontier and must not be treated
as an empty exact Top-K or a zero payoff.

`upperBound` nulls: a complete search has no unexplored part; the `exhaustive`, `candidate`, `canonical` and `session` traversals do not
record an unexplored bound (and `bestGap` is null); when a bounded traversal stops with nothing left unexplored, `upperBound` is null and
the gaps are 0. A progress report is taken while the search runs, not at a stop: `complete` is false and `upperBound`
and the gaps are null.

## `incumbents`

| Field | Meaning |
|---|---|
| `updates` | Top-K insertions |
| `stride` | The timeline records every `stride`-th insertion; at 256 entries every other one is dropped and the stride doubles. The last insertion is always recorded |
| `timeline[]` | `update` ordinal, `atMs` (from the request start), the `nodes`, `candidates` and `simulations` at that point, `filled` (decks in the Top-K), `best`, `kth`, `fraction`, `upper` (`proof.globalUpperBound` at that point; null before it is known) |
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
| `initial` | Evaluate the request's `initialDecks` |
| `setup` | Decide the correlated and resource bounds, prepare the warm start |
| `ptWarmStart` | PT: find a Top-K in the maximum-bonus regime first |
| `ptRegimeCompile` | PT: drop members that cannot reach the current K-th and recompile the bounds |
| `conversionCompile` | Gekisou score: compile the bounds of each conversion part |
| `seed` | Warm start before the joint traversal (see `incumbents.warmStart`) |
| `search` | Main search; with conversion parts one entry per part, `label` being `free`, `snap <Snap ID> slot <slot>` or `pair slots <i>,<j>` |
| `lotteryRefinement` | Complete-law refinement after the physical LUCK domain closes |
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
| `fineBoundMs` | Per-order raw and fine caps of complete teams |
| `cutoffTableMs` | Building simulation cutoff tables |
| `simulationMs` | Live simulation and certified probability/score playback |
| `stoppedSimulationMs` | Whole-live simulations stopped by the cutoff |
| `warmStartMs` | Warm start and polishing, excluding the simulations and cutoff tables they run |
| `intervalFrontierMs` | Certified candidate insertion, interval ranking and refinement bookkeeping, excluding probability playback |
| `otherMs` | The rest: preparation, compilation, result assembly, other traversals, and computing `proof.upperBound` after a stop (`proof.boundMs`) |

## `leaves`

| Field | Meaning |
|---|---|
| `proposed` | Candidate decks handed to evaluation (deduplication cache hits included, see `caches.candidates`) |
| `visited` | New candidates (what `maxCandidates` counts); teams in their canonical layout for played lives |
| `evaluated` | Candidates evaluated completely (every performance order) |
| `partial` | Candidates whose evaluation a stop interrupted |
| `cheapPruned`, `finePruned` | Teams dropped before any simulation: the sum of their per-order cheap caps, or of their raw and fine caps, stays below the K-th |
| `started` | Teams whose performance orders started to run |
| `orderBoundPruned` | Teams dropped by remaining-order caps: completed exact payoffs, or certified expected-score caps for LUCK, plus the caps of the other orders stay below the K-th; includes upper-only preparation and reuse of that complete-program upper bound |
| `simulations` | Completed performance-order evaluations or certified order enclosures, including completed orders of a subsequently excluded LUCK candidate |
| `cutoff.tables` | Performance orders simulated with a cutoff table |
| `cutoff.unavailable` | Performance orders without a finite cutoff table |
| `cutoff.stopped` | Simulations stopped early (the team cannot reach the Top-K) |
| `cutoff.stoppedAt[i]` | Stops where the played frames were in `[i/10, (i+1)/10)` of the chart |
| `peakRetained` | Most decks the Top-K held |

### `leaves.lotteryUpper`

Terminal joint Rush/probe preparation for a Score candidate with a certified cutoff. A completed native
recorder/DP capability can tighten an expected-score upper bound; it does not complete a score simulation,
a candidate value or a probability law. The first optional cap proves terminal native-note kernels and every
historical rank query, including prior readiness, fixed-bonus cancellation and nonwrapping integer support.
An unavailable whole-score upper retains the fine-bound decomposition: matching positive direct probes may
use all four joint buckets under the common LUCK gate, while all history drift, historical ranks and conversion
allowances remain in its unchanged remainder. Native per-note caps can also tighten this fallback.

| Field | Meaning |
|---|---|
| `attemptedOrders` | Preparations started |
| `preparedOrders` | Completed recorder/DP/last-query capabilities |
| `declinedOrders`, `declines` | Refusals, classified as `noLuckRange`, `externalRanking`, `recorderAdmission`, `scoreArithmetic`, `probabilityDomain`, `unfinishedRanges` or `terminalQuery` |
| `stoppedOrders` | Preparations interrupted by cancellation or the cooperative deadline |
| `boundedOrders` | Prepared laws with an accepted native whole-score upper or matching fine-cap decomposition |
| `incompatibleCaps` | Neither the whole-score upper nor the fallback decomposition was proved; the previous cap remains |
| `tightenedOrders` | Accepted caps that lower the retained integer order cap |
| `prunedTeams` | Teams excluded during the upper-only prepass |
| `elapsedMs` | Preparation, native upper construction and fallback fine-cap weighting time, included in `time.simulationMs` rather than an extra exclusive activity |

None of these preparations increments `leaves.simulations` or the diagnostic full-score `evaluations`.
Every unfinished order retains its previous cap. `Complete` still requires the full-domain canonical ranking
certificate.

### Diagnostic recorder work

The `search-diagnostics` build also exposes these fields in `LuckScoreProfile` / `luckProfile`.
They describe the private deterministic score recorder, independently of later factor replay or DP work.

| Field | Meaning |
|---|---|
| `recorderTraceOnlyRuns` | Private structural recorders that actually entered calculate at least once; enabling an unused recorder does not count |
| `recorderTraceOnlyQueries` | Actual calculate entries in structural mode, including entries that later return an error |
| `recorderTraceOnlyActiveQueries` | Those entries whose original native undo or execution frame interval was nonempty; other entries were already quiet |
| `recorderRunMs` | Time in the existing deterministic recorder phase, including partial work before cancellation or refusal |

The three counts are updated at the actual entry and are retained if the attempt later stops or fails.
They do not count skipped note evaluations or executed probability branches; an active frame interval may
contain no notes. These fields do not increase full-score `evaluations` or `leaves.simulations`.

For a full bounds evaluation, `recorderRunMs` covers the original frame-recording interval, after weighted
recorder setup and before post-recording checks and bound replay. Upper preparation keeps its existing
recorder interval, including weighted setup and terminal checks. Both intervals retain their elapsed work
on early return. A later recorded-program hit does not add that recorder time a second time.
`recorderRunMs` is included in the enclosing simulation/preparation time, not an extra exclusive activity;
it must not be added to that parent total. It contains controller, life, Combo and structural-validation work
even when native numeric execution is omitted. DP and later factor replay retain their separate phases.

### Diagnostic native cap profile

Native `profile_case` diagnostic builds expose the calling thread's `LuckScoreProfile` as `luckProfile`.
The fields below describe optional factor-prefix, native-note and whole-score upper certificates; they are
separate from the public `leaves.lotteryUpper` counters. One optional certificate can decline while an earlier
capability remains available. These arithmetic certificates do not count as full-score evaluations.

| Field | Meaning |
|---|---|
| `terminalFactorBuilds` | Completed optional factor-prefix certificates |
| `terminalFactorRefusals` | Optional factor certificates refused by their admission, allocation or arithmetic checks; cancellation is excluded |
| `terminalFactorMs` | Time spent preparing these optional factor certificates, including refused and stopped attempts |
| `terminalFactorAdditions` | Sum of the certified upper counts of nonzero additions to each floating field, across successful preparations; these are bounds on possible native work, not performed replay operations |
| `terminalFactorUndos` | Sum of the certified upper counts of relevant frame-diff subtractions across successful preparations |
| `terminalFactorProbeRuns` | Sum of nondecreasing possible-probe filing runs across successful preparations |
| `terminalFactorMaximumState` | Largest exact-real intermediate floating-field magnitude bound used by a successful certificate |
| `terminalFactorMaximumDrift` | Largest floating-field drift allowance of a successful certificate; this is a factor allowance, not a score interval width |

The counts and times add over preparations; the last two fields retain maxima. `terminalFactorMs` is a phase
within the existing preparation/simulation time and must not be added to that parent total. Probability masses
do not determine these operation counts. A lower count describes a tighter all-path work bound, not a smaller
candidate domain or fewer lottery branches.

| Field | Meaning |
|---|---|
| `terminalKernelBuilds` | Completed native terminal-note cap vectors |
| `terminalKernelRefusals` | Optional native-note kernel construction refused; cancellation is excluded |
| `terminalKernelMs` | Native-note kernel construction and optional whole-score upper work, including refused and stopped attempts |
| `terminalKernelNotes`, `terminalKernelComboObservations` | Terminal note count and recorded Combo observations across successful native-note cap constructions |
| `nativeScoreBuilds` | Completed optional whole-score expectation uppers; these are not candidate evaluations |
| `nativeScorePlanRefusals` | Missing, reversed or nonadjacent rank snapshots, intervening filings, unequal fixed coefficients, pending ranks or other structural/capacity failures |
| `nativeScoreReadyRefusals` | A Query has not yet captured probability readiness for an included terminal or historical note |
| `nativeScoreKernelRefusals` | A historical prefix, note mapping or native kernel could not be certified |
| `nativeScoreSupportRefusals` | Nonnegative, finite arithmetic or nonwrapping note/range/rank/final support could not be established |
| `nativeScoreMs` | Whole-score construction subphase, included in `terminalKernelMs`; never add it to that parent total |
| `nativeScoreRankWindows`, `nativeScoreRankNotes` | Rank windows and selected note occurrences handed to historical native-kernel evaluation, including work in later-refused or stopped attempts |

The kernel and whole-score counts add over preparations. Cancellation is excluded from all refusal counters;
its elapsed work still contributes to phase times. Query-readiness and support checks establish whether an
upper is usable, independently of the search's time and candidate limits. No profile field changes `Complete`.

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
| `tail`, `tailChoicesSkipped` | Tail bound excluding a run of remaining branches at once, and the branches it skipped |
| `pair` | Bound of a single child branch |
| `nodeTies`, `pairTies` | Bounds equal to the K-th payoff, kept on power |
| `seedBonusSkipped` | PT warm start: prefixes outside the maximum-bonus regime |
| `modules` | Bound modules by name (`memberAdditive`, ...; `carrierSplit`: the node bound split by the combo carriers of the slots to fill, see `carriers`; `carrierSplitTail`: the same bound over the children left in a node's choice loop), each `{checks, pruned}` over all depths |
| `carriers` | Gekisou score on a chart with a combo range: cheap bounds by the number of combo carriers (a member and Snap bringing Gekisou combo bonus windows; a node with `c` carriers placed and `r` slots to fill reads level `c + r`): `levels` compiled apart from the pool-wide bounds (the largest over the search parts), `nodes[n]` the bounded nodes reading level `n` (0 to 5) |
| `luckFamily` | Controller-family preparation, refusal and cache counters, detailed below |
| `rootOrder` | Root children visited in descending order of their depth-1 bound: `skipped` counts root children left once one was strictly inferior to the K-th (each would be pruned at depth 1), `traversalsPruned` the traversals (the whole domain, or one Gekisou conversion part with its slot rules) whose best root child already was |

### Family template admission

`environment.bounds.familyTemplate` records preparation of the optional Score-family reward
template separately from the node requests in `joint.luckFamily`. This distinguishes an
unavailable template from a template that was available but never reached by the traversal.
`admitted` concerns only this optional bound; it is never a ranking or probability-law certificate.
`refusal` is the first failed template prerequisite, such as `additiveEnvelope`, `terminalCaps`,
`rushClass`, `networkRanking`, a coefficient or window domain check, or a capacity limit.

`factorEnvelope` exposes the complete-domain command and execution counts, factor and frame
norms, their independent position maxima, the resulting `drift`, and `deltaWithChain` after
the snapshot and arithmetic-chain allowances. `roundings` counts the certified floating
operations; `feedbackAlpha` is its outward product with binary32 unit roundoff. These are
bounds on possible work, not observed simulation operations. Independent maxima may come
from different physical choices. `positionLimits`, `characterLimits` and
`snapLimits` each use the component order `[commands, executions, factorNorm, frameNorm]`;
the resource arrays are null when the optional distinct-resource relaxation is unavailable.
The final scalar limits include every available intersection. `positiveFactorAdmitted` and
`feedbackAdmitted` retain the two numeric gate decisions even when both fail. The additive envelope's `refusal` identifies
its first failed prerequisite; `admitted` reports whether it exists. The raw inputs may expose
further failures after that first one. `judgementMax`, `sensitivity`, `a0`, `global` and
`chainExtra` retain the other values used by the certificate. The diagnostic fields observe
the compiled decisions and do not authorize pruning themselves.

### `joint.luckFamily`

Preparation and cache counters for the optional certified-LUCK Score bound at joint depth four. Its native
family covers fixed members with the complete allowed Snap domain before a node applies its actual prefix
bindings and remaining choices. These counters are independent of leaf evaluations and candidate ranking.

| Field | Meaning |
|---|---|
| `contextChecks`, `contextRefusals`, `contextStopped`, `contextMs` | Context-admission attempts, refusal counts by reason, stopped attempts and elapsed preparation time |
| `checks`, `boundedNodes` | Node requests that reached the admitted family path, and requests that returned a complete numeric upper (including zero for an empty suffix) |
| `familyLookups`, `familyHits`, `refusedHits` | Member-family lookups, complete coefficient-table hits and separately cached refusal hits |
| `preparedFamilies` | Successfully prepared complete numeric coefficient tables; not evaluated candidates |
| `preparationRefusals`, `preparationDeclines` | Unavailable native families or pair construction, with reason counts; excludes cancellation |
| `preparationMs` | Native family preparation time, including refused and stopped attempts |
| `envelopeMs`, `envelopeRefusals` | Coefficient-table construction time and unavailable numerical envelopes; time includes stopped attempts, refusal counts exclude cancellation |
| `capacityDeclines` | Search-side allocation or reward-template-scope failures; native family capacity failures are separately included in `preparationDeclines.capacity` |
| `stopped` | Interrupted node-bound requests; no partial upper is returned |
| `orderLaws`, `profiles` | Complete native profile/order certificates materialized before numerical-envelope construction; they can increase when that later construction declines, and do not count actual DP propagations or scored orders |
| `evictions`, `peakEntries`, `peakBytes` | Cache evictions and recorded high-water entries/bytes for cache containers, the retained reward template and complete coefficient tables; excludes transient family laws, other caches and process RSS |

Both refusal objects use the same reason keys: `context`, `terminalMapping`, `pairDomain`, `recorderAdmission`,
`lifeFeedback`, `judgementFeedback`, `writerProfiles`, `probabilityDomain`, `budget`, `capacity` and
`incompleteCoverage`. A zero-cache or inapplicable objective skips this optional path and can leave its counters
at zero. A refused-cache hit is not a successful family and does not increment `familyHits`.

`joint.modules.luckFamily.{checks, pruned}` counts only usable family caps actually compared with the current
Top-K cutoff, and the resulting exclusions. It need not equal `boundedNodes` or the native preparation counts.
The cap retains the same canonical score/power tie rule as other joint bounds.

All times are measured inside the request's search budget. `preparationMs` contains its DP/recording work;
those nested profile times must not be added to it. Elapsed work before refusal or cancellation remains
counted. These counters do not add candidate evaluations, change a stop reason or establish `Complete`.
`peakBytes` is allocation accounting for this cache, not WASM linear-memory capacity or native/browser RSS.

## `composition`

Member composition → Snap traversal of regular Lives: `memberNodes` (by members placed), `snapNodes`,
`classNodes`, `bindingNodes`, `compositions` (member sets reached); the bounds `composition`, `team` (a whole
composition and its partial Snap pairings), `class`, `classBinding`, each `{checks, pruned}`; `modules`, the bound
modules by name (`memberAdditive`: score of Lives without Gekisou), each `{checks, pruned}`; `classInfeasible`,
`classResourceChecks`, `classResourceTightened`; seed candidates `seeds.{preseed, team, weighted, class,
powerFrontier}`; `powerFrontierClosed`.

## `candidate`

Heuristic candidate strategy: `warmupMemberSets`, `warmupProposals`, `explorationProposals`.

## `lotteryRefinement`

After the physical candidate domain is exhausted, refine only candidates and orders that still affect ranking.
Counters accumulate within the request; exhausting an allowance or declining refinement is not a proof.

| Field | Meaning |
|---|---|
| `attemptedOrders` | Orders for which construction of a complete nominal law was attempted |
| `completedOrders` | Orders with a complete mass-one law, including laws reused for an equal initialized model |
| `installedOrders` | Complete laws successfully used to narrow the ranking frontier |
| `declinedOrders` | Orders without a complete law due to unsupported inputs or random sources, work allowances, cancellation or arithmetic capacity |
| `declines` | Provider refusal counts: `domain`, `branchDepth`, `workBudget`, `arithmetic`, `unhandledRandom`, `cancelled`, `unsupported` |
| `budgetExhausted` | The shared replay or frame allowance reached zero; this flag alone establishes no ranking result |
| `arithmeticDeclines` | Complete laws not installed because search-side exact payoff arithmetic could not represent the result |
| `replayRuns`, `frames`, `terminalPaths` | Replay segments started, frames executed and terminal paths completed across all refinement attempts; reused complete laws add zero playback work |

Refinement time is charged to `time.simulationMs`; its runs are separate from the coarse 120-order evaluations
counted by `leaves.simulations`. Refinement can stop once the ranking is certified, so `Complete` does not require
every order to have an exact law or every returned expectation to have an exact rational value.

The [nominal LUCK refinement method](luck-refinement.md) describes the frame checkpoints,
work accounting and retained ranking certificates.

## `caches`

`candidates` (decks already evaluated or pruned; teams in their canonical layout for played lives), `bonusRows`
(row tables of the PT bonus bound) and `rushWindows` (Rush entry windows), each `{lookups, hits, evictions, peakEntries}`; `evictions` counts entries
dropped (all of them when a cache is cleared). `bonusRowsRefused` counts lookups that gave up the bound because the
table was full.

`luckCurves` exposes the request's lottery and score-summary reuse. `propagatedCurves` counts completed uncached
DP propagations; `peakStates` and `transitions` include uncached propagation work even when interrupted. Hits add
no propagation work. `recordingLookups` and `recordingHits` count compiled recorder reuse. `recordingPeakEntries`
and `recordingPeakBytes` record the largest retained identity count and key storage observed in a session. The
latter includes the shared byte dictionary, complete raw or delta payloads and entry-buffer capacity; it excludes
the shared probability objects and temporary encoding allocations. The recorder-key cache retains at most
128 entries and one MiB, further limited by the supplied curve-cache allowance; immutable dictionary bytes and
actual entry-buffer capacity count toward that same limit. Full reconstructed key equality decides every hit.
`sharedRecordingLookups` and `sharedRecordingHits` count a separate request-level table for complete reduced
recordings that require no life interpreter. Reuse requires both the unchanged recording key and the complete
owned context, including initialized model state and exact chart, play, setup and ranking inputs. Initial power
is normalized only because this reduced interpreter never calculates score. `sharedRecordingScopeBuilds`,
`sharedRecordingScopeBytes` and `sharedRecordingScopeDeclines` count scope construction, total successfully
encoded bytes and optional refusals. `sharedRecordingScopeMs` is a diagnostic timer included in `recordMs`.
`sharedRecordingKeyDeclines` and `sharedRecordingCapacityDeclines` count identity and retention refusals.
`sharedRecordingPeakEntries` and `sharedRecordingPeakBytes` describe its independent allowance of at most
128 entries and one MiB, further limited by the supplied curve-cache allowance. The latter counts the retained
scope, complete keys, entry-buffer capacity and each distinct retained probability allocation once; it is not
process RSS and does not include the unchanged session-local table or temporary encoding allocations. The two
tables can retain the same recording identity. Unsupported contexts continue through the session-local table
and ordinary recorder, and capacity refusals do not change the probability result or completion state.
`summaryLookups` and
`summaryHits` count complete score-summary reuse for equal initialized models; `summaryPeakEntries` and
`summaryPeakBytes` bound the largest session cache observed. A zero request cache capacity disables these caches.
`programLookups` and `programHits` count initialized-model lookups and hits for compiled all-path factor
histories whose admitted control flow is independent of initial total power. After a new model passes its own
complete recorder and admission checks, `programRecordedLookups` and `programRecordedHits` count a second
lookup using all inputs consumed by bounds replay. The latter can reuse a program from a different initialized
model only when the complete recorded inputs match and the same certified probability curve is retained.
`programRecordedKeyDeclines` counts recorded identities refused by the key byte allowance; cancellation is
separate. `programRecordedPeakKeyBytes` is the largest successfully constructed recorded identity observed.
Each hit reevaluates the original note arithmetic and signed rank operations at the requested power.
`programCompilations` counts completed programs, including those refused retention by capacity;
`programEvictions`, `programPeakEntries` and `programPeakBytes` describe the bounded resident cache, including
its shared chart/run context, recorded identities and retained probability curves. Both lookups refer to the
same retained program; a recorded hit creates no initialized-model alias. This cache is separate from the
deterministic native `ScoreProgram` cache. Reuse adds no factor-history replay work.
`leaves.simulations` counts completed order enclosures, including those in a candidate subsequently excluded by
remaining-order caps; summary hits still supply complete order enclosures without playback.
`luckScoreCaps` counts reuse of certified whole-program score upper bounds retained after partial-order
exclusion. These entries supply upper bounds, not candidate values or completion certificates.

## `memory`

`peakBytes`: the most memory the program has held when the document was written. On WebAssembly it is the size of
the linear memory (`memory.buffer.byteLength`), which never shrinks, so it covers the whole life of the instance; on
Linux the peak resident set size of the process; null elsewhere.

## Suggested page fields

- Status: `proof.complete`; when not complete and `proof.bestGap` is not null, show "not proven; the optimum exceeds
  the current best by at most x%". `proof.globalUpperBound` against `proof.best` gives the same statement for the
  whole domain.
- Progress: `proof.fraction` (labelled as position-based), or `proof.topLevelDone / proof.topLevelTotal`.
- Convergence: `best`, `kth` and `upper` against `atMs` from `incumbents.timeline`.
- Time breakdown: `name`, `startMs`, `wallMs` of `phases`.
- Data identity: `environment.data` (region, master version, data SHA-256).
- Scale: `nodes`, `leaves.visited`, `leaves.simulations`.

The other fields serve development.
