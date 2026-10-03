# Item bonuses and event points

Band furniture raises member power; it has no direct event-PT effect. Boost drinks restore LB, and consuming LB
multiplies PT and rewards. Card/Snap event-item bonuses are a separate effect. A master that adds item effects or
resource constraints needs the model reviewed against these rules.

## Effects and player state

| Input or effect | Behavior | Model |
| --- | --- | --- |
| Band items 101–105, 201–205, 301–305, 401–405, 501–505 | Effect rows of type `1000` add the matching band's three power rates | [`BandItemMaps`](../crates/ournotes-sim/src/bonus.rs) |
| Band-item level | Use the exact owned item's level; level rows cover 1–30. Effect rows through level 50 do not make those levels legal | [`OwnedSnapshot`](../crates/ournotes-search/src/owned_snapshot.rs), [input contract](owned-snapshot.md) |
| Item 34 / 35, type `7` (`LB`), value 1 / 10 | Small/normal boost drinks restore LB; owning a drink does not change a run's score or PT | A run declares its consumption, not drink inventory |
| Event effect type `0` / `1` / `2` | Direct event PT / event-item amount / parameter bonus; distinct categories | [`event`](../crates/ournotes-sim/src/event.rs), [`bonus`](../crates/ournotes-sim/src/bonus.rs) |
| `consumedCount` for normal Live | 0: ×1; 1–10: the boost-rate row for that count (×5, ×10, …, ×50 for event PT) | [`boost_bonus`](../crates/ournotes-sim/src/event.rs) |

All declared owned furniture is used at its supplied level; there is no equipment-slot selection. Unknown
ownership or level stays unknown, and explicitly unowned items contribute nothing. Level 0 is not a valid owned
level in the strict input contract.

Furniture rates are summed before multiplying and truncating each base power channel, so several items together
give at least the sum of their separately truncated bonuses. The [differential test](../crates/ournotes-sim/tests/jp_band_item_differential.rs)
checks this against reference values recorded from the client; it runs with the `native-fixtures` feature (see
[native validation](native-validation.en.md#unit-level-reference-vectors)).

For a normal Live, PT uses `((10000 + card_PT_bonus) * eventPointRate * rank_base_PT) / 10000` with i32 wrapping
products and truncating division. Event items use their own bonus and reward rate. Furniture may increase PT
indirectly by raising score across a rank threshold; it does not add a direct percentage. Replenishing LB and
choosing how much LB to consume are different operations.

## Paired controls

Keep roster, deck, chart, judgements, random roots, objective and all other player state fixed when varying
furniture. Compare empty, one matching item at levels 1/30, all legal max items, and other-band controls. Report
power, score, score rank, direct PT percentage, event-item percentage and final PT separately. Include both
same-rank and threshold-crossing pairs.

At a fixed deck and score, compare normal-Live consumption 0/1/5/10; the PT multiplier follows the boost-rate
table. Consumption changes rewards, not score. Compare PT and event-item targets separately; drink inventory does
not substitute for an explicit consumption value. The [event tests](../crates/ournotes-sim/tests/event.rs) cover separate boost
columns, zero consumption and wrapping; the [ownership tests](../crates/ournotes-search/tests/owned_snapshot.rs) cover missing, unowned
and invalid item facts.
