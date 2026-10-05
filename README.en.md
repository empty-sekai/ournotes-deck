# ournotes-deck

Deck power, skip score and live score for BanG Dream! Our Notes, and an exact Top-K deck search.

[中文](README.md)

## Repository layout

The repository is a Cargo workspace of two crates; the search depends on the model, the model never on the search:

| crate | contents |
| --- | --- |
| `crates/ournotes-sim` | the model: deck power, skip score, live score and the whole-live simulation (`live`), event points (`event`), live modes (`scenario`), the card pool a roster resolves to (`pool::{Pool, Deck}`), the deck data file (`data`), chart statistics (`chartstats`) and single-live replay (`replay`) |
| `crates/ournotes-search` | the exact Top-K deck search and the recommendation pipeline (the modules below), and the `ournotes-deck` and `ournotes-recommend` command-line programs |

Depend on `ournotes-sim` for computation or simulation alone (chart statistics, replay); depend on `ournotes-search`
to search, and take the model's types from `ournotes-sim`. `wasm/replay` is the replay WASM over `ournotes-sim`,
`wasm/recommend` the recommendation WASM over `ournotes-search`, and `tools/search-harness` measures the search; these
three are separate packages outside the workspace, each with its own `Cargo.lock`. Both crates have a
`search-diagnostics` feature; `ournotes-search`'s also enables `ournotes-sim`'s.

## Recommendation pipeline

Construction and search are separate stages:

`DeckData + Roster + RecommendationRequest -> handler::build_card_pool -> BuiltProblem -> search::recommend_built`

`engine` owns typed/JSON facades, `types` owns request/result contracts, `handler` resolves and validates
the scenario and objective, `domain` owns the legal candidate domain, and `search` dispatches solvers.
`auxiliary::{evaluate_fixed, evaluate_built, rank_fixed_songs}` uses the same numerical path.
CLI, WASM and the harness use these modules.

Reuse loaded `DeckData` and immutable `BuiltProblem` values. Each search has fresh budget/frontier/result state.
The one-shot deadline includes construction; execution on an already built problem starts its deadline at the call.
Outcomes carry `telemetry` (`ournotes-deck.telemetry/1`, see [telemetry](docs/telemetry.en.md)) with problem construction, bound compilation and search as separate phases; reused problems have no construction phase.

The facade supports Snap-inclusive Free Live, conditional Mission Gekisou, score and client event points.
Power/Skip retain canonical member-set identity. Played Live/PT rank teams (a leader, four other members and the Snap
paired with each) by their mean payoff over the 120 equally likely performance orders; the positions of the four
non-leader members are a layout, not a decision, and each team is reported in a canonical layout with its score
distribution over the orders and its best order. A skill probability check with Gekisou off is rejected as
unsupported. In a Gekisou live without a LUCK range it gates nothing that can run, so each order keeps one exact
score; a LUCK range ranks decks by certified intervals over the native lottery probabilities, and an overlapping
frontier ends `RefinementRequired`.
The default `branchAndBound` decomposes ordinary Live into member compositions and Snap pairings; Gekisou retains
joint member/Snap traversal. Both bound the mean score with position-mean skill gains and bound normal-played PT in a
certified nonwrapping bonus domain. Unsupported bounds fall back to enumeration with
`telemetry.environment.bounds.fallback`. Both completed `branchAndBound` and `exhaustive` certify conditional Top-K; candidate searches remain heuristic.
`telemetry.joint` reports bound checks and prunes by depth. See the [search argument](docs/search.md#uniform-member-order-search).
`telemetry.composition` reports the decomposition stages; the [composition argument](docs/search.md#member-compositions-snap-pairings-and-power-frontiers) covers the Snap search and conditional PT closure.
`telemetry.proof.globalUpperBound` bounds the best value of the whole domain while the search runs.
The default deadline is three seconds with no candidate-count cap. Explicit budgets remain available; only exhausted/proven search returns `Complete`.
The reproducible correctness experiments (bounded exhaustive comparisons, bound audits and cutoff audits) and how to run them are listed under [search validation](docs/search.md#validation).
The lower-level `search` API below has a separate contract: one result per member set, with a chosen performance order, rather than the facade's uniform order target.

## What it computes

- **Deck power**: per-slot terms (card stats from level, awake count and rank; character rank and total rank;
  band items; song type and tag bonuses; the snap's power bonus and type link; the leader skill; VIP; memory; event
  parameters) and the deck total, with the game's integer and binary32 float arithmetic, including its rounding and
  floor conversions.
- **Skip score**: the score of a skipped live for a chart.
- **Live score**: the per-note score, frames, the combo bonus table, factor commands, and the whole live frame by
  frame (`live::full`): judgement conversion, combo, life (recovery, guard, life zero), the score calculator
  (including the rewind a late judgement causes), and the conditions and effects of live skills and snap skills.
  Gekisou (range states, the Gekisou combo and Just counts, the luck lottery, the rank bonus, Gekisou and
  Gekisou snap skills) is modelled as well; the unified entry point searches Mission Gekisou score and PT under
  declared conditions.
- **Event points**: event bonuses, score ranks, boosts and the event-point amount the game client computes. The game
  server decides the awarded amount; this crate reproduces the client's own computation.
- **Search**: the best K decks for deck power (with or without a song, with or without event parameters), for the
  skip score and for the live score, one result per set of five member cards. The live score is the whole-live
  simulation with live skills and snap skills under a judgement stream (Gekisou off by default; with `--gekisou`,
  ranked by the sum of the scores over a seed set), or, with snap skills excluded,
  the score of a per-note play with live skills only. With snap skills the search simulates candidate decks, so it is
  slower, and streams with many missed or late notes can take much longer; a time limit bounds such requests.

## Correctness

**Agreement with the game.** Every calculation follows the game client's behavior function by function. Each unit is
then checked against the client's own implementation: the client's functions run on the same inputs, and the results
are compared bit for bit (floats by their bits, with integer overflow and the paths that throw compared too). The
checks cover:
- the deck-power primitives and the slot calculation;
- note scores, frames and the combo bonus;
- factor commands and the score calculator's rewind;
- the conditions and effects of live skills and snap skills;
- life, combo and judgement;
- randomness;
- score ranks and event points.

Deliberately broken variants run alongside, to confirm that the comparison does catch differences. The five-slot sum
and the bonus builders are integer code, ported function by function. The whole live is compared with the client
frame by frame; the method and how to reproduce it are in [native validation](docs/native-validation.en.md). The
tests that replay the client's reference vectors build with the `native-fixtures` feature.

**Exact search.** A `Complete` search result is exactly the canonical Top-K over every legal deck. Pruning uses only
bounds that are proven admissible under the game's arithmetic (proofs in [docs/search.md](docs/search.md)). Search
results are compared item by item with an independent exhaustive enumeration, which shares no bound, decomposition
or Top-K code with the search; with snap skills it simulates every member set, leader, snap placement and
performance order. The cases and the commands that run them are under [search validation](docs/search.md#validation).
A search that reaches its time limit returns `TimedOut`, with legal and exactly evaluated decks but no
ranking claim. Inputs outside the proven range, unknown cards, rules the game would reject and parts of the game that
are not modelled are reported as errors.

**Shared runtime.** Per-note calculation, chart statistics and search share one Rust model; the same replay request
gives the same result natively and in WebAssembly, compared as described in
[native validation](docs/native-validation.en.md#native-and-webassembly).

**Applicability.** Results hold for the declared resources, inputs, fields and phases. A per-play result uses
explicit judgements, frame order, seed and ranking policy; statistical baselines and weights do not replace it.

## Data

The crate contains no game data. It reads a deck data file (`nnnotes.deck-data/1`, written by the
`nnnotes deck-data` command) with the master tables and every chart of one master data version, plus the user's
card box:

```json
{
  "player": { "characterRanks": { "1": 20 }, "bandItems": { "101": 10 }, "vipRank": 3, "events": [] },
  "members": [ { "id": 1, "level": 60, "awake": 3, "rank": 2, "liveSkillLevel": 3, "gekisouSkillLevel": 1 } ],
  "snaps": [ { "id": 1, "level": 20, "rank": 1 } ]
}
```

`level` may be replaced by `exp`. Charts are selected by score id (`MasterLiveMusicScore._id`).

The live score with snap skills plays a judgement stream:
`{"frames": [0, 16, 33], "judged": [[frame, noteId, judgement, judgementTimeMs]], "baseSeed": 0, "assist": false}`.
`frames` holds the music time of each frame in ms (non-decreasing); each `judged` row judges a note in frame
`frames[frame]`, with its judgement before conversion (1 Miss, 2 Bad, 3 Good, 4 Great, 5 Perfect, 6 Just); life,
combo and skills follow from the simulation. The default stream is the theoretical best: frames at 60 fps
(`floor(i * 1000 / 60)` ms) until 2000 ms after the last judged note or skill event, and every judged note Perfect,
at its chart time, in the first frame that reaches it (with Gekisou off the game judges no Just).

The live score with snap skills excluded defaults to the theoretical best as a per-note play: every judged note
Perfect, a full combo and no life lost. Another play can be given as
`{"notes": [{"noteId", "timeMs", "noteType", "scoreType", "life", "combo"}], "lifeAtEvent": [...], "assist"}`
(score types: 1 Just, 2 Perfect, 3 Great, 4 Good, 5 Bad, 6 Miss).

## Library use

```toml
[dependencies]
ournotes-sim = { git = "https://github.com/empty-sekai/ournotes-deck" }
ournotes-search = { git = "https://github.com/empty-sekai/ournotes-deck" }
```

```rust
use ournotes_search::search::{Constraints, Objective, SearchRequest, search};
use ournotes_sim::cards::Roster;
use ournotes_sim::data::DeckData;
use ournotes_sim::pool::Pool;

let data = DeckData::from_path("deck-data.json").unwrap();
let roster = Roster::from_json(&std::fs::read_to_string("box.json").unwrap()).unwrap();
let pool = Pool::new(&data.master, &roster).unwrap();
let out = search(&pool, &SearchRequest {
    objective: Objective::SkipScore { score_id: 10000103, chart: data.chart(10000103).unwrap() },
    k: 10,
    constraints: Constraints::default(),
    time_limit: None,
})
.unwrap();
for deck in &out.results {
    println!("{:?} {} {:?} {:?}", deck.score, deck.power, deck.members, deck.snaps);
}
```

## Command line

```sh
ournotes-deck power --data deck-data.json --roster box.json -k 10
ournotes-deck skip  --data deck-data.json --roster box.json --score SCORE_ID -k 10
ournotes-deck live  --data deck-data.json --roster box.json --score SCORE_ID --expectation finite --seed-law law.json [--play stream.json] [--gekisou] -k 10
```

`live` scores with snap skills and ranks by the expected score over the native member orders of the finite root-seed
law in `--seed-law` (JSON `[[rootSeed,positiveWeight],...]`); `--play` is a judgement stream and defaults to the
theoretical best play; `--gekisou` turns Gekisou on. `--scenario`, `--scenario-music` and `--context` select the
scenario (see `ournotes-deck --help`).

Constraints: `--leader ID`, `--include ID,...`, `--exclude ID,...`, `--exclude-snaps ID,...`, `--no-snaps`,
`--time-limit-ms N`. The output is JSON.

Chart statistics:

```sh
ournotes-deck chart-stats --data deck-data.json [--seeds 8] [--charts ID,...] [--jobs N] -o chart-stats.json
```

measures every chart's deck-independent numbers on the whole-live simulation (`ournotes-deck.chart-stats/2`) in
two scenarios: Gekisou on (`seeds`, as a Battle Live plays) and Gekisou off (`offSeeds`, as a solo live such as Free
Live or Challenge Live plays). `--charts` measures the listed score ids only (in the file's order; ids the file does
not have are ignored); `--jobs N` measures N charts at once and writes the same document as one at a time. Without
`-o` the document goes to the standard output.

With Gekisou on, the play is the theoretical best play with Gekisou: every note judged at its time, Just inside the
Just-count ranges and Perfect elsewhere, rank 1 in every range. Per seed: the exact no-skill score; the Gekisou
ranges' results (`ranges`: the range score, the rank 1 bonus, the largest Gekisou combo `maxCombo`, the Just count
`justCount`, the luck points `luckPoints` and the lottery results `lotResults`; the combo, Just and luck missions
rank by `maxCombo`, `justCount` and `luckPoints`, and without skills the luck points come from the lottery of the
luck ranges alone, 0 elsewhere); and for every score-up kind of the master (2000 / 2002 / 2004 / 2005 rows grouped by
type, duration, targets and conditions, see `kinds`) at every performance position the score gained at factor 1 per
unit of deck power (`weights[kind][k]`). A deck scores about `P × (score / power + Σ factor_k × weights[kind_k][k])`;
every seed checks this on a random deck of the master's own values at another power and fails beyond the flooring
bound. Charts with a luck range are given on the first N published seeds (`--seeds`, default 8), which is not a
native expectation; a chart with more than three fevers, where the game fails when the fourth starts, is
`unplayable` (it plays with Gekisou off).

Other ranks need no further play: a rank bonus is `trunc(rangeScore × percent / 100)`, a fixed score in the frame of
the range's end that changes no factor and no note score, so at rank r_i in range i the no-skill score is exactly
`score − Σ rankBonus_i + Σ trunc(rangeScore_i × rankBonusPercents_i[r_i − 1] / 100)` and a weight is
`weights[kind][k] + Σ (rankBonusPercents_i[r_i − 1] − rankBonusPercents_i[0]) / 100 × rangeWeights[kind][k][i]`
(`rangeWeights`: the range score the effect gains per unit of deck power). Every seed also plays its check deck at
random ranks through explicit rank confirmations (`rankCheck`). A kind whose conditions read the confirmed rank
(7012) has no `rangeWeights`, nor has a chart where a rank bonus can fall inside another range's score frames. Every
seed also gives the no-skill score and range scores of the same play with every Just judged Perfect
(`scorePerfect`, `rangeScorePerfect`).

With Gekisou off, the play is the theoretical best play (every note Perfect at its time), seed 0, without Just, luck,
Gekisou combo or rank bonus; `score`, `weights` and a check as above. A kind whose conditions read the Gekisou state
cannot play without Gekisou and has null weights.


### Gekisou skill aptitude

Statistics also include each skill's aptitude for a chart, without selecting a best formation or changing `seeds`
or `offSeeds`. The file-level `gekisouAptitude` holds shapes and measurement rules; each chart's
`charts[].gekisouAptitude` holds range `factors` and the `variants` of its missions. It is null for a chart with no
Gekisou range, one unplayable with Gekisou, or a master without measurable skills. These are additional fields;
the format remains `ournotes-deck.chart-stats/2`.

```sh
ournotes-deck chart-stats --data deck-data.json --aptitude-max-seeds 128 --aptitude-cross-seeds 32 -o stats.json
ournotes-deck chart-stats --data deck-data.json --no-gekisou-aptitude -o baseline.json
```

- `--aptitude-max-seeds N`: at most N seeds for a random increment, default 65536, N at least 2; stop earlier when both grade endpoints meet their SE targets.
- `--aptitude-cross-seeds N`: at most the first N seeds for ordinary skill cross terms, default 64, N at least 1.
- `--no-gekisou-aptitude`: skip aptitude measurement; both file-level and per-chart `gekisouAptitude` are null.
  Existing statistics are still produced.

Shapes deduplicate by source, mission and effect parameters. Member skills use their highest level; support skills
use the level at the snap's highest rank, not necessarily the highest level in the effect table. Support skills
that differ only in their band targets share a shape, preserving `skills[].memberTargetIds` / `bandIds`, and are
measured both with `bandMatch: true` and `false`. Each support skill has a **synthetic, effect-free member Gekisou
skill** of its own mission as host, never a real card whose effects could contaminate the increment. Each run plays
one member or support skill alone through the whole-live engine.

`score`, `scorePerfect`, `tail` and range increments are `[mean, standard error of the mean]`: with-skill minus
without-skill on the same seed, at `model.power`. `tail = Δscore − Σ(ΔrangeScore + ΔrankBonus)` includes gains outside
the range score frames, such as effects persisting after the range ends. `factors` lists range note counts,
entering combos and baseline lottery counts. `weights` gives changes in the plain ordinary kind's weight at each
position; `rangeWeights` gives the corresponding range changes, not full formation weights. Both are null without
a plain kind. Each variant's `check` uses its first measured seed, random ranks and an ordinary-skill deck at
another power to validate the linear prediction; exceeding its flooring bound fails the measurement.

Random increments start at 32 seeds and double along the same seed prefix until the configured cap. Preset batches
extend to 65536; a cap outside those boundaries is included as the final batch.
`score` and `scorePerfect` each use their own increment mean and no-skill baseline. Both SEs must be at most
`max(1% × |mean increment|, 0.1% × mean no-skill score)` before stopping. At the cap, `seTargetMet` reports whether
both targets were met. Deterministic increments report one seed and zero SE; four identical samples alone cannot establish that
a random skill is deterministic. The seed mean is not the game's expectation: its seed law is unknown, and SE
does not measure model error. Cross terms can use fewer seeds, reported as `crossSeeds` per variant (0 without a
plain kind). Meeting the SE target may only satisfy the absolute baseline threshold, not 1% relative precision
on the increment; a small sample mean's sign alone does not establish that a skill helps or hurts.

**Model limits:**

- Only `battleLiveScore` changes, not the separately reported `soloScore`; Free Live has no aptitude gain.
- **Do not add increments of multiple skills.** Gekisou combo saturation, luck gauge/rush interactions and
  Just-count-dependent support triggers can all break additivity.
- The theoretical best play has no Great or Miss: combo protection 12004, Great-to-Perfect 12006 and judgement
  window extension 4004 have zero effect here. Just-count effects 13000/13002 and luck-point effect 11002 can change
  range indicators without increasing score. Shapes of other missions are gated off and omitted on this chart.
- Ordinary-skill factors and rank changes use the linear formula, with rank rounding differences checked by
  `check`. Below 100% Just, interpolation of the no-ordinary-skill increment between Just and Perfect plays is
  approximate: conversion 13005, per-Just support 2001 and Just-count effect 13002 are nonlinear. Perfect-play
  cross weights are not measured, so full aptitude with nonzero ordinary skills is unavailable below 100% Just.
  Scaling by `1 − 0.2q` for Great proportion q is also approximate.

These interpolation and scaling limits concern statistical summaries. A declared per-note play uses the shared
`replay` API with its actual frame order, skills and seed; see the [model contract](docs/native-validation.en.md#calculation-contract-of-the-shared-model).

Library callers can use `chart_stats_with` / `document_with` with
`Options { seeds, aptitude: Some(AptitudeOptions { max_seeds, cross_seeds }) }`; `aptitude: None` disables it.
A `DeckData` without charts still produces the shape header, also available through
`aptitude_header(master, kinds, options)`.

## Tests

`cargo test` at the repository root runs the unit and integration tests of both crates: it reads synthetic deck data
files and compares the search with exhaustive enumeration on small synthetic pools. The two WASM packages build with
`cargo build --manifest-path wasm/<replay|recommend>/Cargo.toml --target wasm32-unknown-unknown --release`. `OURNOTES_DECK_ORACLE_CASES`, `OURNOTES_DECK_ORACLE_SEED0`,
`OURNOTES_DECK_ORACLE_MEMBERS` and `OURNOTES_DECK_ORACLE_SNAPS` enlarge that comparison. For the live score with snap
skills, `OURNOTES_DECK_SNAPS_CASES`, `OURNOTES_DECK_SNAPS_SEED0`, `OURNOTES_DECK_SNAPS_MEMBERS`,
`OURNOTES_DECK_SNAPS_SNAPS`, `OURNOTES_DECK_SNAPS_NOTES` and `OURNOTES_DECK_SNAPS_VARIANTS` do the same.

## Changelog

[CHANGELOG.md](CHANGELOG.md) lists the changes, generated from the commits (Conventional Commits) by
[git-cliff](https://git-cliff.org/). The commit that bumps the version regenerates it with
`git cliff --tag vX.Y.Z -o CHANGELOG.md`.

## Licence

MIT OR Apache-2.0.
