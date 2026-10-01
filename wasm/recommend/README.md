# Draft current-core browser recommendation adapter

This research build calls the unchanged current-main score, power and payoff implementation. It does not certify latest native parity, arbitrary roster validity, or screenshot-to-recommendation production behavior.

```js
import init, { RecommendationSession } from './pkg/ournotes_recommend_wasm.js';
await init();
const session = new RecommendationSession(deckDataJson);
session.setRoster(rosterJson);
const resultJson = session.recommend(requestJson);
session.free();
```

Pass the original UTF-8 JSON strings. Run each synchronous recommendation in a dedicated Worker. A failed roster JSON parse retains the previous roster. Changing DeckData requires a new session. The wrapper does not authenticate ownership, reject every unknown legacy roster key, or validate missing progression/rank data; the formal roster resolver remains pending.

All explicit `networkConfirmations` and `simulation.liveFinishedFromFrame` inputs, including null and empty arrays, are Unsupported before computation. This build does not implement their newer settlement semantics.

Deadlines are cooperative at complete atom boundaries. Fully evaluated candidates before the deadline may remain; an incomplete atom/candidate never enters Top-K. Hard cancellation terminates the whole Worker and drops its state. There is no SearchSession.step/resume, streamed incumbent, multi-threaded search, or browser OCR loop.

Only the 28 explicitly synthetic current-core corpus cases are certified by the accompanying CLI/WASM report. Timing-dependent completion counts can differ. Duplicate-character search requirements produce a Complete empty feasible domain; this differs from illegal FixedDeck slots rejected as Input by the core.

Build on CNB with the supplied locked dependencies and wasm-bindgen CLI 0.2.127:

```sh
cargo build --manifest-path wasm/recommend/Cargo.toml --target wasm32-unknown-unknown --release --locked
wasm-bindgen --target web --out-dir pkg target-j4/wasm32-unknown-unknown/release/ournotes_recommend_wasm.wasm
```