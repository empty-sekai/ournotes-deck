# Recommendation coverage and evidence

The production entry point is `ournotes-recommend`; the request, CLI and Rust API are documented in [recommendation-contract.md](recommendation-contract.md). Results describe an explicit finite root law and play/context model. Exhaustive search proves a conditional optimum only when the legal physical domain is exhausted or safely bounded. Candidate search and budget exits keep their heuristic/unproven labels.

## Mode, action and objective matrix

Played objectives are expected score, probability of reaching a score threshold, client event points and explicitly conditional client reward quantity. Skip uses the same four terminal payoffs with deterministic score; power uses `power`.

| Mode | Power | Skip | Played Live | Required assumptions and current scope |
|---|---|---|---|---|
| Free | Exact checked search | Four objectives | Ordinary and Gekisou, four objectives each | Explicit play and finite root law; Gekisou Solo rank1 |
| Challenge | Special song view/event parameter | Four objectives | Ordinary and Gekisou, four objectives each | Resolved base music and distinct literal-zero mission branch; missing master rows are errors |
| Mission | Exact checked search | Rejected; no verified client action | Gekisou forced on, four objectives | Native Solo updater/rank1; declared finite law |
| Battle | Exact checked search | Rejected | Gekisou forced on, four objectives | Complete aggregate ranking packet arrival timeline; native eligibility and one settlement per frame |
| Arena | Exact checked search | Rejected | Gekisou forced on, four objectives | Special master view and ranking timeline; current JP lacks Arena rows, so its full branch is tested with synthetic data |
| Tutorial | No recommendation contract | No recommendation contract | No recommendation contract | Not implemented and not counted as supported |

The five formal modes have **41** legal mode/action/objective branches:5power +2skip*4 +7played*4. Challenge's literal-zero case adds13 synthetic branches, for54 executable model branches and8 invalid/Unsupported guards. This is a coverage denominator, not54 complete original-game lifecycles. Songless power is also available.

## Cards, play, constraints and random variables

- Choose five cards with distinct characters. Physical slot2 is the leader. Each snap is absent or paired to one member and can be equipped only once. All five member slots shuffle, and the paired snap follows its member. Physical permutations are included in played-score search.
- Supported constraints:`leader/includeMembers/excludeMembers/excludeSnaps/noSnaps`. Unknown keys, conflicting constraints, invalid owned IDs and incomplete played inputs are rejected. Required snaps, fixed all-slot layouts, upgrade budgets and disjoint multi-deck optimization have no current contract.
- Inputs preserve the caller's DeckData provenance and Roster progression/bonuses. Existing omitted Roster fields use model defaults; recognition does not prove complete ownership or unknown progression. Easy/Normal/Hard/Expert are selected through valid score IDs belonging to the resolved song.
- A full judged stream must score every chart note exactly once, including explicit Misses, reach every event and settle all Gekisou ranges. `theoreticalBest` is a caller-selected AP/Just condition. Raw FT-result/touch/window/scheduler recommendation bridges are not wired; existing native mechanism proofs are retained.
- Explicit Gekisou frame `deltaTimes` use direct JSON-number-to-f32 parsing;0.016666668 is legal, rational/private-number objects, negative/non-finite values and wrong lengths are rejected. Ordinary Live uses `musicTimeMs` and does not consume deltaTimes.
- Each signed root drives the same MemberShuffle, Skill and Luck random state. Duplicate atoms and integer weights are preserved, exact fractions are decimal strings, overflow is an error, and best skill order is not an executable player action. The actual TickCount population law is unknown.
- Ranking input is an immutable conditional aggregate arrival model. Batch arrivals are allowed; settlement is ordered by range and limited to one eligible Finish8 range per frame. `networkApplications` distinguishes arrival from application. Causal peer-score ranking, partial packets and disconnects are not modeled.
- Event payoffs are calculated per atom before weighting. Frozen power/result clocks, event windows, counters/consumption, selected rewards and multiplayer result panel are explicit inputs. Client previews do not prove server reward selection or actual awards. Full failure/continue/quit/server lifecycle, unlock eligibility and other regional native/IFix equivalence are not certified.

## Search and termination

Power and skip-score reuse the existing checked exact canonical member-set solver. Other payoffs stream legal physical decks into a bounded TopK, reuse resolved chart/play inputs, retain bounded FIFO atom cache and safely prune feasibility or strict conservative payoff bounds. Partial candidates are discarded. Candidate/time limits and cancellation are distinct; candidate search never reports Complete for merely exhausting its proposals. The score upper bound is conservative, and bounded large-pool results do not prove a global optimum.

## Validation scope

The numerically identical R3 source previously passed fmt, all-targets Clippy with warnings denied,6recommendation+8scenario Rust tests,19wholeCLI explicit-input cases and54synthetic branches+8guards. Independent R3 acceptance covered14constant-clock,6nonconstant-clock/oracle/invalid-input,24search-boundary,15network,45current-JP-master scene,8synthetic-Arena and3bounded-large-pool checks. The independent oracle enumerates and sorts separately but reuses the public point model; it is not an original ARM64 variable-clock proof. Public CI runs the repository's normal tests; external original game artifacts are not committed.

Prior exact real-master/chart physical-domain oracle checks covered26040 and48960 decks; their R2 version identity remains distinct from the R3 parser fix. Prior x64/WASM point-model consistency covered8scenes*7roots and is distinct from original-game proof. These long checks were not rerun solely for JSON parsing.

Existing offline original ARM64 evidence covers late and early aggregate queues (two5500frame captures;92independent terminal/application fields). A separate paired real-card common-root chain executed original constructors, two7500frame captures and506414baseline fields with zero differences,603RNG calls per capture=4shuffle+9SkillValue+590Luck; reseed/split-root/mispair controls distinguish the tested mechanism. R3 independently accepted two unchanged explicit7500numeric-step requests and matched18winner fields against reused native evidence, adding0nativeframes. This proves those declared inputs/root, not all cards/roots or real population law.

R3 independent bounded single samples:realJP63members/64snaps candidate3.1157s/19.58MiB/191complete candidates, exhaustive1.5750s/19.38MiB/178complete; synthetic80/60 candidate2.0549s/5.85MiB/1154complete. Each returned TimedOut with heuristic or unproven optimality, one excluded partial candidate and boundedcache64/Top5. Single samples have no statistical p50/p95 claim. Prior80sample bounded measurements retain their earlier identity; cooperative deadlines are not hard real-time guarantees.
