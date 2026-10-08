# Account recommendation

`engine::recommend_account(data, account_json, request_json, progress)` shares the existing search engine.
The request is `ournotes-deck.recommendation-request/2`; the answer is `ournotes-deck.account-recommendation/1`.
See [account input](account-input.md) for the account envelope.

```json
{"format":"ournotes-deck.recommendation-request/2","goal":{"kind":"freeLive","musicId":10,"difficulty":"expert","accuracy":{"greatFraction":0}},"metric":{"kind":"score"},"k":5,"limits":{"timeLimitMs":null}}
```

The goal grammar includes `power`, `skip`, `freeLive`, `missionLive`, `battleLive`, `arenaLive`, and `challengeLive`.
The optional top-level `aggregation` is `expected` (the default) or `maximum`. It changes the ranking objective
while preserving the card pool, song, accuracy or complete play, room, event context and constraints. `expected`
values each team's mean payoff under the declared order and lottery model; `maximum` values its highest payoff
in the supported random-outcome domain described below. For deterministic power and skip, both aggregations give
the same value.
Maximum is available for Free Live, Challenge Live, Skip and Power. Gekisou lives (Mission Live, Battle Live and
Arena Live) support Expected aggregation; Maximum requests for these modes return an unsupported error.
For example, adding `"aggregation":"maximum"` to the request above searches for the highest score in that support
domain under its declared accuracy. It does not change that accuracy to a perfect play. Unknown aggregation names are input errors.

`capabilities()` reports which goal/metric pairs this build actually computes, plus the unsupported pairs.
`defaultAggregation` is `expected`; `aggregations.expected` and `aggregations.maximum` map each supported goal to
its available metric names. `aggregationTieBreak` reports the ranking key for each aggregation.
`maximumModels` maps each supported Maximum goal to its model; `maximumModel` describes the played-live model.
A supported pair can still reject an unsupported skill or lifecycle, and support is not a latency guarantee.
Battle/Arena use the declared room policy and native rank-1 confirmation on range completion. Custom ranks remain
unsupported. LUCK uses certified score laws over the native lottery probabilities; a rank is proved only by separated
bounds or a verified equal-program certificate. An overlapping frontier remains `RefinementRequired` and unproven.

Expected Live values average all 120 member performance orders. Non-Gekisou Maximum Live values optimize across
those orders and the supported outcomes of ordinary random skill checks, treating successive random choices
independently. A native binary32 probability draw can equal 1, so a threshold of 1 retains both trigger and
non-trigger outcomes; thresholds above 1 always trigger, and thresholds at or below 0 never trigger.
This support search does not enumerate integer PRNG base seeds or certify that one base seed realizes every
choice in an optimized outcome. `Complete` proves the optimum in this declared independent-support model;
`team.bestOrder` is not a finite-seed replay certificate. The stream's `baseSeed` is not a constraint on this
support search: member order and successive skill choices are optimized under the declared model. The chart,
judgement stream or declared accuracy,
frame sequence and event conditions remain fixed. Snaps stay paired with their members. Nonleader layout is
canonical, with the leader in slot 2. Five fixed member/Snap pairs therefore give at most five teams, one per leader;
the 120 performance orders never become additional recommended teams. The result preserves the search's order: selected payoff, power, then its
canonical key. It does not reorder a truncated Top-K under a different secondary objective.

Maximum results and progress include `result.maximumModel`. Free Live and Challenge Live use:

```json
{
  "kind": "independentNativeDrawSupport",
  "performanceOrders": 120,
  "ordinarySkillDraw": "binary32OfSystemRandomNextDouble",
  "ordinarySkillComparison": "strictLessThan",
  "streamBaseSeedRole": "notAConstraint",
  "rootSeedRealizability": "notEstablished",
  "bestOrderCertificate": "performanceOrderAndTerminalValuesOnly"
}
```

Power and Skip use `kind:"deterministic"`, `performanceOrders:1`, and `"notApplicable"` for both
`rootSeedRealizability` and `bestOrderCertificate`. Expected results omit `maximumModel`. The typed search result,
fixed-deck evaluation and deterministic search-session progress expose the same model contract for Maximum.
Completion and ranking fields remain separate: `optimality.proven:true` certifies the declared domain, while
`rootSeedRealizability` records the seed boundary even in a completed result. The
[support-domain argument](search.md#maximum-support-and-seed-realizability) explains the relation to seeded values.

`metric:{"kind":"challengePoints","eventId":7,"consumption":1}` maximizes newly earned Challenge points from
an ordinary played or skipped Live. The lower-level search metric is `clientChallengePoints`. It uses the
`MasterLiveChallengePoint` row of each outcome's score rank times the consumed Live Boost's event-point rate;
member/Snap event-point bonuses do not multiply Challenge points. As with `eventPoints`, supply a result clock,
event windows and local-event context, or `eventContext.rewardProjection:true` for a single-result increment.
Explicit local records and reward projection are mutually exclusive. Projection runs private synthetic counters:
the native per-result EP amount and CP increment do not depend on previous balances, even when Int32 counters wrap.
It does not validate resource affordability, predict terminal balances, or include cumulative achievement/loop
rewards. The answer echoes `metric.rewardProjection:true`. Conditional items still need explicit server-selected
rewards and local context. A played result with explicit context requires the event's local record. Existing balances are
excluded from the payoff. A challenge Live spends these points and cannot select the earnings metric.

The event song-ranking choice uses `goal:{"kind":"challengeLive","challengeMusicId":...,"difficulty":"expert"}`
with `metric:{"kind":"score"}` and the applicable held `eventIds`. `capabilities().eventMusicRanking` describes this
mapping. The verified client stores per-difficulty high scores and the maximum SoloScore for each challenge song;
the challenge-song ranking query is keyed by challengeMusicId, without difficulty. The recommendation maximizes
the selected aggregation of score on the selected song and difficulty. It does not predict a server rank, combine different songs,
or treat event-point accumulation as a song-ranking score.
This mapping describes the verified challenge-song route. It does not infer the semantics of newer master-data
`isMusicRankingDisabled` / `isTotalMusicRankingDisabled` fields absent from the verified native MasterEvent schema.
Accuracy is a deterministic declared play: Greats are spread evenly over judged notes, then the Just fraction is
applied to remaining Just-eligible notes. It is not a distribution of human errors. Gekisou-off goals require Just 0.

Event requests accept `eventContext.resultClock` with `kind: played|skip`, `serverNowJstTicks`, and nullable
`liveStartJstTicks` for a played live. Ticks remain 64-bit integers in the raw JSON text. Conditional event-item
metrics still require explicit selected server rewards. `room` supplies the Battle/Arena score aggregation policy.
Only Challenge and power with `eventParameter:true` read event IDs for power bonuses.

Answers use status `ok`, `incomplete`, `invalid`, or `failed`; input issues never masquerade as an empty successful
search. An `ok` result has `optimality.proven` only when the underlying search proves its canonical Top-K.
Timeout and unresolved-overlap results keep `proven:false`. Progress and final results echo `result.aggregation`.
Final results also include `result.exitReason`, distinguishing exhaustion, time or candidate limits, and required refinement; progress omits it.
For `expected`, `value.exact` is a true expectation fraction or null;
`value.interval` contains certified lower/upper endpoints as exact binary rational fractions. Its integer `score`
is a downward-rounded display bound when an exact expectation is unavailable. Non-score objectives use the same
contract under `value.payoff`. `rankCertified` reports whether that team's displayed rank has been established.
Final deterministic played-live teams include 120 scores and their order statistics. Lottery intervals and progress
omit unavailable exact order scores. A certified rank does not claim that its expectation is known as an exact fraction.

For `maximum`, `value.score` is the maximum score in the declared support domain and `value.exact` is its exact
fraction with denominator 1. A non-score metric's maximum is in `value.payoff`. The maximum score and maximum payoff are optimized separately
within the team and need not occur in the same outcome. `team.bestOrder` contains `order` (member card IDs), `score`
and nullable `payoff` for an outcome maximizing the selected metric, then score. `orders` is null in maximum mode:
the peak objective does not report mean, median or probability statistics. A maximum for a threshold metric is 1
when at least one supported outcome satisfies it, and 0 otherwise; it is attainability in the declared model, not a probability.

Each progress callback is a complete answer with `final:false`. `recommend` returns `final:true` and is synchronous;
run it in a Worker and terminate that Worker to cancel. Progress and final answers include the account scope but no
ignored identity fields. Telemetry reports WebAssembly linear memory; `memoryBudgetBytes:null` means no configured
budget is being claimed.

Validation includes synthetic account parsing, structured error/privacy checks, actual account-to-search results,
and native/WASM transport comparison. Generate the transport corpus with:

```sh
OURNOTES_ACCOUNT_CORPUS=$PWD/target/account-corpus cargo test --release --locked -p ournotes-search --test adapter_fixture_export export_account_transport_corpus -- --ignored --exact
node tools/search-harness/account-wasm.cjs target/recommend-node/ournotes_recommend_wasm.js target/account-corpus
```

The Node comparison checks dataset identity, byte and legacy-text construction, invalid UTF-8, full progress schema,
structured missing facts, and native/WASM results. Timing and platform memory telemetry are excluded from equality.
