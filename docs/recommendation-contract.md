# Recommendation contract (v1, owner: solver)

Integration entrypoint is the separate binary `ournotes-recommend`:

```text
ournotes-recommend --data DECK_DATA.json --roster ROSTER.json --request REQUEST.json [-o RESULT.json]
```

Core Rust entrypoint: `search::recommendation::recommend(&DeckData, &Roster, &RecommendationRequest)`.
No OCR, BoxLens, frontend or adapter files are owned by this branch.

Inputs reuse `cards::Roster` (camelCase player/members/snaps, IDs must be master IDs) and `data::DeckData` (`nnnotes.deck-data/1`). Preserve player progression, card level/exp, awake/rank, ordinary/gekisou skill levels, snap rank/level and player bonuses. Omitted existing Roster fields use the crate's existing defaults; adapters must surface uncertainty instead of treating recognition as verified ownership.

Request envelope and enum tags are stable:

```json
{
  "format": "ournotes-deck.recommendation-request/1",
  "execution": {"kind":"live","scoreId":1004,"gekisou":false,"play":{"kind":"theoreticalBest"}},
  "scenario": {"kind":"free","musicId":100},
  "metric": {"kind":"score"},
  "seedLaw": {"atoms":[[1,1],[-1,3],[42,1]],"provenance":"explicit illustrative finite law, real TickCount distribution unknown"},
  "constraints": {"noSnaps":false},
  "k": 5,
  "strategy": {"kind":"exhaustive"},
  "limits": {"timeLimitMs":3000,"maxCandidates":1000,"cacheEntries":2048}
}
```

Execution `power` accepts optional `musicId` and `eventParameter`; `skip` requires `scoreId`; `live` requires `scoreId`, `gekisou` and explicit `play`. Play is either `theoreticalBest` (an explicitly selected AP/Just scenario, never predicted real play), or `stream` with `stream` in the existing `JudgementStream` schema. Native member order is random over all five physical slots, with paired snaps; the root law is authoritative for Skill and Luck as well as MemberShuffle. Signed roots and duplicate atoms are preserved.

For Gekisou execution, an explicit stream may supply `deltaTimes`, one JSON number in seconds per frame, for example `[0.016666668,0.01,0.02]`. Ordinary Live uses the declared `musicTimeMs` clock and does not consume `deltaTimes`. Numbers use the existing direct f32 parsing and rounding; integer zero is allowed, negative or non-finite steps are rejected, and the array length must equal `frames.length`. Rational objects such as `{"numerator":1,"denominator":60}`, numeric strings, null elements and non-standard NaN/Infinity tokens are invalid. Omitting the whole field selects the documented constant 60FPS default; this cannot substitute for a requested explicit or variable time step. Duplicate fields and unused fields of an execution/play variant remain errors. The R3 parser uses typed wire structures to avoid the R2 internally-tagged/arbitrary-precision decimal buffering defect; game evaluation and search are unchanged. K is bounded to 1..100; visited/evaluated candidate counts can exceed K.

Scenario is `free|mission|battle|arena|challenge` with `musicId`; Arena/Challenge use IDs from their special master tables. Live and skip require a scenario. Songless power may omit it. Optional `context` uses the existing `scenario::ContextInput` schema: frozen `powerSnapshot`, separate `resultClock`, `eventPayoff` with explicit counters/windows and multiplayer result-panel adapter. Event clock execution must match skip/played.

Metrics: `power`, `score`, `scoreAtLeast` with `threshold`, `clientEventPoints` with `eventId`, or `conditionalClientEventItems` with `eventId/resourceType/resourceId`. Played metrics optimize exact expectation of the terminal per-atom payoff, using integer masses. Threshold metric is a probability under that law. Event quantity is client preview, never server reward authority. Conditional items require explicitly selected rewards; peer totals/confirmations are supplied inputs.

Native Mission/Battle/Arena force Gekisou on; the boundary rejects false. Mission uses the native Solo updater and group ordinal 1. Battle/Arena require `networkConfirmations:[{"frame":180,"range":0,"rank":2,"percent":160},...]`: exactly one entry for every fever, a valid arrival frame/group ordinal 1..5, and the percent from the selected master's mission-pattern ranking table. `frame` means the frame when the complete aggregate peer result is AVAILABLE, not when the bonus is applied. Batch arrivals are legal. The production scheduler consumes at most one eligible range per frame, in ascending range order, after it reaches native Finish8. The result atom's `networkApplications:[[range,frame],...]` records actual settlement; state Complete7 before the update reaches Finish8 during that update. Ranking uses group ordinal (not UI competition rank). An immutable supplied rank timeline is a conditional model, not a prediction of peer placement or a causal peer-score model; callers must state that assumption. The evaluator retains the COMPLETE post-shuffle random state and rejects incomplete range settlement. Optional `simulation:{"musicLengthMs":123000,"scoreMusicLengthMs":122000,"liveFinishedFromFrame":7320}` overrides declared audio/score durations and lifecycle flag. Without these fields, chart-derived lengths and an unasserted live-finished flag apply and remain visible in the model scope. A full play must judge every scoring chart note exactly once (including explicit Misses), reach all chart events, and settle every Gekisou range. Raw touch inference/window-runtime optimization, the native TickCount population law, unlock eligibility, regional-binary equivalence and server-selected rewards remain outside this verified recommendation model. Existing ARM64 proofs of shuffle, shared Skill/Luck streams, frozen Gekisou snapshots and native ranking are retained; those mechanisms are not treated as unknown.

`strategy:{"kind":"candidate","powerSeeds":8,"proposals":1000,"proposalSeed":1}` requests explicitly heuristic proposals, exactly scored under the law. Exhaustive mode enumerates every legal physical deck with safe feasibility pruning and bounded streaming Top-K; candidate mode returns `optimality=heuristic`, never Complete. Limits are cooperative; a current atomic simulation can overrun wall time. Partial candidates never enter Top-K. Candidate cap and time expiry remain distinct exit reasons. Results include exact fractions as **decimal strings** for JavaScript safety, physical member/snap IDs, objective/probability/model scope, completion/optimality, evaluation counters and elapsed time. Existing power/skip exact searches keep their canonical member-set identity; native/event searches rank physical decks.

Errors are structured (`Input`, `Master`, `Game`, `Unsupported`, `Domain`, `Capacity`), exit code 2, with a specific message. Unknown constraint keys fail at the shared `Constraints` deserializer in both legacy and new JSON boundaries; they cannot silently change the feasible set. Error and timeout are different states. Default limits are bounded; a fully unbounded run must be explicit. The CLI writes one completed response only; externally killing it yields no successful response, and a caller must treat that as cancellation. Frame-level deadline checks discard unfinished candidates. `maxCandidates` applies to physical enumeration/proposals; canonical power/skip searches use their existing checked branch search and the wall-time budget, as stated in the response proof scope.

