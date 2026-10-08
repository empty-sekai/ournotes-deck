# Real-chart search benchmark status

**The 20-second end-to-end complete-ranking target is not met.** Public source
[`21954072ce7c8a0b66b564dc2a1b96bd2519ad98`](https://github.com/empty-sekai/ournotes-deck/commit/21954072ce7c8a0b66b564dc2a1b96bd2519ad98)
completes **12 of the original 48 real-chart requests** in native and actual Chromium Worker execution;
**36 time out in each runtime**. All 12 completed requests finish within 20 seconds. The completed set is
unchanged from `4f30cc845eb4df78e82cf05c233c8a20c386be00`: zero newly completed requests and zero lost completions.
No real LUCK score request completes, and all three long no-LUCK controls still time out.
This evidence does not resolve [issue #37](https://github.com/empty-sekai/ournotes-deck/issues/37).

The public [real-chart CI run](https://github.com/empty-sekai/ournotes-deck/actions/runs/37779863844) contains one serial execution per request and runtime.
The [compact evidence receipt](search-benchmark-evidence.json) retains every request's unrounded timings,
input hashes, original answer hashes, completion states and cross-runtime/version comparisons.
The previous complete 64-case report, including its historical synthetic results and correctness-job
limitations, is preserved unchanged in [the source-specific archive](search-benchmark-history-4f30cc.md).
Synthetic performance is excluded from the current target result.

## Original PR inputs and measurement scope

The matrix is the original [PR #40](https://github.com/empty-sekai/ournotes-deck/pull/40)
[fixture at `9372d432e43f600d62950c88766126fc9ea21a98`](https://github.com/empty-sekai/ournotes-deck/tree/9372d432e43f600d62950c88766126fc9ea21a98/tools/search-harness/fixtures/full48).
Its six fixture files and two preparation/materialization source files were checked byte for byte against
that Git commit. All 48 materialized requests were reconstructed from the original templates and existing
native preparation streams without rerunning or replacing preparation. Across both source revisions and
both runtimes, all **192 data/snapshot/request SHA-256 triples** match those unchanged inputs. Complete
snapshot and legacy-roster cultivation and ownership values match the three published profiles.

“Real-chart” means the published game charts, cards, skills and scoring tables. The three inventories are
**fixed hypothetical inventories, not player-account exports**: newcomer has 20 members / 12 Snaps,
midcore 23 / 19, and veteran 44 / 42. The eight distinct score IDs are `10000203`, `10002003`, `10006103`,
`10007003`, `10009203`, `10009303`, `10010503` and `10010703`. The matrix includes 18 LUCK score requests,
18 nonlinear requests and 12 Free/no-LUCK controls.

The dataset is pinned TW game data, master version `0947498bc108756b2cc388d4c971105e`, 3,860,477 bytes,
SHA-256 `de867d2df3020e9430c164cdc889cd114113e50b2ab12003b6978665905493bf`.
The [source pin](../tools/search-harness/fixtures/full48/source.json) identifies the immutable public dataset
and replay manifest. Each original request retains **K = 3, 60,000 ms, 1,024 cache entries, no candidate cap,
empty hard constraints, all legal members/leaders/paired Snaps and all 120 uniform performer orders**.
The 90-second external Worker watchdog does not enlarge the request's search allowance.

Native end-to-end time runs from process spawn through exit, including startup, input reads, dataset
construction, recommendation and output persistence. Chromium time runs from the host's page invocation
through the final Worker response, including input transport, Worker creation, WASM initialization,
dataset/solver construction and recommendation. Browser installation/launch, server startup and host
fixture reads are outside that scope. The target is complete/proven within 20 seconds of these intervals,
not merely a fast inner search phase.

## Completion and compatibility

| Inventory | Requests | Native complete / timeout | Chromium complete / timeout | Complete within 20 s, each runtime | Newly completed |
| --- | ---: | ---: | ---: | ---: | ---: |
| Newcomer | 16 | 3 / 13 | 3 / 13 | 3 | 0 |
| Midcore | 16 | 3 / 13 | 3 / 13 | 3 | 0 |
| Veteran | 16 | 6 / 10 | 6 / 10 | 6 | 0 |
| **Total** | **48** | **12 / 36** | **12 / 36** | **12** | **0** |

All 48 planned cases executed in both runtimes and passed transport/result contracts. All 12 pairs of
complete results have identical ordered canonical Top-K members, paired Snaps and power. For all 48
cross-runtime comparisons, the exact rational score/payoff certificates of shared returned teams are
compatible. The 36 unfinished requests remain unproven; compatible partial answers are not a complete
ranking. Across revisions, all 96 same-runtime comparisons are compatible and all 24 complete-result
semantic projection hashes are unchanged.

The audit checked all 192 original answer hashes against the per-case and top-level reports. These are
archive integrity and compatibility checks, not an independent exhaustive ranking or game-client oracle.
Small independent nominal-law/ranking correctness tests remain separate evidence; synthetic throughput
is not a substitute for these real-chart completion results. At the measured public source, the standard
test jobs were blocked by a Clippy `collapsible_if` failure; independent correctness, MSRV and diagnostic
checks passed. Later local fixes do not establish a green CI run for a different source.

## Every original real-chart request

Times below are end-to-end seconds rounded to three decimals. The evidence receipt retains the original
millisecond values. Every timeout is listed; a value near 60 seconds is a stopped request, not the time
needed to finish its complete proof.

### Newcomer

| Request | Native status | Native seconds | Chromium status | Chromium seconds |
| --- | --- | ---: | --- | ---: |
| `short-newcomer-score` | TimedOut | 60.047 | TimedOut | 60.703 |
| `long-newcomer-score` | TimedOut | 60.046 | TimedOut | 60.726 |
| `mixed-short-newcomer-score` | TimedOut | 60.048 | TimedOut | 60.707 |
| `sparse-newcomer-score` | TimedOut | 60.044 | TimedOut | 60.705 |
| `dense-newcomer-score` | TimedOut | 60.047 | TimedOut | 60.711 |
| `mixed-long-newcomer-score` | TimedOut | 60.048 | TimedOut | 60.737 |
| `short-newcomer-probability` | TimedOut | 60.045 | TimedOut | 60.721 |
| `long-newcomer-life` | TimedOut | 60.046 | TimedOut | 60.741 |
| `mixed-short-newcomer-pt` | TimedOut | 60.045 | TimedOut | 60.718 |
| `sparse-newcomer-capped` | TimedOut | 60.044 | TimedOut | 60.716 |
| `dense-newcomer-probability` | TimedOut | 60.047 | TimedOut | 60.736 |
| `mixed-long-newcomer-life` | TimedOut | 60.045 | TimedOut | 60.734 |
| `free-short-newcomer` | Complete | 0.389 | Complete | 1.364 |
| `free-long-newcomer` | Complete | 0.730 | Complete | 1.899 |
| `no-luck-short-newcomer` | Complete | 1.096 | Complete | 2.446 |
| `no-luck-long-newcomer` | TimedOut | 60.049 | TimedOut | 60.725 |

### Midcore

| Request | Native status | Native seconds | Chromium status | Chromium seconds |
| --- | --- | ---: | --- | ---: |
| `short-midcore-score` | TimedOut | 60.057 | TimedOut | 60.965 |
| `long-midcore-score` | TimedOut | 60.053 | TimedOut | 60.945 |
| `mixed-short-midcore-score` | TimedOut | 60.058 | TimedOut | 60.972 |
| `sparse-midcore-score` | TimedOut | 60.053 | TimedOut | 60.938 |
| `dense-midcore-score` | TimedOut | 60.058 | TimedOut | 60.940 |
| `mixed-long-midcore-score` | TimedOut | 60.054 | TimedOut | 60.950 |
| `short-midcore-capped` | TimedOut | 60.057 | TimedOut | 60.929 |
| `long-midcore-probability` | TimedOut | 60.053 | TimedOut | 60.936 |
| `mixed-short-midcore-life` | TimedOut | 60.059 | TimedOut | 60.966 |
| `sparse-midcore-pt` | TimedOut | 60.056 | TimedOut | 60.946 |
| `dense-midcore-capped` | TimedOut | 60.057 | TimedOut | 60.934 |
| `mixed-long-midcore-capped` | TimedOut | 60.053 | TimedOut | 60.957 |
| `free-short-midcore` | Complete | 0.787 | Complete | 2.173 |
| `free-long-midcore` | Complete | 0.965 | Complete | 2.493 |
| `no-luck-short-midcore` | Complete | 5.119 | Complete | 8.724 |
| `no-luck-long-midcore` | TimedOut | 60.062 | TimedOut | 60.993 |

### Veteran

| Request | Native status | Native seconds | Chromium status | Chromium seconds |
| --- | --- | ---: | --- | ---: |
| `short-veteran-score` | TimedOut | 60.041 | TimedOut | 60.543 |
| `long-veteran-score` | TimedOut | 60.035 | TimedOut | 60.559 |
| `mixed-short-veteran-score` | TimedOut | 60.048 | TimedOut | 60.554 |
| `sparse-veteran-score` | TimedOut | 60.035 | TimedOut | 60.546 |
| `dense-veteran-score` | TimedOut | 60.035 | TimedOut | 60.521 |
| `mixed-long-veteran-score` | TimedOut | 60.047 | TimedOut | 60.552 |
| `short-veteran-pt` | Complete | 4.026 | Complete | 5.453 |
| `long-veteran-capped` | TimedOut | 60.034 | TimedOut | 60.524 |
| `mixed-short-veteran-probability` | TimedOut | 60.045 | TimedOut | 60.566 |
| `sparse-veteran-life` | TimedOut | 60.033 | TimedOut | 60.553 |
| `dense-veteran-pt` | Complete | 6.690 | Complete | 7.971 |
| `mixed-long-veteran-pt` | Complete | 5.256 | Complete | 6.602 |
| `free-short-veteran` | Complete | 0.307 | Complete | 1.059 |
| `free-long-veteran` | Complete | 0.595 | Complete | 1.558 |
| `no-luck-short-veteran` | Complete | 2.366 | Complete | 4.038 |
| `no-luck-long-veteran` | TimedOut | 60.044 | TimedOut | 60.546 |

## Platform differences and observed bottlenecks

Each request has only one observation per runtime and source. Newcomer changed from AMD EPYC 9V74 to
Intel Xeon Platinum 8573C; veteran changed from Intel Xeon Platinum 8573C to AMD EPYC 9V45. Midcore used
AMD EPYC 7763 in both runs. In particular, the lower veteran PT timings cannot be presented as an
algorithmic speedup: their visited/node counts are unchanged and the CPU changed. Timeout duration is
censored by the request limit and supplies no completed speedup ratio.

For the short LUCK score requests, the native terminal upper-bound stage still consumes 49.494 seconds
(newcomer), 50.195 seconds (midcore) and 54.231 seconds (veteran). Terminal-recipe hits/builds are respectively
0/7,394, 8/6,314 and 0/11,357. Repeated recording and terminal scoring remain major costs.

For the long no-LUCK requests, newcomer fine-order bounds fall from 395,087 to 245,285 and midcore from
214,441 to 134,019, but native simulation still takes 45.807 and 47.155 seconds. Their prefix-sharing trees
execute 86.68 million and 72.61 million native frames. More candidates fit into the same unfinished
request; this has not produced a completed ranking. These deterministic cases require improvements
outside probability propagation as well.

## Original reported request from issue #9 / PR #10

[Issue #9](https://github.com/empty-sekai/ournotes-deck/issues/9) supplies an original request and a complete
15-member / 35-Snap roster for real `scoreId = 10001002`, `musicId = 100010`, Battle with Gekisou enabled,
theoretical-best play and three declared frame-zero rank-1 confirmations at 250%. The request keeps
**K = 5, 60,000 ms, 2,048 cache entries, no candidate cap and empty constraints**. The issue itself describes
the roster as synthetic, not a player account. This is an externally reported request on a real game chart.
[PR #10](https://github.com/empty-sekai/ournotes-deck/pull/10) reports that the corrected request still returns
`TimedOut` under the declared limit; it does not publish a complete-ranking time.

The original request and roster values are unchanged in the local reproduction. It uses the same explicitly
pinned TW dataset described above, which contains the requested 390-note chart and validates the roster.
The report did not supply its original JP/international dataset bytes or hash, so this is a reproduction on
that identified TW version, not a claim of exact historical dataset equivalence. The other mentioned
score ID, `10001001`, has no published matching roster and is not invented as another benchmark case.

A local CLI run of source `712d7255822137e5b59cc8c15514cfc45d56d2ad`,
tree `9baa5ec96e82d0b84e0c085a1c2088e0b68ed9ee`, exits normally with **`TimedOut`, unproven, in
60.1077 seconds end to end**. It visits 672 candidates and performs 15,079 native
order simulations; native simulation accounts for 57.719 seconds. The exact CLI, input and output hashes
are recorded in the evidence receipt. This single failed local run is separate from the 48-case public CI
matrix and is not an optimization success. A reported-request CI suite is being integrated; no completed
public CI pass for that suite is claimed here.

## Public artifacts

These three downloaded real-chart shard archives and their root reports were SHA-256 verified. Each
belongs to the public source and workflow linked above; the receipt also records report, input-receipt
and manifest hashes. No unverified aggregate archive is needed for this result.

| Artifact | SHA-256 |
| --- | --- |
| [Newcomer](https://github.com/empty-sekai/ournotes-deck/actions/runs/37779863844/artifacts/11552564726) | `d43f3840586d0c5ea1e0f6f665ebca059411c0ed674e33e8c4371f5ba24e08c3` |
| [Midcore](https://github.com/empty-sekai/ournotes-deck/actions/runs/37779863844/artifacts/11553371943) | `6109aa237d3eb86949af6e81f76a187a573f3ebeb6e7af3e0a9964bccf7775e2` |
| [Veteran](https://github.com/empty-sekai/ournotes-deck/actions/runs/37779863844/artifacts/11553417742) | `097c3404a8c5d3280dca2266cd2bd75aed7c6a9b4e08d56b02a2b848deb929eb` |
