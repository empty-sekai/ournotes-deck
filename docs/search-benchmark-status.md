# Complete search benchmark status

## Scope and evidence

This report records immutable source commit
[`4f30cc845eb4df78e82cf05c233c8a20c386be00`](https://github.com/empty-sekai/ournotes-deck/commit/4f30cc845eb4df78e82cf05c233c8a20c386be00)
from [PR #38](https://github.com/empty-sekai/ournotes-deck/pull/38).
The complete [Chromium matrix run](https://github.com/empty-sekai/ournotes-deck/actions/runs/37773916872)
finished on 2026-10-08. Each result is one serial request per case and runtime, not a median or a
statistical speedup estimate. Both runtimes use the same source and exact inputs. All 64 original local
data/snapshot/roster/request quadruplets match the CI input receipts byte for byte; runtime hashes match
those receipts as well.

Every request keeps its original 60,000 ms search limit and K = 3. Cache capacity is 1,024 entries except
`short-score-cache0`, which keeps zero. The full legal member, leader and paired-Snap domains remain in
scope. The uniform score objective retains all 120 original equally weighted performer orders. No
sampling, ownership truncation or increased proof-work allowance is used to count a request as complete.

The real matrix contains 48 requests over pinned public chart data and complete generated ownership pools:
18 LUCK score requests, 18 nonlinear requests and 12 Free/no-LUCK controls. The three generated pools
contain 20/12, 23/19 and 44/42 member/Snap resources. They are declared benchmark pools, not private
player accounts. The dataset SHA-256 is
`de867d2df3020e9430c164cdc889cd114113e50b2ab12003b6978665905493bf`.
The additional synthetic corpus contains all 16 original cases.

Native end-to-end time includes process start, input reads, solver construction, search, output and exit.
Chromium end-to-end time includes input transport into the page, Worker construction, WASM initialization,
dataset/solver construction and the recommendation response. It excludes browser installation, host
server startup and browser launch. Chromium was 145.0.7632.6. The external Worker watchdog is 90 seconds;
it does not increase the search allowance. The target remains complete/proven within 20 seconds end to end.

## Result

| Corpus | Cases | Native complete/proven | Chromium complete/proven | Native complete within 20 s | Chromium complete within 20 s |
| --- | ---: | ---: | ---: | ---: | ---: |
| Real newcomer | 16 | 3 | 3 | 3 | 3 |
| Real midcore | 16 | 3 | 3 | 3 | 3 |
| Real veteran | 16 | 6 | 6 | 6 | 6 |
| Synthetic | 16 | 14 | 14 | 12 | 7 |
| **Total** | **64** | **26** | **26** | **24** | **19** |

All 64 planned cases executed in both runtimes and passed the transport/result contracts. The 26 cases
that completed in both runtimes have identical canonical Top-K. The other 38 cases timed out in both
runtimes; their reported certificates are compatible, which does not establish a final ranking or an
optimality proof. No real LUCK score request completed. All three long no-LUCK controls also timed out.
This source therefore does **not** meet the target and does not resolve issue #37.

The separate [correctness workflow](https://github.com/empty-sekai/ournotes-deck/actions/runs/37773773164)
passed its independent synthetic nominal-law ranking oracle and MSRV checks. Two test jobs exposed three
integration fixtures that required the RNG-law provider to run even when a complete mapped-payoff
provider could certify the ranking first. That source's full test suite was not green. Subsequent test
changes validate native probability enclosures and actual provider evidence, preserving the original
canonical winner and complete-domain assertions. The large real matrix is not an independent exhaustive oracle.

## Every original request

Times are process/Worker end-to-end seconds, rounded to three decimals. `TimedOut` is unfinished and is
never counted as complete or proven. The workflow artifacts preserve unrounded timings, exact hashes,
telemetry and output JSON.

### Real newcomer

| Request | Completion in both runtimes | Native seconds | Chromium seconds |
| --- | --- | ---: | ---: |
| `short-newcomer-score` | TimedOut | 60.046 | 60.700 |
| `long-newcomer-score` | TimedOut | 60.044 | 60.728 |
| `mixed-short-newcomer-score` | TimedOut | 60.048 | 60.721 |
| `sparse-newcomer-score` | TimedOut | 60.043 | 60.709 |
| `dense-newcomer-score` | TimedOut | 60.047 | 60.724 |
| `mixed-long-newcomer-score` | TimedOut | 60.045 | 60.729 |
| `short-newcomer-probability` | TimedOut | 60.048 | 60.717 |
| `long-newcomer-life` | TimedOut | 60.050 | 60.740 |
| `mixed-short-newcomer-pt` | TimedOut | 60.048 | 60.711 |
| `sparse-newcomer-capped` | TimedOut | 60.043 | 60.724 |
| `dense-newcomer-probability` | TimedOut | 60.049 | 60.729 |
| `mixed-long-newcomer-life` | TimedOut | 60.044 | 60.708 |
| `free-short-newcomer` | Complete | 0.395 | 1.355 |
| `free-long-newcomer` | Complete | 0.742 | 1.893 |
| `no-luck-short-newcomer` | Complete | 1.085 | 2.436 |
| `no-luck-long-newcomer` | TimedOut | 60.052 | 60.732 |

### Real midcore

| Request | Completion in both runtimes | Native seconds | Chromium seconds |
| --- | --- | ---: | ---: |
| `short-midcore-score` | TimedOut | 60.060 | 60.950 |
| `long-midcore-score` | TimedOut | 60.056 | 60.945 |
| `mixed-short-midcore-score` | TimedOut | 60.061 | 60.964 |
| `sparse-midcore-score` | TimedOut | 60.055 | 60.922 |
| `dense-midcore-score` | TimedOut | 60.060 | 60.946 |
| `mixed-long-midcore-score` | TimedOut | 60.054 | 60.935 |
| `short-midcore-capped` | TimedOut | 60.058 | 60.934 |
| `long-midcore-probability` | TimedOut | 60.064 | 60.943 |
| `mixed-short-midcore-life` | TimedOut | 60.060 | 60.941 |
| `sparse-midcore-pt` | TimedOut | 60.053 | 60.935 |
| `dense-midcore-capped` | TimedOut | 60.053 | 60.933 |
| `mixed-long-midcore-capped` | TimedOut | 60.061 | 60.930 |
| `free-short-midcore` | Complete | 0.804 | 2.213 |
| `free-long-midcore` | Complete | 0.974 | 2.510 |
| `no-luck-short-midcore` | Complete | 5.197 | 8.843 |
| `no-luck-long-midcore` | TimedOut | 60.056 | 60.974 |

### Real veteran

| Request | Completion in both runtimes | Native seconds | Chromium seconds |
| --- | --- | ---: | ---: |
| `short-veteran-score` | TimedOut | 60.057 | 60.755 |
| `long-veteran-score` | TimedOut | 60.050 | 60.780 |
| `mixed-short-veteran-score` | TimedOut | 60.064 | 60.791 |
| `sparse-veteran-score` | TimedOut | 60.052 | 60.800 |
| `dense-veteran-score` | TimedOut | 60.050 | 60.772 |
| `mixed-long-veteran-score` | TimedOut | 60.063 | 60.786 |
| `short-veteran-pt` | Complete | 6.643 | 8.568 |
| `long-veteran-capped` | TimedOut | 60.057 | 60.829 |
| `mixed-short-veteran-probability` | TimedOut | 60.058 | 60.783 |
| `sparse-veteran-life` | TimedOut | 60.050 | 60.830 |
| `dense-veteran-pt` | Complete | 10.306 | 11.714 |
| `mixed-long-veteran-pt` | Complete | 8.283 | 10.289 |
| `free-short-veteran` | Complete | 0.490 | 1.653 |
| `free-long-veteran` | Complete | 0.946 | 2.297 |
| `no-luck-short-veteran` | Complete | 3.905 | 6.315 |
| `no-luck-long-veteran` | TimedOut | 60.059 | 60.822 |

### Synthetic

| Request | Completion in both runtimes | Native seconds | Chromium seconds |
| --- | --- | ---: | ---: |
| `short-score` | Complete | 0.369 | 0.874 |
| `short-score-cache0` | Complete | 16.671 | 22.552 |
| `short-probability` | Complete | 19.263 | 28.088 |
| `short-capped` | Complete | 16.925 | 25.491 |
| `short-pt` | Complete | 0.298 | 0.763 |
| `short-life` | Complete | 0.296 | 0.738 |
| `short-free` | Complete | 0.033 | 0.243 |
| `short-no-luck` | Complete | 0.045 | 0.274 |
| `conditional-score` | Complete | 0.469 | 1.045 |
| `conditional-capped` | Complete | 13.677 | 20.943 |
| `long-score` | Complete | 17.148 | 22.953 |
| `long-probability` | TimedOut | 60.009 | 60.141 |
| `long-free` | Complete | 21.242 | 29.544 |
| `long-no-luck` | Complete | 22.985 | 31.690 |
| `near-ties-score` | Complete | 12.250 | 17.331 |
| `near-ties-capped` | TimedOut | 60.009 | 60.121 |

## Artifact identities

Every artifact below belongs to the immutable source and workflow above.

| Artifact | SHA-256 |
| --- | --- |
| [Newcomer](https://github.com/empty-sekai/ournotes-deck/actions/runs/37773916872/artifacts/11550691312) | `ccf3b05ddcc9de0ca1b525b5924362e9c3bad869b0af970ca3ae961299843bf2` |
| [Midcore](https://github.com/empty-sekai/ournotes-deck/actions/runs/37773916872/artifacts/11550746440) | `bdffe3f68a4b4a84691c82c91e2b7b99f86aec8b7114fa053960b2d59f1cde07` |
| [Veteran](https://github.com/empty-sekai/ournotes-deck/actions/runs/37773916872/artifacts/11550860512) | `5dfdbbffd8ad1509731ff11802cd197ae15992d723c8200282c3c85601208263` |
| [Synthetic](https://github.com/empty-sekai/ournotes-deck/actions/runs/37773916872/artifacts/11549343480) | `b51fdaa3ec56cac06f17ccf0a67d7ef91d6f4c32bbf028d004c3b3557f17ed28` |
| [Aggregate](https://github.com/empty-sekai/ournotes-deck/actions/runs/37773916872/artifacts/11551141111) | `21b753c76012f9545e6c3f99263499aebb0e88729e496c3824178733832c6a7e` |
| [Inputs and builds](https://github.com/empty-sekai/ournotes-deck/actions/runs/37773916872/artifacts/11549261750) | `519b4ba03b29b1efe11859d068f09c4b60bd9c15fb90cd42ad6f07e94d92ede6` |

## Measured bottlenecks

The real newcomer short LUCK score request spent 57.18 seconds in its terminal upper-bound stage,
preparing 8,506 orders and excluding 91 individual candidates. Only three candidates received complete
evaluations. Stronger complete family exclusions and reusable terminal recordings directly target that work.

The real newcomer long no-LUCK control followed the deterministic path: 7,042 visited candidates,
6,681 fine-bound exclusions, 395,087 fine-order bounds and 10,388 native simulations. Prefix sharing reduced
its recorded frame work from a separate-order reference of 366.53 million frames to 78.08 million executed
frames, yet it still timed out. LUCK-only optimizations do not resolve this deterministic bottleneck.
These diagnostics describe work inside an unfinished request; they do not estimate the remaining work
needed to prove the full legal domain.
