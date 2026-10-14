# ournotes-sim

The deck model of [ournotes-deck](https://github.com/empty-sekai/ournotes-deck) for Python: the chart statistics of
the Rust crate `ournotes-sim` on a deck data document, measured in parallel in native code.

```sh
pip install ournotes-sim
```

Wheels are built for CPython 3.11 and later (one abi3 wheel per platform) on Linux (x86_64, aarch64; manylinux
2.28), Windows (x64) and macOS 11 or later (x86_64, arm64).

## Use

```python
import json
import ournotes_sim

text = open("deck-data.json", encoding="utf-8").read()       # nnnotes.deck-data/1
stats = json.loads(ournotes_sim.chart_stats(text))            # ournotes-deck.chart-stats/3
print(ournotes_sim.info())
```

| name | does |
|---|---|
| `chart_stats(data, seeds=None, workers=None, aptitude=True)` | the chart statistics document (JSON text) of a deck data document (JSON text); `seeds`: replay seeds (default 8), `workers`: threads (default: the available parallelism), `aptitude`: include the single Gekisou skill shapes. Raises `ValueError` for invalid data or a failed expectation check |
| `chart_stats_cached(data, seeds=None, workers=None, aptitude=True, *, cache_dir)` | `chart_stats` with the simulation programs kept in `cache_dir`, and the cache's counters (`requests`, `hits`, `computed`, `writes`, `invalid`, `bytes`); the document is the same as `chart_stats`'s |
| `info()` | the model: `name`, `version`, `source`, `commit` (the source commit of a release build, `None` for other builds), `sourceSha256` (the SHA-256 of the ournotes-sim sources: equal for builds that run the same model), `dataFormat` and `format` |
| `MASTER_TABLES` | the master tables a deck data document holds (each one is required, zero rows are allowed) |

Every version of the package carries one model, the ournotes-sim of the same release. The package contains no game
data: deck data documents are made from each user's own master data, for example with
[nnnotes](https://github.com/MetaSekaiLab/nnnotes).

## Building

```sh
pip install ./python[test]      # from the repository root: builds the extension module with maturin
pytest python/tests
```

## Licence

MIT OR Apache-2.0.
