# Browser recommendation adapter

A single-threaded Worker transport over the shared Rust account resolver and exact search.

```js
import init, { DeckSolver } from './pkg/ournotes_recommend_wasm.js';
await init({ module_or_path: wasmBytes });
const solver = new DeckSolver(new Uint8Array(deckDataBytes));
const capabilities = JSON.parse(solver.capabilities());
console.assert(solver.datasetId === deckDataSha256);
const answerJson = solver.recommend(accountJson, requestJson, progressJson => {
  // A complete account-recommendation/1 answer with final:false.
}, 250);
solver.free();
```

Use original UTF-8 deck-data bytes and original account/request JSON text. Invalid UTF-8 is rejected. The dataset
hash covers exactly the bytes, including any BOM. JSON.parse/stringify can round account IDs or JST ticks.
Construction also accepts original JSON text for existing callers. Legacy owned-snapshot inputs retain their
original request/answer/progress shapes; account inputs use the new formats below.

- Account: `ournotes.account/1` ([fields and coverage](../../docs/account-input.md)).
- Request: `ournotes-deck.recommendation-request/2` ([goals and semantics](../../docs/recommendation.md)).
- Answer: `ournotes-deck.account-recommendation/1`, with `status`, `missing`, `errors`, `final`, and `result`.
- `capabilities()` returns JSON text identifying formats, supported pairs, and explicit limitations.

`recommend` is synchronous. Use a dedicated Worker and terminate it to cancel. Callback exceptions are ignored.
An input error is a structured answer, not a trap. Only a proven result may be called optimal. Explicit unsupported
areas remain release gaps; the adapter does not make a sampled or weighted lottery score exact.

```sh
cargo build --manifest-path wasm/recommend/Cargo.toml --target wasm32-unknown-unknown --release --locked
wasm-bindgen --target web --out-dir pkg wasm/recommend/target/wasm32-unknown-unknown/release/ournotes_recommend_wasm.wasm
```

Use the locked wasm-bindgen CLI version, 0.2.127. A `--target nodejs` package can be checked with the synthetic
account transport corpus described in [recommendation validation](../../docs/recommendation.md).
