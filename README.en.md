# ournotes-deck

Deck power, skip score and live score for BanG Dream! Our Notes, and an exact Top-K deck search.

[中文](README.md)

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
  Gekisou snap skills) is modelled as well; the search supports Gekisou off only.
- **Event points**: event bonuses, score ranks, boosts and the event-point amount the game client computes. The game
  server decides the awarded amount; this crate reproduces the client's own computation.
- **Search**: the best K decks for deck power (with or without a song, with or without event parameters), for the
  skip score and for the live score, one result per set of five member cards. The live score is the whole-live
  simulation with live skills and snap skills (Gekisou off) under a judgement stream, or, with snap skills excluded,
  the score of a per-note play with live skills only. With snap skills the search simulates candidate decks, so it is
  slower, and streams with many missed or late notes can take much longer; a time limit bounds such requests.

## Correctness

Correctness has three layers, and each shows something different.

**Agreement with the game.** Every calculation follows the game client's code function by function. Each unit is
then checked against the client's own implementation: the client's arm64 native functions run in an emulator on the
same inputs, and the results are compared bit for bit (floats by their bits, with integer overflow and the paths that
throw compared too). The checks cover:
- the deck-power primitives and the slot calculation;
- note scores, frames and the combo bonus;
- factor commands and the score calculator's rewind;
- the conditions and effects of live skills and snap skills;
- life, combo and judgement;
- randomness;
- score ranks and event points.

There are millions of generated inputs, real master rows among them, and 0 mismatches. Deliberately broken variants
were run alongside, to confirm that the comparison does catch differences. The five-slot sum and the
bonus builders are integer code; they were not run on their own, and are ported function by function. This crate is
then compared with that checked model, also with 0 mismatches: more than 50,000 whole-live frame-by-frame scenarios
each with Gekisou off and on, including 1,048 and 1,267 real charts played in full.

**Exact search.** A `Complete` search result is exactly the canonical Top-K over every legal deck. Pruning uses only
bounds that are proven admissible under the game's arithmetic (proofs in [docs/search.md](docs/search.md)). Search
results are compared item by item with an independent exhaustive enumeration, which shares no bound, decomposition
or Top-K code with the search. On real cards and charts, 25,600 requests covering about 450 million decks showed 0
mismatches. For the live score with snap skills the enumeration simulates every member set, leader, snap placement
and performance order: on real cards and charts, with the default and random judgement streams, 1,280 requests over
2.2 million simulated decks and orders, and on synthetic pools whose snap skills change the ranking, 3,200 requests
over 109 million, showed 0 mismatches.
A search that reaches its time limit returns `TimedOut`, with legal and exactly evaluated decks but no
ranking claim. Inputs outside the proven range, unknown cards, rules the game would reject and parts of the game that
are not modelled are reported as errors.

**Not yet verified.** How the units combine, frame by frame, into a whole live has not been compared with the game as
a whole. That needs a recording of a play on a device: the random seed, frame times and each note's judgement.
With Gekisou on the score depends on the random seed; the search does not offer a Gekisou objective yet.

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

```rust
use ournotes_deck::cards::Roster;
use ournotes_deck::data::DeckData;
use ournotes_deck::search::{Constraints, Objective, Pool, SearchRequest, search};

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
ournotes-deck power --data deck-data.json --roster box.json -k 10 [--music ID] [--event]
ournotes-deck skip  --data deck-data.json --roster box.json --score SCORE_ID -k 10
ournotes-deck live  --data deck-data.json --roster box.json --score SCORE_ID [--play stream.json] -k 10
ournotes-deck live  --data deck-data.json --roster box.json --score SCORE_ID --exclude-snap-skills [--play play.json] -k 10
```

`live` scores with snap skills and reads `--play` as a judgement stream; with `--exclude-snap-skills` it scores live
skills only and reads `--play` as a per-note play.

Constraints: `--leader ID`, `--include ID,...`, `--exclude ID,...`, `--exclude-snaps ID,...`, `--no-snaps`,
`--time-limit-ms N`. The output is JSON.

## Tests

`cargo test` runs the unit tests, reads synthetic deck data files and compares the search with exhaustive
enumeration on small synthetic pools. `OURNOTES_DECK_ORACLE_CASES`, `OURNOTES_DECK_ORACLE_SEED0`,
`OURNOTES_DECK_ORACLE_MEMBERS` and `OURNOTES_DECK_ORACLE_SNAPS` enlarge that comparison. For the live score with snap
skills, `OURNOTES_DECK_SNAPS_CASES`, `OURNOTES_DECK_SNAPS_SEED0`, `OURNOTES_DECK_SNAPS_MEMBERS`,
`OURNOTES_DECK_SNAPS_SNAPS`, `OURNOTES_DECK_SNAPS_NOTES` and `OURNOTES_DECK_SNAPS_VARIANTS` do the same.

## Licence

MIT OR Apache-2.0.
