# Replay rank analysis

`ReplaySession::start_rank_analysis` evaluates one declared complete judged-stream
play with all 120 permutations of five skill-event positions, equally weighted.
The physical performers and their paired support skills stay fixed. Only the
mapping from chart skill-event positions to performer indexes changes.

The analysis fixes the input judgements, frame clock, accuracy plan and ranking
inputs. Its hit fraction describes this declared order model; it does not model
human error or an unspecified distribution of random seeds.

## Request

The JSON entry point is `ReplaySession::start_rank_analysis_json`. The WASM
binding exposes the same operation as `session.startRankAnalysis(json)`.

```json
{
  "format": "ournotes.replay-rank/1",
  "replay": {},
  "target": { "kind": "score", "threshold": 1000000 },
  "powerDomain": { "min": 1, "max": 20000000 }
}
```

`replay` is a complete `ournotes.replay/1` request with exactly five performers.
It can be constructed with `ReplaySession::template` and then supplied with the
caller's performer and play inputs. Analysis owns `skillOrder` and disables
per-frame tracing. The current power must be in `1..=20000000`.

The score threshold is an explicit nonnegative integer. The caller obtains it
from the same version of chart data and the declared result scope. A solo score
threshold and a room-total score threshold describe different targets.

`powerDomain` defaults to `1..=20000000`. It defines the integer interval used
for required-power inversion; current-power statistics use `replay.power`,
which need not be inside this interval.

## Incremental execution

The returned `RankAnalysisJob` supports:

- `status()` / `status_json()`: inspect the current progress;
- `advance(max_orders)` / `advance_json(max_orders)`: run at most 1 to 120
  additional complete orders;
- dropping the job: cancel and release its programs.

WASM uses `status()`, `advance(maxOrders)` and `free()`. The first two return JSON.
A Worker can advance a small batch and yield before the next call. One complete
order is atomic. Final aggregation and required-power inversion run after the
last order. Parsed session data is shared with the job and remains valid after
the caller releases its session handle. Programs are released at completion or
an unsupported terminal state.

Every progress object has format `ournotes.replay-rank-result/1`,
`completedOrders`, `totalOrders: 120`, and one of these statuses:

| Status | Meaning |
| --- | --- |
| `running` | A partial prefix is complete; `result` is null. |
| `complete` | All 120 orders are complete; `result` contains the statistics. |
| `unsupported` | The declared execution is outside the supported domain; `code` and `reason` explain why, and `result` is null. |

A valid `advance` after a terminal state returns the same state. Invalid request
inputs or execution failures return errors rather than successful statistics.

## Result

A complete result includes:

- `scoreId`, `power`, `threshold` and `powerDomain`;
- `orderModel: "uniformSkillOrder120"`;
- `orderScores`: 120 integer scores in lexicographic permutation order;
- `scoreSum`, `orderCount: 120`, `minScore` and `maxScore`;
- `targetHitCount`: the number of scores at least the threshold;
- `need`: the independently certified required-power result.

Equal scores and equal skill values retain their full permutation mass.
Expected score is `scoreSum / orderCount`, and the hit fraction is
`targetHitCount / orderCount`. Zero hits is a complete, valid result.

For positive declared cycle duration `cycleMs`, the expected number of qualifying
plays per hour is `targetHitCount / orderCount * 3600000 / cycleMs`.

## Required power

Let `Sπ(P)` be the exact terminal score for skill order π at integer power P and
let T be the threshold. Required power is the first integer in `powerDomain`
that satisfies:

```text
sum(Sπ(P), over all 120 orders) >= 120 * T
```

This is a threshold on expected score. It is not a guarantee that every order
qualifies, and it is not the mean of the orders' individual required powers.

Each order records an exact [power-parameterized score program](score-programs.md).
Every program must have a nondecreasing certificate over the entire interval
before an integer lower-bound search is performed. Evaluation retains the native
binary32 operations, integer truncation and wrapping arithmetic.

`need` is one of:

- `{"status":"exact","power":P,"scoreSum":N,"previousScoreSum":M}`:
  N reaches `120*T`; M is below it. M is null when P is the domain minimum.
- `{"status":"outsideDomain"}`: certified scores cannot reach the target in
  this interval.
- `{"status":"unproven","reason":"..."}`: no interval-wide monotonicity
  certificate is available. Current-power order statistics remain complete.

Score sums are integers with magnitude at most `120 * 2^31`, so JSON numbers
represent them exactly.

## Supported execution domain

The judged-stream analysis checks the actual constructed effects and their
trigger, condition, reset, release and cumulative inputs. Supported schedules
are independent of the root random seed. Active LUCK ranges, raw-result callbacks
and unproved random schedules return `unsupported`.

Free Live uses its actual execution mode: static LUCK mission metadata alone
does not create an active LUCK range. Solo and declared fixed-rank Gekisou use
the replay entry point's normal settlement path.

## Synthetic correctness checks

The replay integration tests compare every order against `ReplaySession::run`,
retain physical support-skill pairings, exhaustively check required power on a
small integer interval, and cover fixed-rank settlement and unsupported domains.

```sh
cargo test -p ournotes-sim --test replay --locked
```

The following exports synthetic native references and checks the actual Node
WASM bindings, including incremental batches, cancellation and session ownership:

```sh
mkdir -p work/replay-rank
OURNOTES_REPLAY_RANK_CORPUS=work/replay-rank/corpus.json \
  cargo test -p ournotes-sim --test replay --locked \
  rank_analysis::export_rank_analysis_corpus -- --ignored --exact
cargo build --release --locked --target wasm32-unknown-unknown \
  --manifest-path wasm/replay/Cargo.toml
wasm-bindgen --target nodejs --out-dir work/replay-rank/nodejs \
  wasm/replay/target/wasm32-unknown-unknown/release/ournotes_replay_wasm.wasm
node wasm/replay/tests/replay-rank.cjs \
  work/replay-rank/nodejs/ournotes_replay_wasm.js work/replay-rank/corpus.json
```

Use the `wasm-bindgen-cli` version pinned by `wasm/replay/Cargo.toml`.
