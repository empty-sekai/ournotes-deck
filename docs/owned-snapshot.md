# Owned snapshot resolver

`OwnedSnapshot::from_json` parses `ournotes.owned-snapshot/1` directly into typed fields. All objects reject unknown
and duplicate fields; integer fields reject floats, numeric strings and out-of-range values. Identity lists are
validated for duplicates during resolution. An omitted or null optional fact is unknown, not an initial value.

The caller supplies the verified dataset identity to `resolve(master, expected_dataset_id, goal)`. The result
contains separate structured `missing` and `errors` lists (`path`, `code`, `message`), and a resolved value only
when both are empty. Identity agreement does not verify the supplied dataset's provenance or certify the model.

`ownedFacts.memberIds` and `snapIds` describe ownership independently of `eligible.members` and `snaps`. Each kind
has its own complete/partial coverage declaration. A card excluded from eligibility remains in the ownership
sets read by memory calculations. An eligible card must be owned. Memory unlock lists must refer to owned facts.
These are declarations, not evidence that a screenshot covers the whole account.

Player facts include local character ranks with complete/partial coverage, an independent total rank, VIP,
active band items, memory progress, and active event IDs. Empty lists explicitly state no active entries; null
means unknown. The resolver checks positive ranks represented by `MasterCharacterRank` and checked sums,
complete coverage against master characters,
and partial totals against the known ranks plus the supplied rank table's possible missing-character ranks.
Complete local-rank coverage can derive a missing total by checked sum; the resolved value reports
`DerivedCompleteRanks` and leaves the original optional total unknown. Partial coverage still needs an independent
total. It passes the observed or derived total through a dedicated Player field, never invented character rows. Legacy Player
inputs retain their historical behavior and are not upgraded to this strict contract.

Manual furniture input can instead use `player.bandItemFacts` with `coverage` and `values` containing exact integer
`id`, optional boolean `owned`, and optional integer `level`. It is mutually exclusive with a non-null legacy
`bandItems` list, including `[]`. Complete coverage requires a row for every bound `MasterBandItem` identity;
partial saves preserve omitted/unknown ownership and return missing fields during evaluation. Owned=true requires
an explicitly supplied level; owned=false must have no level. A present level never infers ownership. Only known
owned rows enter the private calculation map; unknowns and explicit unowned entries remain in the original facts.

Both manual facts and nonempty legacy item lists require the actual `MasterBandItem` identity and exact
`MasterBandItemLevel` row, then the matching effect row. Reserved effect rows alone cannot authorize cultivation.
Captured JP `1.0.0.300` has 25 item identities, 750 represented level rows (1..30 per item), and 1250 effect rows
(1..50); this static observation is not native execution or player attainability evidence. Required player-rank
metadata is parsed, but resources/unlock/player-rank prerequisites and native bonus/rounding certification remain
separate.

Strict VIP input requires a positive rank represented by `MasterVip._vipRank`; bonus rows and their IDs do not
define that domain. The same frozen JP source has represented ranks 1..21 and bonus rows for ranks 2..21;
rank 1 is legal with no deck-power bonus row. A catalog without `MasterVip` returns `unsupported_master` for
supplied VIP input. A fresh export needs this table and a new content identity. The original rank-0 constructor
calibration remains a legacy low-level experiment; it is rejected by the normal strict player input and is not
relabeled as a lawful account. Character ranks use their full rank table, independently of total-rank bonus
thresholds (frozen JP local ranks 1..50 versus total thresholds up to 5000).

The exact deck-data format remains `nnnotes.deck-data/1`; `/2` is still rejected. Old effect-only datasets do not
gain the new manual capability: nonempty legacy item input or manual item facts return `unsupported_master` when
the catalog/level tables are absent. The existing empty legacy list retains its explicit no-active-items meaning,
with `legacyCompleteActiveItems` scope and no inferred ownership coverage. A fresh export must bind its own actual
content hash; existing frozen source/native proof is not relabelled after adding tables. Box may keep IDs as
decimal strings, but original UTF-8 core JSON requires unquoted i64 integer tokens and rejects quoted IDs/floats.

Eligible member facts accept level or experience (both must agree), awake, rank and optional ordinary/Gekisou
skill levels. Member level caps come from `MasterMemberCardLevelLimit` at the actual rarity/awake; Snap caps and
skill levels come from the actual Support rank row. Missing cap/rank rows produce explicit errors. This checks
the loaded rows, not every prerequisite, resource expenditure, unlock rule or account lifecycle.

Power and Skip do not require unused member skill levels. Their private projection marks those slots unavailable
and retains the original unknowns in `snapshot()`. Callers cannot obtain its Pool/Roster; `evaluate_deck` and
`search` reject a different goal before using the shared core. Physical slots and unique Snap constraints use the
existing Pool checks, with slot 2 as leader. `snap_skill_derivation` exposes only rank-derived metadata.

Normal Live requires the ordinary skill level of every eligible member that has a live skill; Gekisou Live also
requires the Gekisou skill level of every member that has one. A known level must have effect rows in
`MasterLiveSkillEffect` or `MasterGekisouSkillEffect`, otherwise it is a `master_row_missing` error rather than a
member playing without the skill. Levels the goal does not read stay unavailable in the private projection and
unknown in `snapshot()`. `GoalDependencies::of(&request.execution)` gives the goal of a request. Scene/play/root-law
dependencies belong to the shared evaluation boundary and are not invented from the card snapshot.

Assumptions annotate explicitly supplied fields with a path and reason; they never fill unknown values. Consumers
must retain these annotations and coverage declarations in result scope. Neither a complete declaration nor a
successful resolution is an account-wide legality, native equivalence or optimality certificate.

Public entry points:

```rust,ignore
let snapshot = OwnedSnapshot::from_json(original_utf8_json)?;
let report = snapshot.resolve(&dataset.master, verified_dataset_id, GoalDependencies::Power);
// Show missing/errors before evaluation. Do not drop them or substitute defaults.
let resolved = report.resolved.expect("caller handled missing/errors");
let result = resolved.evaluate_deck(member_ids_in_slot_order, paired_snap_ids, &objective)?;
```

For the shared scenario/goal entry points, use `snapshot.resolve_data(&dataset, verified_dataset_id, goal)`.
`engine::recommend_snapshot(&dataset, snapshot_json, request_json, progress)` does all of this from JSON text: it
derives the goal from the request, resolves against `DeckData::sha256` (the SHA-256 of the deck data text) as the
dataset identity, and returns `missing`/`errors` with a `status` instead of failing; the browser adapter in
`wasm/recommend` exposes exactly this answer.
The resolved value borrows the entire `DeckData`, so safe Rust cannot change its charts or provenance while
that value is in use. `recommend`, `evaluate_fixed`, and `rank_fixed_songs` require that same bound object and
the goal the snapshot was resolved for. They preserve the private projection and reject a request that replaces
the resolved active-event facts. A Master-only resolution cannot call these entry points.

Every shared result carries the snapshot revision, owned and eligible coverage, assumptions, and whether the
total rank was observed or derived. Song ranking retains this scope even when its budget yields no rows.
The loader must still verify the content identity supplied as `datasetId`; a Rust object reference is not a
content hash or proof that user-supplied facts are true. Shared scope also carries `playerBonusEvidence` with
`conditionalCurrentCoreProjection` status: memory and event effects explicitly remain `unmodeledLatestNative`
while the current core consumes the declared facts. Complete input coverage does not establish their full
latest-native numerical behavior or full account legality.

`start_search_session` starts an in-memory cooperative exhaustive Power/Skip physical-deck search from this
same dataset-bound projection. It freezes the request and preserves the owned scope, separates slice budgets from
one total monotonic deadline, supports cancel/resume only for the unchanged unexpired input, and invalidates stale
bindings permanently. Its explicit v1 physical Top-K/tie differs from the synchronous canonical member-set fast
path. The full contract and limitations are in [search-session.md](search-session.md).
