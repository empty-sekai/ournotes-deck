# Account input

`ournotes.account/1` contains the original game save in `account: {"_player": ...}` and an envelope:

- `datasetId`: SHA-256 of the exact deck-data bytes, including any UTF-8 BOM.
- `server`: `jp` with JP data, `intl` with TW data.
- `revision`: an opaque revision, such as the save content digest; never an account identifier.
- `coverage`: all seven keys below, each `complete` or `partial`.
- `assumptions`: `{path, reason}` records for facts explicitly declared by the caller.
- `declared._vip._rank`: the VIP rank, starting at 1. The save's `_player._vip` is ignored.

The seven coverage keys are `_player._memberCards`, `_player._supportCards`, `_player._characters`,
`_player._bandItems`, `_player._memory._musicGroups`, `_player._memory._members`, `_player._memory._supports`.
A complete list treats absent cards as unowned, absent characters as experience 0, absent furniture as level 0,
and absent memories as locked. Partial lists report any unknown fact required by the goal.

Read fields:

| Collection | Fields |
| --- | --- |
| `_memberCards[]` | `_masterId`, `_exp`, `_awakeCount`, `_rank`, `_liveSkillLevel`, `_performanceSkillLevel` |
| `_supportCards[]` | `_masterId`, `_exp`, `_rank` |
| `_characters[]` | `_masterId`, `_exp` |
| `_bandItems[]` | `_masterId`, `_level` |
| `_memory._musicGroups[]` | `_id`, `_musics[]._id`, `_musics[]._unlockedScoreRank` |
| `_memory._members[]`, `_memory._supports[]` | `_id`, `_unlocked` |

All other save fields are ignored during parsing, including names and account/profile identifiers. They do not
enter the roster, answer or progress. Send the original JSON text; parsing and serializing it through JavaScript
numbers can change 64-bit integers. Identity fields accept integer tokens or decimal strings; 32-bit fields require
integer tokens. Missing/null scalar facts remain unknown. Duplicate keys in read objects and duplicate IDs are errors.

Experience is converted through each card kind's level table. Character rank uses `MasterCharacterRank._exp`;
account resolution rejects data missing those thresholds. Every represented character contributes to total rank.
A memory music group's bonus uses the songs actually listed in that group, not an inferred complete master group.
Power/skip do not read skill levels. Ordinary live reads live skill levels; Gekisou also reads performance skill levels.
Excluding a card from candidates does not remove its ownership for memory bonuses.

Input problems carry `{path, code, message}`. Paths use input array indexes, and card messages name `_masterId`.
Unknown data IDs and server/dataset mismatches are invalid; missing required facts produce `incomplete`.

The internal typed roster and legacy owned-snapshot transport remain available to existing validation callers.
The browser page's account transport does not infer or translate missing account facts from that legacy format.
