# Browser recommendation adapter

WebAssembly transport for the owned-snapshot recommendation. It calls the same Rust score, power, payoff and
search code as the native `ournotes-recommend` command; nothing is reimplemented in JavaScript.

```js
import init, { DeckSolver } from './pkg/ournotes_recommend_wasm.js';
await init();
const solver = new DeckSolver(deckDataJson);    // throws an Error when the deck data is invalid
solver.datasetId;                               // lowercase hex SHA-256 of deckDataJson
const answerJson = solver.recommend(snapshotJson, requestJson, resultJson => {
  // progress report: the result JSON the search would return if its time limit expired now
}, 250);
solver.free();
```

Pass the original UTF-8 JSON text of all three documents: `JSON.parse`/`JSON.stringify` would round integers
above 2^53. A solver keeps one deck data document; another dataset needs another solver.

## Inputs

- `deckDataJson`: a `nnnotes.deck-data/1` document. `datasetId` is the SHA-256 of exactly this text, the same
  value as `sha256sum deck-data.json` or `crypto.subtle.digest('SHA-256', bytes)` over the file bytes, as long as
  the text passed in is the file decoded without changes (a byte order mark, if any, is part of the hash).
- `snapshotJson`: an `ournotes.owned-snapshot/1` document whose `datasetId` names this deck data. Unknown facts are
  `null`, never defaults; see [owned snapshots](../../docs/owned-snapshot.md).
- `requestJson`: an `ournotes-deck.recommendation-request/1` document. Its execution selects the facts the
  snapshot must supply: power and skip read no skill levels, Live reads ordinary skill levels, Gekisou Live also
  Gekisou skill levels.

## Answer

`recommend` returns `ournotes-deck.snapshot-recommendation/1` as JSON text and does not throw for input problems:

```json
{"format":"ournotes-deck.snapshot-recommendation/1","datasetId":"<sha256>","status":"incomplete",
 "missing":[{"path":"eligible.members[1001].liveSkillLevel","code":"missing","message":"required by the selected goal"}],
 "errors":[],"result":null}
```

- `status`: `ok` (`result` holds the recommendation), `incomplete` (only `missing` facts), `invalid` (`errors` in
  the snapshot or request, possibly with `missing`) or `failed` (the inputs resolved, the computation failed).
- `missing` and `errors`: lists of `{path, code, message}`. Paths follow the snapshot fields; card entries are
  addressed by card ID, as in `eligible.members[1001].awake`. Unparsable documents report path `snapshot` or
  `request` with code `parse`; problems found while building or searching report path `request` with the error
  kind as code (`input`, `unsupported`, `domain`, ...).
- `result`: the recommendation result, the same structure the native command writes, with the snapshot scope in
  `resolvedContext.ownedSnapshot`.

## Progress and cancellation

The optional callback receives progress reports as result JSON text: `completion` is `TimedOut` and `results`
hold the exactly evaluated decks found so far. Reports come from the search's deadline checks and after Top-K
changes, at most once per `progressIntervalMs` (default 250), the first once that interval has passed. Fixed-deck
evaluation and the canonical power/skip search make no reports. Exceptions thrown by the callback are ignored.
Reporting never changes the search: with or without a callback, a complete search visits the same nodes and returns
the same result.

`recommend` is synchronous. Run it in a dedicated Worker; the request's `limits.timeLimitMs` stops the search
cooperatively, and terminating the Worker cancels it at once.

## Build

```sh
cargo build --manifest-path wasm/recommend/Cargo.toml --target wasm32-unknown-unknown --release --locked
wasm-bindgen --target web --out-dir pkg wasm/recommend/target/wasm32-unknown-unknown/release/ournotes_recommend_wasm.wasm
```

Use the wasm-bindgen CLI version locked in `Cargo.lock` (0.2.127). `--target nodejs` builds the package that
`tools/search-harness/wasm-node.cjs` checks against native answers.
