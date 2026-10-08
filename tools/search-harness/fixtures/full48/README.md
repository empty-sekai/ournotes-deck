# Full chart and ownership matrix

This fixture defines 48 played-Live requests using published chart and card data with three fixed hypothetical inventories. The repository contains the complete case definitions, declared card progress, player facts, play recipes and payoff parameters. `source.json` identifies the downloaded dataset dependency; generated datasets, streams, requests and run reports belong in the harness output directory.

See the [harness instructions](../../README.md) for preparation and execution commands.

## Coverage

| Group | Cases | Contents |
| --- | ---: | --- |
| `score` | 18 | Six LUCK-containing charts across all three profiles |
| `nonlinear` | 18 | Four score thresholds, five capped scores, five event-point objectives and four score-and-final-LIFE thresholds |
| `control` | 12 | Two Free charts and two lottery-free Mission charts across all three profiles |

The profiles declare 20 members and 12 Snaps (`newcomer`), 23 and 19 (`midcore`), and 44 and 42 (`veteran`). Every declared owned card remains eligible. The search chooses among all legal leaders and member/Snap bindings. Score uses the uniform expectation over all 120 member performance orders, with each Snap paired to its member.

Every request specifies K=3, a 60,000 ms search budget, no candidate cap, 1,024 cache entries and empty constraints. The external process watchdog is 90,000 ms. These limits are explicit fixture inputs. A run reports its actual completion and proof status; `TimedOut` and `RefinementRequired` remain unresolved results.

## Files

- `matrix.json` declares the ordered cases, chart selectors, request templates, play recipes and event context.
- `profiles/*.json` declares exact member/Snap IDs, cultivation and skill levels, character ranks, VIP rank, active band-item levels, event selection and memory progress. These are fixed inputs, rather than a random inventory generated during each run.
- `source.json` supplies the dataset location and content identity used by preparation. The downloaded dataset provides card definitions, legal progress ranges, chart geometry and scoring tables.

Preparation validates every referenced ID, declared progress value and scene/chart association against the selected dataset. It computes the owned snapshot's dataset identity from the original verified bytes and derives complete owned-ID lists from each profile's member/Snap arrays. The profile's `player` object uses the owned-snapshot field layout and remains unchanged.

## Materialization

Each case's `request` is a template for `ournotes-deck.search-request/1`. Its `execution.play` is supplied by the named play recipe. Cases with a `context` reference receive the corresponding materialized event context. The remaining request fields are already explicit, including the nonlinear thresholds.

`best` uses the native theoretical judgement policy. `miss-every-211` asks `benchmark_prepare` to construct the native theoretical stream for the selected scene and replace every 211th judged entry, counted from one, with a Miss. Its seed is zero. Both members of a Score/LIFE pair use the same complete stream. The recipe also supplies all three lottery-free-long controls. Frame times, native Just eligibility and note identities come from the downloaded chart through the existing model.

The event-point context uses event 1, one consumed count and zero local event balances. Its declared result time is 496,800 seconds after that event's start in the pinned dataset. Preparation converts this declared wall time to normalized DateTime ticks without a timezone offset, sets both saved start and result time to that value, and retains `canonical-master-no-offset` for event-window interpretation. The computer's current date does not affect the request.

The generated benchmark uses `ournotes-deck.search-benchmark/1`; its snapshots use `ournotes.owned-snapshot/1`. It can therefore use the existing native `profile_case` and WebAssembly harness transports. Dataset preparation and successful process execution are separate from a complete, proven search result.
