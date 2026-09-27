# ournotes-deck

Deck power, skip score and live score for BanG Dream! Our Notes, and an exact Top-K deck search.

[中文](README.md)

## What it computes

- **Deck power**: per-slot terms (card stats from level, awake count and rank; character rank and total rank;
  band items; song type and tag bonuses; the snap's power bonus and type link; the leader skill; VIP; memory; event
  parameters) and the deck total, with the game's integer and binary32 float arithmetic, including its rounding and
  floor conversions.
- **Skip score**: the score of a skipped live for a chart.
- **Live score** (partial): the per-note score, frames, the combo bonus table, the factor commands of live skills
  and the resulting score of a judgement stream. Snap skills and Gekisou are not modelled yet and are reported as
  unsupported.
- **Event points**: event bonuses, score ranks, boosts and the event-point amount the game client computes. The game
  server decides the awarded amount; this crate reproduces the client's own computation.
- **Search**: the best K decks for deck power (with or without a song, with or without event parameters), for the
  skip score and for the live score with live skills only, one result per set of five member cards.

## Exactness

A `Complete` search result is exactly the canonical Top-K over every legal deck; pruning uses only bounds that are
proven admissible under the game's arithmetic (see [docs/search.md](docs/search.md)). A search that reaches its time
limit returns `TimedOut` with legal, exactly evaluated decks and no ranking claim. Inputs outside the proven range,
unknown cards, rules the game would reject and parts of the game that are not modelled are reported as errors.

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

`level` may be replaced by `exp`. Charts are selected by score id (`MasterLiveMusicScore._id`). The live score
defaults to the theoretical best play: every judged note Perfect (with Gekisou off the game judges no Just), a full
combo and no life lost. Another play can be given as
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
ournotes-deck live  --data deck-data.json --roster box.json --score SCORE_ID --exclude-snap-skills [--play play.json] -k 10
```

Constraints: `--leader ID`, `--include ID,...`, `--exclude ID,...`, `--exclude-snaps ID,...`, `--no-snaps`,
`--time-limit-ms N`. The output is JSON.

## Tests

`cargo test` runs the unit tests, reads synthetic deck data files and compares the search with exhaustive
enumeration on small synthetic pools. `OURNOTES_DECK_ORACLE_CASES`, `OURNOTES_DECK_ORACLE_SEED0`,
`OURNOTES_DECK_ORACLE_MEMBERS` and `OURNOTES_DECK_ORACLE_SNAPS` enlarge that comparison.

## Licence

MIT OR Apache-2.0.
