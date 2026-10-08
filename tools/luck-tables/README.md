# Incremental real-chart LUCK response tables

This tool builds reusable **probability-response experiments**, not full-team search certificates. It keeps the original chart, difficulty, play stream, request, owned inventory and all 120 performance-order semantics. Base/single-skill coverage is distinct from combination coverage; a fitted composition is not an exact nominal score law.

The native `luck_response` generator resolves dependencies and produces certified-DP probability intervals. `pipeline.py` selects declared skill levels, calls native planning without DP, schedules missing entries, and stores content-addressed results. Monte Carlo comparison is disabled by default (`--mc-runs 0`). The independent analysis tool `analyze_responses.py` reports compression and composition errors; its measurements do not change the generator's probability contract.

## Original real corpus

The default `--corpus-mode real-benchmark` requires the complete PR40 `full48` manifest before any explicitly named generation selection. It discovers LUCK from the original master mission rows, not case names. The source remains `tools/search-harness/fixtures/full48/source.json`; dataset identity is verified against both the manifest and owned snapshots.

Its 36 LUCK requests use six **Expert** charts and ten chart/play contexts: short, dense, and best/Miss variants of long, mixed-short, sparse and mixed-long. The three complete inventories select 18 distinct LUCK chain source/level pairs, with at most 20 formation variants. A base plus every selected variant at each of five positions gives **650 base/single entries** after shared-context deduplication. This is the scope of the current corpus, not coverage of every game chart, difficulty, or skill combination.

`newcomer-combinations.json` explicitly declares all 26 position subsets of zero through three copies of Gekisou skill 7 level 1 on five positions, for the original short/long newcomer requests. Those two requests require 52 entries including their base/single entries. Appending the specification to all 650 single entries adds 40 entries, rather than duplicating the 12 existing ones. No full combination Cartesian product is generated.

Other charts or difficulties can be supplied through `--corpus-mode declared-manifest`. This accepts the same `ournotes-deck.search-benchmark/1` format with any nonempty unique case list. Every case must still supply complete original data/snapshot/request inputs and matching dataset identities. The first planner supports Mission requests; it refuses unsupported scenes explicitly. This mode does not claim those additional inputs have already been measured.

## Generate and resume

Build once, materialize the unchanged full corpus, then plan without running DP:

```sh
cargo build --release --locked --manifest-path tools/search-harness/Cargo.toml --bin benchmark_prepare --bin luck_response
python3 tools/search-harness/matrix.py prepare --suite full48 --cases all --no-build --cache-dir work/datasets --out work/full48-inputs
python3 tools/luck-tables/pipeline.py plan work/full48-inputs/benchmark.json --generator tools/search-harness/target/release/luck_response --output work/luck-plan.json --cases short-newcomer-score,long-newcomer-score --combinations tools/luck-tables/newcomer-combinations.json
python3 tools/luck-tables/pipeline.py run work/luck-plan.json --generator tools/search-harness/target/release/luck_response --store work/luck-tables --batch-size 128
python3 tools/luck-tables/pipeline.py compact work/luck-plan.json --generator tools/search-harness/target/release/luck_response --store work/luck-tables --output work/luck-compact
```

For all 650 base/single entries, omit `--cases` and `--combinations` when planning. Plans merge only identical complete **native shared dependency descriptors**. A changed original input file after planning is rejected; replan first. A new input dataset can reuse old objects only if the native dependency descriptors and algorithm identity remain identical.

Each invocation processes a batch from one context. The default 128-entry batch keeps the original corpus's largest 101-entry context together, preserving native controller-cache reuse. Smaller batches trade that reuse for more frequent resume points. Successful per-entry JSON receipts and the generator's verified lossless/U16/U24/U32 archives are immutable by SHA-256. Curves live once in each native report; entry receipts reference them. Complete context dependencies are stored once by content hash, not copied into all 650 entries. The index is atomically replaced after each completed entry. An interrupted or failed batch stays explicitly unresolved; the next invocation retries it and preserves earlier successes. Corrupted or missing successful objects/archives are regenerated. An unchanged second run calls no native generation for completed entries. Structural `unsupported` results are retained as failures; `--retry-failures` retries them. Capacity/error probability results are always retried. A run exits nonzero unless every selected **DP entry** succeeds. A failed codec mode remains explicit in the native report but does not discard completed DP work; `compact` retries packing the saved entries without DP and separately reports complete archive publication.

The local plan is an execution receipt with original input paths. For another machine, materialize the same declared inputs and replan; the portable store uses relative paths and content identities. `objects/`, `reports/`, `contexts/` and `blobs/` must travel together. The Actions cache is an accelerator, while the uploaded artifact preserves results and unresolved statuses. No workflow commits generated tables to Git or posts comments.

`compact` first verifies every selected object/report/archive hash. Missing or failed entries produce an explicit incomplete manifest and prevent publication of a supposedly complete table. Once coverage is complete, it merges old and newly generated entries per context, calls native **planning only** to reconstruct the exact complete archive identity, then native `pack` for lossless/U16/U24/U32. Every key is decoded and checked by the pack command. No DP or Monte Carlo runs during compaction. Its portable `manifest.json` lists all covered task/entry keys and each random-access archive's hash, size and relative path; a lookup consumer can open one archive for the entire context instead of loading individual batch sidecars.

## Incremental identity

Dataset SHA, original request/snapshot hashes and source paths are provenance, not the sole cache key. The native generator exports:

- A shared descriptor: resolved chart/play/setup/frame clock and shared native rules, complete score-shape/probe catalogue and row-to-shape mapping, actual neutral/probe dependencies, and algorithm schema.
- Each job's descriptor: ordered entry keys, selected skill rows at their actual levels, and referenced trigger/condition/release/reset, target and cumulative definitions.
- `sharedFingerprint` for incremental reuse. The archive's `context.fingerprint` additionally binds **all entries in that archive**, so it is intentionally not the per-job cache key.

Changing one skill level rebuilds jobs that depend on it; changes to an actual neutral/probe or shared lottery rule affect their dependent contexts. New members or Snaps that use existing skills only add provenance or needed jobs. A new unrelated chain writer adds its own jobs without invalidating old entries when it leaves the shared score shapes, neutral and probes unchanged. New score-probe catalogue behavior can invalidate the shared context. Every requested writer still receives independent native catalogue validation. Unrelated dataset facts can therefore change the whole-file SHA without changing a job, and a new combination leaves the old single entries reusable. The initial source digest conservatively covers all crate Rust source, manifests/lockfiles and the native generator: even an unrelated ordinary-score code edit can invalidate the tables. This over-invalidation is intentional until a narrower algorithm dependency contract is established. Actual dataset-change experiments are separate from these dependency contracts.

Planning verifies the binary's embedded simulation source hash against the selected source tree, using the same length-prefixed hash as `ournotes-sim/build.rs`. The plan also records the actual generator executable hash; running with a different executable requires replanning, even on a full cache hit. The executable hash is provenance rather than a task key, so identical semantic sources can reuse data across builds. Search/harness source identity is not yet embedded in the executable: the CI same-checkout build provides that outer guarantee, and local callers must likewise supply the source used to build their generator. Use one writer per store.

Entry order, multiplicity, owner performance position, source type, level and formation variant are retained. The Python selector does not grant native equivalence or silently drop missing definitions. All native planning results must cover every requested entry exactly once.

## Small engineering checks

```sh
python3 -m unittest discover -s tools/luck-tables -p 'test_pipeline.py'
```

These checks use small JSON fixtures and a fake native transport to test selection, dependency invalidation, corruption, cancellation and resumability. They do not measure synthetic solver performance. Native correctness, actual response curves, complete 120-order comparisons and original real-request performance remain separate validation work.
