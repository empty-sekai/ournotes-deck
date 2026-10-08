# LUCK Gekisou real-chart search benchmark status

**The LUCK Gekisou target is not met: 0 of 18 original LUCK Score requests complete, and only 3 of 36
LUCK requests complete overall.** Public source
[`d06fe6cb975cbfb9d7d5c57576f62346a77c01f2`](https://github.com/empty-sekai/ournotes-deck/commit/d06fe6cb975cbfb9d7d5c57576f62346a77c01f2)
has the same result in native and actual Chromium Worker execution. The other 33 LUCK requests time out
at their original 60-second limit and remain unproven. The three completed requests are veteran
`clientEventPoints` cases that already completed at source `21954072`; there are **zero newly completed
LUCK requests**. All three finish within 20 seconds. This evidence does not resolve
[issue #37](https://github.com/empty-sekai/ournotes-deck/issues/37).

The [public real-chart CI run](https://github.com/empty-sekai/ournotes-deck/actions/runs/37792504759)
has finished all 49 planned requests in both runtimes. Its broader total is 12 Complete / 37 TimedOut per
runtime: 36 LUCK requests, 12 original Free/no-LUCK controls and one separately qualified Issue #9
reproduction without LUCK. The completed controls do not count as progress on the LUCK target. The
49-case completion/20-second target gate fails, while execution and result contracts pass.

The [current compact evidence receipt](search-benchmark-evidence.json) preserves every original input
triple, answer hash, unrounded time, completion state and cross-runtime/version comparison. It also
records the LUCK classification per request. The previous
[21954072 evidence receipt](search-benchmark-evidence-21954072.json) is archived byte for byte; the older
complete 64-case report remains in [its source-specific archive](search-benchmark-history-4f30cc.md).
Synthetic performance is not used as evidence for the LUCK target.

## Original inputs and the verified LUCK subset

The 48-request matrix is the original [PR #40](https://github.com/empty-sekai/ournotes-deck/pull/40)
[fixture at `9372d432e43f600d62950c88766126fc9ea21a98`](https://github.com/empty-sekai/ournotes-deck/tree/9372d432e43f600d62950c88766126fc9ea21a98/tools/search-harness/fixtures/full48).
Its six fixture files and two preparation/materialization source files were checked byte for byte against
that Git commit. All 48 materialized requests were reconstructed from the original templates and existing
native preparation streams without rerunning or replacing preparation. Across both compared sources and
both runtimes, all **192 data/snapshot/request SHA-256 triples** match the unchanged inputs. Complete
snapshot and legacy-roster cultivation and ownership values match the three published profiles.

LUCK membership is checked from each original request's `execution.gekisou`, `execution.scoreId` and
`scenario.musicId`, the pinned `MasterLiveMusic` mission fields, the corresponding `MasterLiveMusicScore`
row, and all three actual chart fever intervals. A request is included only when Gekisou is enabled and
at least one active mission is `2` (LUCK). Case names are not used for classification. Six enabled charts
produce 36 LUCK requests: four charts have missions `[2, 2, 2]`, and two have `[1, 2, 3]`. Free requests
using a LUCK chart have Gekisou disabled and are excluded. The no-LUCK controls have `[3, 3, 3]` or
`[1, 1, 1]`; the reported Battle request has `[3, 3, 3]`.

“Real-chart” means published game charts, cards, skills and scoring tables. The PR #40 inventories are
**fixed hypothetical inventories, not player-account exports**: newcomer has 20 members / 12 Snaps,
midcore 23 / 19, and veteran 44 / 42. The eight distinct original score IDs are `10000203`, `10002003`,
`10006103`, `10007003`, `10009203`, `10009303`, `10010503` and `10010703`.

The dataset is pinned TW game data, master version `0947498bc108756b2cc388d4c971105e`, 3,860,477 bytes,
SHA-256 `de867d2df3020e9430c164cdc889cd114113e50b2ab12003b6978665905493bf`.
The [source pin](../tools/search-harness/fixtures/full48/source.json) identifies the immutable public dataset
and replay manifest. Every original PR #40 request retains **K = 3, 60,000 ms, 1,024 cache entries, no
candidate cap, empty hard constraints, all legal members/leaders/paired Snaps and all 120 uniform
performer orders**. The 90-second external Worker watchdog does not enlarge the search allowance.

The new input receipt has a different hash because it adds an `evidence` field containing four source and
preparation file digests. Removing only that field reproduces the old receipt bytes exactly. The bundled
manifest is unchanged; all 192 original data/snapshot/request/roster file references, comprising 39 unique
blobs, were compared byte for byte with the original materialization. Preparation output and specs also
match exactly. The receipt records the preparation binary identity; that binary was not included in the
input bundle and preparation was not re-executed by this audit.

Native end-to-end time runs from process spawn through exit, including startup, input reads, dataset
construction, recommendation and output persistence. Chromium time runs from the host page invocation
through the final Worker response, including input transport, Worker creation, WASM initialization,
dataset/solver construction and recommendation. Browser installation/launch, server startup and host
fixture reads are outside that scope. The target requires a complete/proven canonical ranking within
20 seconds of these intervals, not just a fast inner search phase.

## LUCK completion and correctness evidence

| LUCK objective | Original requests | Native complete / timeout | Chromium complete / timeout | Complete within 20 s, each runtime | Newly completed |
| --- | ---: | ---: | ---: | ---: | ---: |
| Score | 18 | 0 / 18 | 0 / 18 | 0 | 0 |
| Score at least threshold | 4 | 0 / 4 | 0 / 4 | 0 | 0 |
| Capped Score | 5 | 0 / 5 | 0 / 5 | 0 | 0 |
| Score and final life at least thresholds | 4 | 0 / 4 | 0 / 4 | 0 | 0 |
| Client event points | 5 | 3 / 2 | 3 / 2 | 3 | 0 |
| **All LUCK** | **36** | **3 / 33** | **3 / 33** | **3** | **0** |

Newcomer and midcore each have 12 LUCK requests and zero completions. Veteran has 12 LUCK requests,
three completions and nine timeouts. All three completed cases are the already-complete
`short-veteran-pt`, `dense-veteran-pt` and `mixed-long-veteran-pt`.

All 48 original PR #40 cases executed in both runtimes and passed transport/result contracts. All 12
complete native/Chromium pairs, including the three LUCK pairs, have identical ordered canonical Top-K
members, paired Snaps and power. All 48 cross-runtime comparisons have compatible exact rational
score/payoff certificates for shared returned teams. Across revisions, all 96 same-runtime comparisons
are compatible and all 24 complete-result semantic projection hashes are unchanged. Both the 12-case
full-matrix completed set and its three-case LUCK subset are unchanged from `21954072`.

The audit checked all 192 original answer hashes and reports, passing 25,625 checks. The reported request
has a separate 895-check audit, with two timed-out runtime results and compatible exact scores/payoffs
for four shared returned teams. The combined 49-case report matches its four original shard reports.
These are integrity and compatibility checks, not an independent exhaustive ranking or game-client
oracle. An unfinished compatible answer is not a certified global Top-K.

The separate [standard CI run](https://github.com/empty-sekai/ournotes-deck/actions/runs/37792504732)
at this measured source is not green. Its [test job](https://github.com/empty-sekai/ournotes-deck/actions/runs/37792504732/job/113363306851)
stopped at two Clippy diagnostics: `type_complexity` in `family_rank_rush_history_tests.rs` and
`collapsible_if` in `search/snaps/profile_mean.rs`. The test step therefore did not run. Independent
correctness, MSRV and diagnostic jobs passed. Later source-specific local lint fixes and correctness
checks do not retroactively change this CI outcome. Independent nominal-law/ranking correctness tests
remain useful separately from performance; synthetic throughput does not establish the LUCK target.

## Every original LUCK request

Times are end-to-end seconds rounded to three decimals. The receipt retains original millisecond values
and exact request thresholds. Every timeout remains listed: its elapsed time is censored by the request
limit and is not a complete-proof time.

### Newcomer LUCK

| Request | Native status | Native seconds | Chromium status | Chromium seconds |
| --- | --- | ---: | --- | ---: |
| `short-newcomer-score` | TimedOut | 60.059 | TimedOut | 60.939 |
| `long-newcomer-score` | TimedOut | 60.056 | TimedOut | 60.940 |
| `mixed-short-newcomer-score` | TimedOut | 60.059 | TimedOut | 60.931 |
| `sparse-newcomer-score` | TimedOut | 60.055 | TimedOut | 60.923 |
| `dense-newcomer-score` | TimedOut | 60.060 | TimedOut | 60.927 |
| `mixed-long-newcomer-score` | TimedOut | 60.057 | TimedOut | 60.968 |
| `short-newcomer-probability` | TimedOut | 60.060 | TimedOut | 60.951 |
| `long-newcomer-life` | TimedOut | 60.054 | TimedOut | 60.951 |
| `mixed-short-newcomer-pt` | TimedOut | 60.059 | TimedOut | 60.915 |
| `sparse-newcomer-capped` | TimedOut | 60.054 | TimedOut | 60.930 |
| `dense-newcomer-probability` | TimedOut | 60.059 | TimedOut | 60.925 |
| `mixed-long-newcomer-life` | TimedOut | 60.056 | TimedOut | 60.961 |

### Midcore LUCK

| Request | Native status | Native seconds | Chromium status | Chromium seconds |
| --- | --- | ---: | --- | ---: |
| `short-midcore-score` | TimedOut | 60.058 | TimedOut | 60.936 |
| `long-midcore-score` | TimedOut | 60.056 | TimedOut | 60.928 |
| `mixed-short-midcore-score` | TimedOut | 60.062 | TimedOut | 60.935 |
| `sparse-midcore-score` | TimedOut | 60.054 | TimedOut | 60.923 |
| `dense-midcore-score` | TimedOut | 60.060 | TimedOut | 60.924 |
| `mixed-long-midcore-score` | TimedOut | 60.056 | TimedOut | 60.933 |
| `short-midcore-capped` | TimedOut | 60.059 | TimedOut | 60.943 |
| `long-midcore-probability` | TimedOut | 60.055 | TimedOut | 60.915 |
| `mixed-short-midcore-life` | TimedOut | 60.062 | TimedOut | 60.948 |
| `sparse-midcore-pt` | TimedOut | 60.055 | TimedOut | 60.930 |
| `dense-midcore-capped` | TimedOut | 60.060 | TimedOut | 60.940 |
| `mixed-long-midcore-capped` | TimedOut | 60.056 | TimedOut | 60.944 |

### Veteran LUCK

| Request | Native status | Native seconds | Chromium status | Chromium seconds |
| --- | --- | ---: | --- | ---: |
| `short-veteran-score` | TimedOut | 60.049 | TimedOut | 60.692 |
| `long-veteran-score` | TimedOut | 60.045 | TimedOut | 60.727 |
| `mixed-short-veteran-score` | TimedOut | 60.061 | TimedOut | 60.716 |
| `sparse-veteran-score` | TimedOut | 60.047 | TimedOut | 60.723 |
| `dense-veteran-score` | TimedOut | 60.050 | TimedOut | 60.734 |
| `mixed-long-veteran-score` | TimedOut | 60.056 | TimedOut | 60.717 |
| `short-veteran-pt` | Complete | 5.638 | Complete | 7.547 |
| `long-veteran-capped` | TimedOut | 60.045 | TimedOut | 60.704 |
| `mixed-short-veteran-probability` | TimedOut | 60.056 | TimedOut | 60.715 |
| `sparse-veteran-life` | TimedOut | 60.044 | TimedOut | 60.727 |
| `dense-veteran-pt` | Complete | 8.961 | Complete | 10.680 |
| `mixed-long-veteran-pt` | Complete | 7.049 | Complete | 9.106 |

## Auxiliary original controls

The 12 original Free/no-LUCK controls have nine Complete and three TimedOut results in each runtime.
All nine completions finish within 20 seconds and were already complete at the baseline. These controls
check retained behavior outside LUCK; they do not improve the LUCK completion count.

| Request | Native status | Native seconds | Chromium status | Chromium seconds |
| --- | --- | ---: | --- | ---: |
| `free-short-newcomer` | Complete | 0.356 | Complete | 1.541 |
| `free-short-midcore` | Complete | 0.620 | Complete | 1.942 |
| `free-short-veteran` | Complete | 0.366 | Complete | 1.311 |
| `free-long-newcomer` | Complete | 0.695 | Complete | 2.031 |
| `free-long-midcore` | Complete | 0.846 | Complete | 2.442 |
| `free-long-veteran` | Complete | 0.754 | Complete | 1.915 |
| `no-luck-short-newcomer` | Complete | 1.120 | Complete | 2.731 |
| `no-luck-short-midcore` | Complete | 4.936 | Complete | 8.371 |
| `no-luck-short-veteran` | Complete | 7.514 | Complete | 11.554 |
| `no-luck-long-newcomer` | TimedOut | 60.064 | TimedOut | 60.955 |
| `no-luck-long-midcore` | TimedOut | 60.059 | TimedOut | 60.954 |
| `no-luck-long-veteran` | TimedOut | 60.056 | TimedOut | 60.737 |

## Original reported request from Issue #9 / PR #10

[Issue #9](https://github.com/empty-sekai/ournotes-deck/issues/9) provides the original request and complete
15-member / 35-Snap roster for real `scoreId = 10001002`, `musicId = 100010`, Battle with Gekisou enabled,
theoretical-best play and three declared frame-zero rank-1 confirmations at 250%. Its pinned music row
has missions `[3, 3, 3]`: **this is not a LUCK request**. It retains **K = 5, 60,000 ms, 2,048 cache entries,
no candidate cap, empty constraints and the full legal domain**. The author describes the reproduction
roster as synthetic, not a player account; the harness preserves that reported roster rather than
generating a replacement. [PR #10](https://github.com/empty-sekai/ournotes-deck/pull/10) says the corrected
request still timed out under its declared limit.

The original request and roster values are preserved. A strict native projection audit compares all
members, Snaps, eligible leaders, cultivation and power fields before materialization; this audit performs
no search and supplies no ranking certificate. The report supplied neither original JP/international
dataset bytes nor a dataset hash. This reproduction explicitly selects the same pinned TW dataset above,
including the requested 390-note chart, without claiming exact historical dataset equivalence. The other
mentioned score ID, `10001001`, has no published matching roster and is not invented as another case.

The public 49-request run is complete and includes this original request in both runtimes:

| Runtime | Completion | Optimality | End-to-end seconds |
| --- | --- | --- | ---: |
| Native | TimedOut | unproven | 60.052 |
| Chromium Worker | TimedOut | unproven | 60.766 |

These two timed-out results are included in the full 49-case totals. The original input hashes, strict
projection proof, output hashes and provenance limitations are retained in the current CI evidence receipt.

## Subsequent local LUCK measurements and remaining work

The [local evidence receipt](search-benchmark-local-evidence.json) preserves earlier source-separated
native runs on the unchanged original inputs. Selected local measurements do not replace the complete
source-specific Chromium evidence above. They use AMD EPYC 9V74 / Linux x86_64 and Rust 1.88.0; all
original input bytes, candidate domains, 120 labels, cache limits and request budgets remain unchanged.

At source [`59105bb0938300aeb1b105938f1e3f138294cf04`](https://github.com/empty-sekai/ournotes-deck/commit/59105bb0938300aeb1b105938f1e3f138294cf04),
tree `39b4e311c40c8c1d5fbb34266dbc089a4b0c7a35`, three selected original LUCK Score requests still time out
and remain unproven:

| Original LUCK request | Completion | End-to-end seconds | Search nodes | Visited candidates |
| --- | --- | ---: | ---: | ---: |
| `short-newcomer-score` | TimedOut | 60.054 | 9,807 | 14 |
| `short-midcore-score` | TimedOut | 60.050 | 763 | 63 |
| `long-newcomer-score` | TimedOut | 60.050 | 856 | 49 |

Short newcomer spends 41.188 seconds in family preparation and reaches only 14 physical candidates.
Earlier controller reuse removed many repeated native program builds but did not remove repeated
whole-family admission and preparation costs. A larger visited-node count within an unfinished request
is not a complete-ranking speedup.

One further native measurement at source
[`8ad653fd1328cb71ebfb7c4ae939a44a1cc9bfbd`](https://github.com/empty-sekai/ournotes-deck/commit/8ad653fd1328cb71ebfb7c4ae939a44a1cc9bfbd),
tree `854203b9956e27baec2affcbd0cd2675444802c8`, gives the same unresolved LUCK result:

| Original LUCK request | Completion | Optimality | End-to-end seconds | Search nodes | Visited candidates | Family preparation seconds |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| `short-newcomer-score` | TimedOut | unproven | 60.075000 | 9,086 | 14 | 41.046630 |

Its original answer SHA-256 is `5d23e60e6ef9c1c25bb678e44dfeb3e9668120949b0cbb95e01c3d3f5b8aec40`. The unrounded end-to-end time is
`60074.999892` ms; family preparation is `41,046.629659` ms. This is the baseline before the
subsequent family-index and cutoff-short-circuit changes.

The source-index change alone, at
[`db80818ca23040ce2e5c2babfc5261aaf7eb9b1b`](https://github.com/empty-sekai/ournotes-deck/commit/db80818ca23040ce2e5c2babfc5261aaf7eb9b1b),
tree `56b5c6d50932f87c3f790f6d652b8b4eb5feab52`, also leaves that original request TimedOut/unproven:
60.057584 seconds end to end, 10,282 nodes and 14 visited candidates. Family preparation takes
41.517477 seconds for 11,083 admitted families, versus 41.046630 seconds for 10,262 at the preceding
source. These unfinished observations do not establish a complete-request speedup.

Adding the cutoff short circuit, at
[`a8c8f994c477beffe08f22ffa80d29ca7aadf6a8`](https://github.com/empty-sekai/ournotes-deck/commit/a8c8f994c477beffe08f22ffa80d29ca7aadf6a8),
tree `ddbfd2320d9249878425851e508e0c46abfb8e81`, still completes **none of the four unchanged original
LUCK Score requests**:

| Original LUCK request | Completion | End-to-end seconds | Search nodes | Visited candidates | Family preparation seconds | Simulation seconds |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| `short-newcomer-score` | TimedOut / unproven | 60.065716 | 303 | 67 | 0.997665 | 58.691552 |
| `short-midcore-score` | TimedOut / unproven | 60.067311 | 99 | 68 | 1.898025 | 57.886857 |
| `short-veteran-score` | TimedOut / unproven | 60.057374 | 68 | 65 | 2.903370 | 56.859586 |
| `long-newcomer-score` | TimedOut / unproven | 60.053924 | 54 | 49 | 0.193255 | 59.635666 |

The reduced preparation time does not translate into completed ranking. Short newcomer records only
94 available leaf caps in 156 checks, compared with 648 in 648 before the short circuit; its simulation
time grows from about 12 seconds to 59 seconds. Stopping optional node preparation can leave later
leaves without a retained profile cap, exposing expensive terminal evaluation. All four timeouts are
retained in the local evidence. The artifact audit checks 2,461 contracts and 135 shared candidate
observations with compatible score/payoff intervals; it does not certify a global Top-K for these
unfinished requests.

The subsequent source
[`2dd416de8906f837ccfd3b5417460d26f4869158`](https://github.com/empty-sekai/ournotes-deck/commit/2dd416de8906f837ccfd3b5417460d26f4869158),
tree `9f82758b56ce470dfd72b8c1d9fb74539447d472`, adds on-demand preparation for a legal cold leaf.
It retains the original complete physical-pair admission, prepares only the actual binding's required
profile over all 120 labels, and keeps other profiles unknown. Seven targeted checks pass, including
zero-cache, exact binding/owner remapping, budget retention and cancellation. The independent native
canonical-ranking oracle also passes for all 62 physical descendants and all 120 orders per descendant,
including cache enabled/disabled and truthful candidate-limit interruption. Default Clippy also passes;
the native-fixture Clippy run is interrupted and has no passing result at this source. These are
correctness and static-analysis checks. **No source-matched real-request performance result is available for this leaf change**, so the
four timed-out measurements above must not be attributed to it or treated as improved completion.

The same six-request local run at `59105bb0` also contains three non-LUCK controls: the original Issue #9
request completes/proves its ranking in 4.437 seconds, `free-short-veteran` in 0.401 seconds, and
`no-luck-short-midcore` in 4.387 seconds. The latter two were already-complete controls. The Issue #9
result is a useful separate regression result for cumulative-score history bounds, but **it is not a LUCK
breakthrough and is not a result of the public `d06fe6cb` CI run**.

## Platform differences and observed LUCK costs

Each request has one observation per source and runtime. From the `21954072` baseline to `d06fe6cb`,
newcomer changes from Intel Xeon Platinum 8573C to AMD EPYC 7763; veteran changes from AMD EPYC 9V45 to
AMD EPYC 9V74. Midcore uses AMD EPYC 7763 in both runs. CPU changes, multiple source changes and single
observations prevent attributing elapsed-time ratios to one algorithm. Timeout durations are censored,
so no completed speedup ratio is computed for them.

On public `d06fe6cb`, short newcomer visits only 14 candidates, while short midcore and short veteran
visit 56 and 77. Their native terminal-upper stage consumes 10.558, 51.147 and 54.148 seconds respectively;
terminal-recipe hits/builds are 0/1,656, 59/6,534 and 102/8,528. The later local short-newcomer measurements
above identify a different dominant cost: roughly 41 seconds of family preparation even after controller
program reuse. Both repeated complete-family preparation and per-candidate terminal recording/scoring
remain measured bottlenecks. None of these unfinished results proves the complete LUCK ranking.

## Public artifacts

All six archives below were downloaded and SHA-256 verified. The full49 aggregate's runs match the four
original result shards; its execution/contracts gate passes and completion target fails. The compact
receipt records the root report, input-receipt, manifest, original answer and source hashes.

| Artifact | SHA-256 |
| --- | --- |
| [full48-newcomer](https://github.com/empty-sekai/ournotes-deck/actions/runs/37792504759/artifacts/11559670264) | `b054966e6a774506f2515edfa128ef9ea4aa185296870a3d3235deb8db717e3a` |
| [full48-midcore](https://github.com/empty-sekai/ournotes-deck/actions/runs/37792504759/artifacts/11558527722) | `3415b4f02d8cb90c1ebc95b8fa0e7c34d59282d04a6bf067d265b638611a2111` |
| [full48-veteran](https://github.com/empty-sekai/ournotes-deck/actions/runs/37792504759/artifacts/11558706262) | `bfb4a91bb59537ea7e438881bc87d45c92ee3e4c2dea97db772d0a872a205c51` |
| [reported](https://github.com/empty-sekai/ournotes-deck/actions/runs/37792504759/artifacts/11558245402) | `c4459622638e4d611bed49a3f847edb311985f2fb52761b07eaa8c543a5ced68` |
| [summary](https://github.com/empty-sekai/ournotes-deck/actions/runs/37792504759/artifacts/11559066017) | `0c9eaff080b77eb1f8cb3e1117e969914bca54323d52baf14c4eaab7f6ea55ab` |
| [inputs](https://github.com/empty-sekai/ournotes-deck/actions/runs/37792504759/artifacts/11557406340) | `95de27351d49829cad49ec4787a8840f454f54eb22cfd760baec0d8607c91e1c` |
