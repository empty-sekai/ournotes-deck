# Changelog

All notable changes to this project are listed here. Versions follow [Semantic Versioning](https://semver.org/).

## [0.0.5](https://github.com/empty-sekai/ournotes-deck/compare/v0.0.4...v0.0.5) - 2026-10-10

### Features

- **chartstats:** Cache compiled measurement programs ([293d81c](https://github.com/empty-sekai/ournotes-deck/commit/293d81c0c3121df86cfeaa057859bc518b0e5fd4))

## [0.0.4](https://github.com/empty-sekai/ournotes-deck/compare/v0.0.3...v0.0.4) - 2026-10-10

### Features

- **chartstats:** [**breaking**] Compute nominal chart expectations ([#33](https://github.com/empty-sekai/ournotes-deck/issues/33)) ([cbdd680](https://github.com/empty-sekai/ournotes-deck/commit/cbdd680199f8cc80a85708953514dedaa12fddc8))
- **replay:** Add exact skill-order rank analysis ([e6c6aff](https://github.com/empty-sekai/ournotes-deck/commit/e6c6aff050eb787326ddab6ad89f9a7c4e60859f))
- **event:** Derive item rewards from each result grade ([c2ecab8](https://github.com/empty-sekai/ournotes-deck/commit/c2ecab8d88b7d0b1237bc163675b50ad0fc60a7d))
- **search:** Rank challenge-point ties by event rewards ([9ff6ac1](https://github.com/empty-sekai/ournotes-deck/commit/9ff6ac149f80ee5a5e8ca30f497997ceeea7b4f6))

### Bug fixes

- **search:** Certify canonical ranking and pruning bounds ([c02509d](https://github.com/empty-sekai/ournotes-deck/commit/c02509d47fe9f8a535d1da6f006778a6f46ea9b1))
- **search:** Certify numeric domains and replay boundaries ([ebb2e9f](https://github.com/empty-sekai/ournotes-deck/commit/ebb2e9f3ba58658399a591631de4770d57644f6b))
- **search:** Certify timers beyond the play horizon ([8932b19](https://github.com/empty-sekai/ournotes-deck/commit/8932b191709a470207a677a1fee899c159613647))
- **search:** Retain bounds for damage reduction skills ([6759c26](https://github.com/empty-sekai/ournotes-deck/commit/6759c2624d4e56c063390f935f2d4a9393a0514f))
- **validation:** Preserve matrix outcomes for invalid contracts ([1d47039](https://github.com/empty-sekai/ournotes-deck/commit/1d470395950838eda6200b60c1ece4579670bb49))
- **search:** Certify window-weighted factor roundoff ([fdf55de](https://github.com/empty-sekai/ournotes-deck/commit/fdf55decc70859802c76001eb8e2969f17dd68b2))
- **search:** Certify replayed factors and bounded warm starts ([7be5f0f](https://github.com/empty-sekai/ournotes-deck/commit/7be5f0f2b8e32be8ed4019fd683cf8b2a33e8a48))

### Performance

- **sim:** Resume exact lottery branches from frame checkpoints ([e9dab37](https://github.com/empty-sekai/ournotes-deck/commit/e9dab37db32d55667e4da9b282817ef33f01147b))
- **search:** Materialize LUCK refinement state at the ranking boundary ([392044f](https://github.com/empty-sekai/ournotes-deck/commit/392044f80b001718c385a58aaf52ffa561bfb7dc))
- **luck:** Reuse complete laws and prioritize uncertain orders ([463eaed](https://github.com/empty-sekai/ournotes-deck/commit/463eaedbf236fd91b9d69d1dcd11f5950b9586aa))
- **search:** Bound certified warm-start proposals ([f31cc6f](https://github.com/empty-sekai/ournotes-deck/commit/f31cc6fba438a85c064fe3b60e9f607518d8c460))
- **luck:** Reuse certified recorder states within score sessions ([96ce5df](https://github.com/empty-sekai/ournotes-deck/commit/96ce5dfbc9776af8e15632619a7d2e807d7bc59f))
- **search:** Prepare conversion envelopes on demand ([0f230fd](https://github.com/empty-sekai/ournotes-deck/commit/0f230fd5a1b5ab05f7ce2668c9c885425178163e))
- **search:** Group conversion traversals by envelope ([f781689](https://github.com/empty-sekai/ournotes-deck/commit/f7816890130d36f85f1d046468426f08586dea1a))
- **search:** Compile conversion bounds from reachable effects ([4686361](https://github.com/empty-sekai/ournotes-deck/commit/4686361bb60c5703a3220111e5fbddd08ef60640))

### Documentation

- **search:** State complete proof obligations and audit boundaries ([f79fd04](https://github.com/empty-sekai/ournotes-deck/commit/f79fd0401b90c0a5128c12607f84ca6811296846))
- **search:** Specify bound preparation and LUCK refinement methods ([cf1049a](https://github.com/empty-sekai/ournotes-deck/commit/cf1049a6945f808400b77f09fc8c9f2549690545))
- Expose roadmap and add optional parallel search ([bca2fdd](https://github.com/empty-sekai/ournotes-deck/commit/bca2fdde8cf45f440243d1c408889135bc538c23))

### Tests

- **search:** Validate canonical results across live objectives ([6f34680](https://github.com/empty-sekai/ournotes-deck/commit/6f34680116c7c4c28e1d37730cc6f3378246224a))
- **search:** Extend canonical coverage to scene payoff contracts ([acf41e1](https://github.com/empty-sekai/ournotes-deck/commit/acf41e15b0df334f50e21cecb2699d31aac60722))
- Bind frame matrices to calculation source and coverage ([910eb8f](https://github.com/empty-sekai/ournotes-deck/commit/910eb8f6cfd419ea94909880f200b2083fe5717c))

### CI

- Verify packaged wasm on pull requests ([#39](https://github.com/empty-sekai/ournotes-deck/issues/39)) ([e900dbc](https://github.com/empty-sekai/ournotes-deck/commit/e900dbce962da714aea7c8b303b8f2a47d5cad37))

## [0.0.3](https://github.com/empty-sekai/ournotes-deck/compare/v0.0.2...v0.0.3) - 2026-10-06

### Features

- **sim:** Expose the SHA-256 of the model sources ([f0ae9fb](https://github.com/empty-sekai/ournotes-deck/commit/f0ae9fbf7e3e4aed048dcf8b34dc84e8591a1bb5))

### Bug fixes

- **search:** Bound retained network score snapshots ([ffde1b2](https://github.com/empty-sekai/ournotes-deck/commit/ffde1b2b5d14afa47f6c980530134abad1f2d4b9))

### CI

- Run each commit once and the diagnostics tests in parallel ([8cdbd03](https://github.com/empty-sekai/ournotes-deck/commit/8cdbd03554a8d034dbd380c180ba1ebd2c51f1a0))

## [0.0.2](https://github.com/empty-sekai/ournotes-deck/compare/v0.0.1...v0.0.2) - 2026-10-06

### Bug fixes

- **sim:** Cancel unchanged notes between LUCK rank snapshots ([25c7a55](https://github.com/empty-sekai/ournotes-deck/commit/25c7a5525177d103ac4de839a646b6ef43d5d5f1))

### Performance

- **sim:** Enclose LUCK factor states with a binary32 endpoint replay ([c50e783](https://github.com/empty-sekai/ournotes-deck/commit/c50e7830fb52e75ce953e6b9bb863a05c74cfe33))
- **sim:** Pair LUCK replay undos with the last execution of the latest frame ([8d96708](https://github.com/empty-sekai/ournotes-deck/commit/8d96708fcd367f070e038f6b9b8e9b04b50d5c82))
- **sim:** Reuse certified LUCK lottery curves across equal recordings ([7c2bc93](https://github.com/empty-sekai/ournotes-deck/commit/7c2bc93d725c7359d0f78a91046de2541182b054))
- **sim:** Reuse frame buffers in the LUCK recording pass ([648342c](https://github.com/empty-sekai/ournotes-deck/commit/648342cd8ffd1753dcdf422559d8135348ea2f1c))
- **search:** Cap score and life targets by the final life of each order ([d2c4fee](https://github.com/empty-sekai/ournotes-deck/commit/d2c4feebeed5fd4842c5e9ca39903e86eb8a866b))
- **search:** Bound event bonuses by the largest bonus of any team ([c5dc074](https://github.com/empty-sekai/ournotes-deck/commit/c5dc074824e3cf25ab021fe8e148c4cf298e19cb))
- **search:** Drop composition nodes whose teams cannot reach the final life ([31ca2a3](https://github.com/empty-sekai/ournotes-deck/commit/31ca2a37698fdcae259ebd853b43d6f94eea6a86))

## [0.0.1](https://github.com/empty-sekai/ournotes-deck/releases/tag/v0.0.1) - 2026-10-06

### Features

- One `ournotes-deck` CLI entry point, with JSON recommendation under the `recommend` subcommand; native and WASM release packages with SHA-256 checksums.
- Snapshot recommendation in wasm and search progress reports ([7112cc1](https://github.com/empty-sekai/ournotes-deck/commit/7112cc13e5855ac8636ce503ff21e637f2a9ce64))
- Rank played teams by their mean payoff over the 120 performance orders ([dadc22f](https://github.com/empty-sekai/ournotes-deck/commit/dadc22f558714dcaaf7704c01f8568f2fe04558a))

### Bug fixes

- **search:** Keep evicted incumbents from closing bounded payoff search ([a2ad989](https://github.com/empty-sekai/ournotes-deck/commit/a2ad9895a4ca4b426f64f1a4fc53850a8097e009))
- **search:** Preserve canonical ties before truncating power frontiers ([d115fd5](https://github.com/empty-sekai/ournotes-deck/commit/d115fd5cdc5ea5af87959cbda193b785d5ec07c8))
- **search:** Retain safe peak bounds for sustained combo effects ([a999855](https://github.com/empty-sekai/ournotes-deck/commit/a999855f9ec524b68fd695ca9a8dcff16b9ff49b))
- **search:** Require whole-point stats before additive power bounds ([9499e13](https://github.com/empty-sekai/ournotes-deck/commit/9499e132783baf8886bd0dd8687ca57e6946ff77))
- **harness:** Include workspace crates in source manifests ([ebc060d](https://github.com/empty-sekai/ournotes-deck/commit/ebc060d2945de94f8ffc84d3a509b7cbf7bec421))
- **api:** Carry account preparation time into the search budget ([2123238](https://github.com/empty-sekai/ournotes-deck/commit/2123238b751c259614c9f147cd4f142f070bab4b))
- **api:** Preserve whole-domain bounds when finalizing recommendations ([316b30f](https://github.com/empty-sekai/ournotes-deck/commit/316b30f3b9beb90e138d414c2a32a0c1c8b84a90))
- **search:** Refine ambiguous LUCK rankings with bounded complete laws ([6d85a4a](https://github.com/empty-sekai/ournotes-deck/commit/6d85a4ac41b1ecbc77e179fa567d07b10e509aa6))
- **sim:** Accept slash-separated master dates ([86a360b](https://github.com/empty-sekai/ournotes-deck/commit/86a360b8889f61579c52b99a381654c926815d45))

### Performance

- Hash simulator maps with FxHash and reuse per-frame buffers ([494b55d](https://github.com/empty-sekai/ournotes-deck/commit/494b55d5b065573109afcb5df999691fac307a8f))
- **search:** Read Gekisou COMBO ramp windows at the combo count bound ([4e56e7c](https://github.com/empty-sekai/ournotes-deck/commit/4e56e7c4e0a74c5367ea46f39952afacc02968ce))
- **search:** Tighten Gekisou combo ramps, gates and trigger windows ([618773a](https://github.com/empty-sekai/ournotes-deck/commit/618773ab06da8a79b42c5e3e298531d9a965323a))
- **search:** Bound Gekisou nodes by the carrier lists of the slots to fill ([ba07b25](https://github.com/empty-sekai/ournotes-deck/commit/ba07b254c1939f95f2db2f143dd9a0b052075275))
- **search:** Couple powers and gains in the carrier split and trim its table costs ([c2432ab](https://github.com/empty-sekai/ournotes-deck/commit/c2432ab0cc9ac33df5d2dc0ded7d38f0e25a58ce))
- **search:** Compile carrier split tables as compact rows with early-stopping top-k ([b60eb92](https://github.com/empty-sekai/ournotes-deck/commit/b60eb92df7b7a582da609cc24f9273797f57a16f))
- **search:** Cache order steps and check pair-level order steps in the joint search ([77e8a0b](https://github.com/empty-sekai/ournotes-deck/commit/77e8a0bbe276e322c254d714def62a136906bb46))

### Documentation

- Move the compact search layout to later ([c34fb23](https://github.com/empty-sekai/ournotes-deck/commit/c34fb230c4d44d7dd84e0469945eacdd86e1b4b2))
- Add a changelog generated by git-cliff ([5fe5218](https://github.com/empty-sekai/ournotes-deck/commit/5fe5218e896a370a4d37288a56009ec2c1c4ea77))

### CI

- Pass the latest stable clippy and move to actions/checkout v7 ([6e03470](https://github.com/empty-sekai/ournotes-deck/commit/6e034704be1501ba9ae2d73622b0d4c70d1abeac))

### Miscellaneous

- Initial commit ([e27d289](https://github.com/empty-sekai/ournotes-deck/commit/e27d289d549aff74955977659e1bee5c72d6a4f5))

