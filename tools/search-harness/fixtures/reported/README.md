# Reported reproduction corpus

This independent corpus contains one case, `issue9-battle-original`, from [issue 9](https://github.com/empty-sekai/ournotes-deck/issues/9) and [PR 10](https://github.com/empty-sekai/ournotes-deck/pull/10). `request.json` and `roster.json` preserve the supplied original bytes. `provenance.json` records their SHA-256 identities. The issue author describes the inventory as a synthetic reproduction roster, not a player account. This harness preserves that author's original reproduction inventory; it does not generate a new one. The chart and master data come from the public game dataset selected below, separately from the synthetic inventory.

The original sample does not identify a dataset version. This corpus explicitly selects the existing [pinned TW source](../full48/source.json), SHA-256 `de867d2df3020e9430c164cdc889cd114113e50b2ab12003b6978665905493bf`. This is a declared version choice for repeatable regression, not a claim to have recovered the report's original dataset.

The request remains Battle music `100010`, score `10001002`, K=5, 60,000 ms, 2,048 cache entries, no candidate limit, empty hard constraints, theoretical-best Gekisou play, and three 250% network rank confirmations. Its original integer context timestamp is never round-tripped through JavaScript serialization. The roster retains all 15 members and 35 Snaps, every member's original cultivation and skill levels, every legal leader, all unique optional Snap bindings, and all 120 performance labels.

## Preparation

```sh
cargo build --release --locked --manifest-path tools/search-harness/Cargo.toml --bin reported_projection
python3 tools/search-harness/reported.py prepare --no-build \
  --cache-dir work/dataset-cache --out work/reported-inputs
```

`--data FILE --offline` accepts an existing dataset only when it matches the pin. `--audit-binary FILE` selects an already built native audit. Existing output directories are rejected. `reported.py` verifies the original source hashes, copies the request and roster without rewriting either, derives an owned snapshot, and invokes `reported_projection`. It publishes `benchmark.json` only after that native audit succeeds.

The schema mapping copies members and Snaps directly into `eligible`, preserves their order, copies the explicit owned-ID lists, converts character-rank and active-band-item maps into `{id,value}` lists, and renames `events` to `eventIds` and memory `unlockedSupports` to `unlockedSnaps`. The explicit empty memory object stays explicit. Missing `exp` and `memory.musicGroups` retain the parsers' existing None values. No cultivation, item ownership or memory progress is invented. Owned-card coverage is conservatively `partial`, because an original legacy ID list does not establish complete account coverage; eligibility is unchanged. The supplied ranks cover every character in the selected Master, so the existing complete-map rule derives total rank 916.

The native audit uses the production strict owned-snapshot resolver and `build_card_pool` for the original request. It compares every parsed roster/player field, every resolved member/Snap field, all power-calculator fields, each candidate's active-item bonus and all Snap skill pairs. Exhaustive struct patterns require the audit to include future model fields. The only normalization is legacy absent total rank versus the strict resolver's identical derived `Some(916)`. It compares both native candidate domains, all eligible member/Snap indexes, required members, unrestricted leader selection, feasibility, scenario context and route. It performs no search or native live simulation.

The output preserves `projection-audit.json`, the original source provenance, a preparation receipt binding the native audit executable and all input hashes, and the derived snapshot. The portable CI bundle also binds these evidence files and retains the original roster. The audit establishes equivalence under the selected repository model; it does not certify game parity or the original account's legality.

## Execution

```sh
node tools/search-harness/benchmark.cjs work/reported-inputs/benchmark.json \
  PROFILE_CASE work/reported-native --candidate-only --runtime native --candidate-source .
node tools/search-harness/benchmark.cjs work/reported-inputs/benchmark.json \
  WEB_PACKAGE work/reported-browser --candidate-only --runtime browser --candidate-source .
```

The Chromium route uses the generic completion benchmark runner. It retains `TimedOut`, `RefinementRequired`, failures and full response JSON; it does not require a complete native answer before recording a browser run. CI suite `reported` contains exactly this one case. `real` contains the unchanged 48-case `full48` matrix plus this case; `all` additionally contains the 16 synthetic cases, totaling 65. Full48 remains exactly 48 requests with K=3 and 1,024 cache entries. The 20-second end-to-end target remains separate from every original request's 60-second search budget.
