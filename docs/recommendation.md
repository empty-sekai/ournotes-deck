# Account recommendation

`engine::recommend_account(data, account_json, request_json, progress)` shares the existing search engine.
The request is `ournotes-deck.recommendation-request/2`; the answer is `ournotes-deck.account-recommendation/1`.
See [account input](account-input.md) for the account envelope.

```json
{"format":"ournotes-deck.recommendation-request/2","goal":{"kind":"freeLive","musicId":10,"difficulty":"expert","accuracy":{"greatFraction":0}},"metric":{"kind":"score"},"k":5,"limits":{"timeLimitMs":null}}
```

The goal grammar includes `power`, `skip`, `freeLive`, `missionLive`, `battleLive`, `arenaLive`, and `challengeLive`.
`capabilities()` reports which goal/metric pairs this build actually computes, plus the unsupported pairs.
A supported pair can still reject an unsupported skill or lifecycle, and support is not a latency guarantee.
Battle/Arena use the declared room policy and native rank-1 confirmation on range completion. Custom ranks remain
unsupported. LUCK uses certified score laws over the native lottery probabilities; a rank is proved only by separated
bounds or a verified equal-program certificate. An overlapping frontier remains `RefinementRequired` and unproven.

Live values average all 120 member performance orders. Snaps stay paired with their members. Nonleader layout is
canonical, with the leader in slot 2. Five fixed member/Snap pairs therefore give at most five teams, one per leader;
the 120 performance orders never become additional recommended teams. Scalar results preserve the search's order:
expected payoff, power, then its canonical key. Challenge-point reward priorities use the lexicographic order below. It does not reorder a truncated Top-K under a different secondary objective.

`metric:{"kind":"challengePoints","eventId":7,"consumption":1}` maximizes newly earned Challenge points from
an ordinary played or skipped Live. The lower-level search metric is `clientChallengePoints`. It uses the
`MasterLiveChallengePoint` row of each outcome's score rank times the consumed Live Boost's event-point rate;
member/Snap event-point bonuses do not multiply Challenge points. As with `eventPoints`, supply a result clock,
event windows and local-event context, or `eventContext.rewardProjection:true` for a single-result increment.
Explicit local records and reward projection are mutually exclusive. Projection runs private synthetic counters:
the native per-result EP amount and CP increment do not depend on previous balances, even when Int32 counters wrap.
It does not validate resource affordability, predict terminal balances, or include cumulative achievement/loop
rewards. The answer echoes `metric.rewardProjection:true`. A played point result with explicit context requires
the event's local record. Existing balances are excluded from the payoff. A challenge Live spends these points and cannot select the earnings metric.

## Challenge-point reward priorities

An optional `secondaryPriority` on `challengePoints` selects a lexicographic objective:

- `eventPointsFirst`: expected CP, expected event points, expected event items, power, canonical IDs.
- `eventItemsFirst`: expected CP, expected event items, expected event points, power, canonical IDs.

```json
{"kind":"challengePoints","eventId":7,"consumption":1,"secondaryPriority":"eventPointsFirst","resourceType":4,"resourceId":88}
```

Supply the event-item resource and event context as for an item projection. The lower-level metric is
`clientChallengePointsWithBonuses`, with `priority`, `eventId`, `resourceType` and `resourceId`.
Each terminal outcome settles all three rewards under the same scenario. Played expectations weight all
120 member orders equally; deterministic Skip has denominator 1. Member and Snap event bonuses contribute
to their respective PT and item quantities. CP retains its ordinary earnings formula.

The search returns at most K teams from the highest expected-CP layer it has found. A result can therefore
contain fewer than K teams. Any positive CP advantage takes precedence over both secondary quantities;
among exact CP ties, the requested secondary order precedes power and canonical identity. The search
covers the complete legal team domain using bounds for the primary dimension and exact secondary values.

The supported domain is lottery-free played outcomes and deterministic Skip. Active LUCK ranges and
ordinary probability predicates with an unproved outcome distribution return `Unsupported`.
`capabilities().challengePointPriorities` declares both priorities, `objective:lexicographicExpected`,
`primary:challengePoints`, `bestPrimaryOnly:true` and `lotteryFree:true`.

The result echoes the priority and resource identity. Each team's `eventRewards` contains
`challengePoints`, `eventPoints` and `eventItems`, each with `score`, an exact numerator/denominator and
`interval:null`. `value.payoff` remains CP. Complete/proven covers the full lexicographic result;
time-limited answers contain the best observed CP layer and are unproven. Scalar upper bounds describe
CP only. Partial-search best/K-th gaps are omitted because secondary optimality requires its own proof.

## Combined metrics

`metric.kind:"combined"` maximizes the expected weighted sum of several metrics of the goal:

```json
{"kind":"combined","consumption":1,"terms":[
  {"kind":"eventPoints","eventId":7,"weight":2},
  {"kind":"challengePoints","eventId":7,"weight":35}]}
```

Each term is a metric kind with that kind's own fields and an integer `weight` in 1..=1,000,000; a metric takes
1..=8 terms. `metric.consumption` is given once and covers every event term; a combination without event terms
takes neither `consumption` nor `eventContext`. Every term settles on the same terminal result, so the payoff of
one outcome is the sum of `weight × term payoff`, and a team's value is its mean over the 120 member orders
(deterministic Skip has denominator 1). Results keep the scalar order: expected payoff, power, canonical key.
Weights express an exchange rate between rewards: with weights 2 and 35 above, one Challenge point counts as much
as 17.5 event points.

The lower-level metric is `{"kind":"combined","levels":[{"terms":[{"metric":{...},"weight":2}, ...]}]}` with one
level. Played lives search with an upper bound that is the weighted sum of the terms' bounds; Skip covers the
complete legal team domain. The supported domain is lottery-free: active LUCK ranges return `Unsupported`.
`capabilities().combinedMetric` declares `objective:weightedExpectedSum`, the term and weight limits and
`lotteryFree:true`.

The result echoes `metric.terms`. `value.payoff` is the weighted sum, and each team's `terms` lists the exact
expectation of every term's own payoff, unweighted and in request order, each with `score`, an exact
numerator/denominator and `interval:null`. The entry of a `scoreAndLife` term is null.

## Event-item rewards

`metric:{"kind":"eventItems","eventId":7,"resourceType":4,"resourceId":88,"consumption":1}` maximizes
items of the requested resource. The lower-level metric is `rankedEventItems` (`Metric::RankedEventItems`).
Use `eventContext.rewardProjection:true` with the declared result clock and event windows; this projection
requires neither account balances nor selected reward IDs. An event outside its held result-time window has
zero payoff.

Each terminal score resolves its result grade under the selected solo, room or Skip model. Skip uses the configured
fixed result grade. The event's `liveEventRewardGroup` or `challengeLiveEventRewardGroup` selects rows by
`eventGroup` in the corresponding reward table. The supported deterministic domain requires exactly one row for
the exact grade, with `probability` marker 10000. This check applies to the entire grade before filtering by the
requested resource. Missing grades, multiple rows and other probability markers are explicit unsupported inputs.

Only that grade's reward is counted. A different resource gives zero for the requested metric; rewards from lower
grades are not accumulated. The quantity uses member and Snap `EVENT_ITEM` effects and the route's item multiplier
(the first boost-rate component): `resourceCount * (10000 + bonus) * itemRate / 10000`, with native Int32 operations
and truncation. Each terminal outcome is settled before averaging; the mean score is not used to select a grade.

`capabilities().eventItemRewards` declares `selection:exactResultGrade`, `eventGroupField:eventGroup`,
`rowsPerGrade:1`, and `probabilityMarker:10000`. Clients can require this contract before submitting an item
projection. Counter increments, affordability and cumulative achievement rewards remain separate quantities.

## Context and output

The event song-ranking choice uses `goal:{"kind":"challengeLive","challengeMusicId":...,"difficulty":"expert"}`
with `metric:{"kind":"score"}` and the applicable held `eventIds`. `capabilities().eventMusicRanking` describes this
mapping. The verified client stores per-difficulty high scores and the maximum SoloScore for each challenge song;
the challenge-song ranking query is keyed by challengeMusicId, without difficulty. The recommendation maximizes
expected score on the selected song and difficulty. It does not predict a server rank, combine different songs,
or treat event-point accumulation as a song-ranking score.
This mapping describes the verified challenge-song route. It does not infer the semantics of newer master-data
`isMusicRankingDisabled` / `isTotalMusicRankingDisabled` fields absent from the verified native MasterEvent schema.
Accuracy is a deterministic declared play: Greats are spread evenly over judged notes, then the Just fraction is
applied to remaining Just-eligible notes. It is not a distribution of human errors. Gekisou-off goals require Just 0.

Event requests accept `eventContext.resultClock` with `kind: played|skip`, `serverNowJstTicks`, and nullable
`liveStartJstTicks` for a played live. Ticks remain 64-bit integers in the raw JSON text.
`room` supplies the Battle/Arena score aggregation policy.
Only Challenge and power with `eventParameter:true` read event IDs for power bonuses.

Answers use status `ok`, `incomplete`, `invalid`, or `failed`; input issues never masquerade as an empty successful
search. An `ok` result has `optimality.proven` only when the underlying search proves its canonical Top-K.
Timeout and unresolved-overlap results keep `proven:false`. `value.exact` is a true expectation fraction or null;
`value.interval` contains certified lower/upper endpoints as exact binary rational fractions. Its integer `score`
is a downward-rounded display bound when an exact expectation is unavailable. Non-score objectives use the same
contract under `value.payoff`. `rankCertified` reports whether that team's displayed rank has been established.
Final deterministic played-live teams include 120 scores and their order statistics. Lottery intervals and progress
omit unavailable exact order scores. A certified rank does not claim that its expectation is known as an exact fraction.

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
