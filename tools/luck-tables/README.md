# Incremental real-chart LUCK response tables

The [recording-family extension](../../docs/luck-response-family-reuse.md) adds bounded minimum-family
recording reuse, opt-in Miss-gauge canonical identities, complete-chart family validation and
independent same-binary dictionary auditing. The [2026-10-09 audit](../../docs/luck-response-family-audit.json)
records all 365,000 minimum-family parameter jobs over 292 natural LUCK charts: 292 grid recordings,
2,364 merged conditional programs and a 744,752 B U24/XZ dictionary. The separate complete base/single
dictionary is 4,599,344 B in U24/XZ on that source. The extension includes runnable commands,
original PR40 input comparisons and the scope of each table. Historical measurements below retain
their explicitly linked earlier source.

This tool generates reusable joint LUCK probability responses from actual published charts and master skills. The primary workflow covers the **complete TW and JP chart catalogues**, including every difficulty, and identifies interacting skill combinations through their complete native controller programs. It does not estimate every entry by repeatedly simulating random seeds.

The nominal DP probability intervals and archive score replay remain diagnostic prediction data. They do not authorize pruning or certify a complete full-team Top-K ranking. Each supplied request retains its original chart, difficulty, play stream, inventory and all 120 performance-order semantics. A whole-catalogue anchor is explicitly an independent kernel construction from real master cards, not an invented owned inventory.

The [complete catalogue audit](../../docs/luck-response-catalogue-audit.json) accounts for **692/692 regional chart/difficulty records**. Its 350,692 base/single labels required 14,236 native DP propagations across chart jobs and produce **7,216 distinct stored programs** after cross-region merging. The complete U24 dictionary occupies **16,429,514 bytes** as index plus unique blobs, or **4,591,788 bytes** in a deterministic XZ bundle. These measurements belong to [the completed catalogue run at `de589e3`](https://github.com/empty-sekai/ournotes-deck/actions/runs/37820020030).

The newer [conditional interaction audit](../../docs/luck-response-conditional-audit.json) identifies **1,250 full-capacity skill combinations as 27 conditional programs in each of three real chart contexts**. Two warmup jobs generate the complete 27-program family; new levels and multiplicities then query it with zero additional DP. The three families occupy **27,104 bytes together in U24/XZ**, with their own source and coverage recorded separately from the complete catalogue. The [research and measurement record](../../docs/luck-response-tables.md) explains both algorithms, compression scope and earlier limited experiments.

## Acquire and generate the complete published catalogues

The acquisition tool reads the publication endpoints used by [bdon.moe](https://bdon.moe), then follows the published manifest and verifies the pinned deck-data bytes:

```sh
cargo build --release --locked --manifest-path tools/search-harness/Cargo.toml --bin luck_response
python3 tools/luck-tables/catalogue.py acquire --region all --output work/catalogue
for region in tw jp; do
  python3 tools/luck-tables/catalogue.py anchor "work/catalogue/$region/deck-data.json" \
    --output "work/catalogue/$region/anchor.json"
  python3 tools/luck-tables/catalogue.py manifest "work/catalogue/$region/deck-data.json" \
    "work/catalogue/$region/anchor.json" --region "$region" --output "work/catalogue/$region/inputs"
  python3 tools/luck-tables/stream_runner.py "work/catalogue/$region/inputs/benchmark.json" \
    --generator tools/search-harness/target/release/luck_response --source . \
    --output "work/results/$region"
done
```

The full published input inventory verified on 2026-10-08 is:

| Region | Songs | Charts, all four difficulties | Natural LUCK charts | Other charts, explicitly accounted for |
| --- | ---: | ---: | ---: | ---: |
| TW | 87 | 348 | 148 | 200 |
| JP | 86 | 344 | 144 | 200 |
| Total regional records | 173 | 692 | 292 | 400 |

Each dataset contains 140 actual writer source/ID/level keys. Formation branches expand these to 240 catalogue keys. A base response and every key at every position require 1,201 labelled jobs per applicable chart: **350,692 base/single jobs** across the 292 natural LUCK charts. The manifest checks exact equality with the master music/score/chart-note sets. It does not omit lower difficulties or select a sample of songs. The 400 other charts retain their actual missions and are `notApplicable`; generation never changes them into fabricated LUCK charts. A later publication is inventoried again instead of trusting these fixed counts.

Every applicable chart independently validates the full requested skill catalogue against the native compiler. The default play profile is theoretical best; arbitrary miss streams remain separately keyed inputs, not implied coverage. All 1,201 labels are retained in the per-chart audit, while a probability curve is stored only once per distinct native program. The report and each lossless/U16/U24/U32 archive must be complete before that chart is reusable.

`stream_runner.py` accepts `--shard-index I --shard-count N`. The current versioned partition is **`scoreId % N == I`**, named `score-id-modulo/1`; adding a song does not move existing scores between shard caches. A shard's `complete` flag refers only to that shard. The Actions workflow shares one pinned input bundle and one native binary across **2 regions × 8 runners**, verifies each restored bundle, and audits every shard and every blob before claiming whole-catalogue completion. The earlier completed `de589e3` run used the ordinal position in the sorted catalogue; its historical audit retains that original rule. The new cache-key prefix deliberately starts a new stable partition.

An unchanged chart with valid input and archive bytes needs no native call. When the dataset/snapshot SHA changes, the runner obtains a fresh native `plan` for every requested base/single job. It reuses a chart only when the current input/spec provenance, complete native catalogue, shared descriptor, full response context and algorithm identity match the verified old generation. The old report keeps its original generation provenance; separately hashed `resumePlan` files and `reusedThroughPlan` bind that result to the new pin. Aggregate validation checks this connection independently. A selected dependency or catalogue-key change regenerates the affected whole chart; a corrupt chart archive also triggers a chart rebuild. The query dictionaries described below repair individual requested programs more finely.

The workflow runs for relevant pull requests and pushes, supports manual execution, and refreshes the published inputs daily at **13:17 UTC only after this workflow is on the default branch**. Following a successful full audit, `analyze_programs.py` merges verified program addresses and publishes separate gzip/XZ dictionaries as `luck-catalogue-program-dictionaries`. Raw shard evidence is retained for 7 days; the deployable dictionaries and full audit are retained for 30 days. A separate `interactions` job checks minimum-basis families on the pinned TW short/long Expert catalogue requests and JP 10000300 Easy, using the same verified binary. Its grid, independent original-DP comparisons, warm reuse and nonminimum control evidence are uploaded as `luck-real-multiskill-validation` for 30 days. These retention periods apply to the current workflow, not retroactively to historical artifacts. Generated tables remain in cache/artifacts. The workflow has read-only repository permissions and does not commit tables or post messages.

## Query interacting skills without enumerating all combinations

Create a native `SPEC` containing uniquely named jobs and the actual ordered entries:

```json
{
  "jobs": [
    {"name": "pair", "entries": [
      [{"source":"gekisouSupport","id":66,"level":5,"matched":true},1],
      [{"source":"gekisouSupport","id":71,"level":3,"matched":true},1]
    ]}
  ]
}
```

Each entry is `[source key, performance position]`. Multiplicity, order, native source capacity and formation compatibility matter. This is an isolated real-skill kernel; a physical member/Snap deck must independently pass its own legality checks for score prediction.

```sh
python3 tools/luck-tables/programs.py DATA SNAPSHOT REQUEST SPEC \
  --generator tools/search-harness/target/release/luck_response \
  --store work/program-dictionary --output work/query --mode u24
```

The query performs these operations:

1. Compile each requested combination through the original native recorder, without DP. Unknown mechanisms, invalid holders or unsupported observations remain explicit failures.
2. Compare complete ordered native identities in process. Equal SHA-256 alone never establishes a native alias; the entire retained identity must compare equal.
3. Check the persistent dictionary by native program address and verify the recorded source, quantization, file length and checksum.
4. Propagate one representative of each **missing** complete program, pack it, and atomically update the dictionary. An existing single or mixed program can be reused by another differently labelled combination.
5. Materialize only the requested keys into `responses.onlrsp`. Optional `--decks DECKS.json` runs the existing predictor over every one of the 120 orders of each supplied legal deck.

Repeat with a different output directory to observe a warm dictionary. `--no-generate` performs native identification and reports missing programs without DP. `--seed-index FILE` imports a verified per-chart program index from a catalogue artifact. The native materializer rechecks each archive's internal context and codec. Corrupt response bytes are repaired only when that program is requested. Concurrent writes to one local dictionary are refused through a POSIX lock; CI uses separate stores.

The deployable dictionary contains **program fingerprint → response blob**, without a global combination-to-response index. Query identification still has a cost, and a new program still needs DP. Distinct interacting programs can grow combinatorially in the worst case; this implementation does not claim a universal polynomial bound. It avoids requiring that the Cartesian product be generated before a new combination can be queried.

All five positions may contain LUCK writers. In that case the native generator explicitly validates a virtual direct-7021 observer, keeps every original writer, and records the observer mode in the complete identity. No writer is replaced to make room for a probe. Unsupported score-reader conditions still refuse generation. The original held-probe API preserves its existing semantics.

Low-level commands are also available:

```sh
luck_response DATA SNAPSHOT REQUEST IDENTIFY_OR_PROGRAMS_SPEC REPORT
luck_response program-pack PROGRAM_REPORT u24 DIRECTORY
luck_response program-materialize IDENTIFICATION PROGRAM_INDEX MATERIALIZED_TABLE
luck_response pack MATERIALIZED_TABLE u24 ARCHIVE
```

`identify` compiles only; `programs` propagates one representative per complete identity directly from its original compiled object. A materialized query is bounded to 65,536 labels and 32 MiB of decoded response ownership; it is not a mechanism for constructing a giant all-combination alias table. Importing a response supplies no native proof capability.

## Conditional basis for interacting minimum guarantees

`basis.py` addresses a more specific source of combination growth. For independent start-minimum writers with thresholds `a_i` and native probabilities `q_i`, let `M` be the maximum guarantee newly supplied at this start, or zero if none trigger. Its cumulative probability is

$$
F(m)=\Pr(M\le m)=\prod_{i:a_i>m}(1-q_i).
$$

The native action updates its state to `max(existing minimum, M)`, retaining any carried minimum. The rest of the controller can therefore consume a distribution over newly supplied guarantees rather than an enumeration of writer names, levels and multiplicities. `canonicalStartMinimum: true` is an opt-in exact-CDF quotient for contiguous native minimum actions; it preserves action barriers and conservatively retains the original block when exact bounded arithmetic cannot represent it.

The conditional basis goes further. For a **fixed remaining native controller program**, condition the complete live on one minimum value at each of its at most three starts, propagate those conditional programs once, and combine their responses with the positive product weights. Current master minimum values use at most three choices per start, giving at most **27 whole-live terms**; the general four-choice implementation caps the allowance at **64**. Every term retains the full original live, including COMPLETE/FINISH tails and all other skill actions. This does not assume independent reset states between ranges, and it does not add independently generated single-skill curves.

The basis computes its weights by four-state forward probability propagation with outward intervals. It does not need the optional exact-CDF rewrite's bounded denominator, so that rewrite's arithmetic limit does not impose the same limit on interacting minimum writers.

```sh
python3 tools/luck-tables/basis.py DATA SNAPSHOT REQUEST \
  tools/luck-tables/fixtures/start-minimum-cross-multiplicity.json \
  --generator tools/search-harness/target/release/luck_response \
  --store work/program-dictionary --output work/basis-query --mode u24
```

The source/mode dictionary is shared with `programs.py`; native conditional identities use their own domain. `basisIdentify` compiles the original jobs and supplies each term address, interval weight and start choice. `basisPrograms` receives the same full jobs and only the missing term addresses. The query verifies all mappings, weights, source/context/input provenance and the exact propagation count before importing any generated terms. It then calls native `basis-materialize` to mix the imported responses with outward arithmetic and packs only the requested response keys. `--no-generate`, `--seed-index` and optional `--decks` have the same roles as in `programs.py`.

The query preserves the caller's spec file, forces `verifyBasis: false`, `basisReconstruct: false`, `mcRuns: 0` and `scoreSamples: 0` in its working specs, and defaults `basisMaxTerms` to 64. It does not reconstruct a repeated original-job curve while generating dictionary terms. The native diagnostic mode can separately enable `verifyBasis: true` to compare the reconstructed joint response with an independently propagated original program; that extra work is explicitly counted.

For one previously unseen combination, eight or 27 conditional propagations can cost more than one direct propagation. Use this basis for families that vary minimum probabilities or multiplicities while sharing their other controller actions. Different gauge, speed, miss-trigger, LIFE or timing behavior can require different conditional programs. The 27/64 bound applies to one admitted remaining controller and is not a bound on every possible multi-skill combination.

### Measured families and reproduction

The [conditional audit](../../docs/luck-response-conditional-audit.json) uses the native source at [`753ad76`](https://github.com/empty-sekai/ournotes-deck/commit/753ad7624d1f01584595290a6af0f72bb4c98029), with simulation hash `0164248e95314dc38d8154f7fa93f7ec0f7e445697e191a7289097ec89d830d6`, distinct from the older complete-catalogue run. On each original PR40 short/long newcomer request and JP 10000300 Easy, the native identifier retains all `2 × 5^4 = 1,250` grid jobs and maps them to 27 whole-live terms. Every job fills five main and ten support slots with actual GK8, GS66 and GS71 master sources. The grid varies GS66 levels on four holders and two compatible formation patterns; its remaining gauge-speed controller stays fixed. These are real master-parameter kernels, with owned-deck checks performed separately below.

| Actual context | Cold warmup, 2 jobs and 27 new DP programs | 12 unseen-level jobs, 0 new DP | 6 new combinations, 0 new DP | 12 repeated jobs, 0 new DP |
| --- | ---: | ---: | ---: | ---: |
| PR40 short newcomer | 3.243 s | 0.236 s | 0.156 s | 0.215 s |
| PR40 long newcomer | 3.134 s | 0.270 s | 0.184 s | 0.265 s |
| JP 10000300 Easy | 1.513 s | 0.273 s | 0.165 s | 0.243 s |

Each timing is one complete query phase, including native identification, dictionary checks and response materialization; it excludes score prediction and the separate 1,250-job grid-identification phase. All reuse queries disable generation. Six independent original-program DP comparisons per context pass: **18 comparisons, 13,056 joint buckets, no disjoint intervals**, matching probes/masks and a maximum endpoint difference of `1.1102230246251565e-14`. The exact-CDF witness also reduces eight requested jobs from four original programs to two canonical programs in each context. Replacing one fixed GK8 level 3 with actual GK20 level 3 changes the remaining controller and correctly requires 27 new programs.

Only the 27-program warm family from each context is included in this compression measurement:

| U24 family | Unique programs | Index + blobs | XZ-6 bundle |
| --- | ---: | ---: | ---: |
| PR40 short newcomer | 27 | 153,087 B | 9,704 B |
| PR40 long newcomer | 27 | 143,673 B | 9,400 B |
| JP 10000300 Easy | 27 | 115,980 B | 9,620 B |
| Three families merged | 81 | 412,080 B | 27,104 B |

All four encodings were packed without further DP. The three separate families and merged dictionary were each measured with gzip/XZ/Zstandard: **all 48 compressed bundles round-trip to their exact TAR SHA and byte length**. The measurement excludes job aliases, nonminimum control families and the full catalogue. Compression effectiveness depends on blob order and codec window; a merged gzip bundle can be larger than the sum of separate bundles. The research record includes all merged codec sizes.

After preparing the complete original PR40 corpus with the commands below and acquiring the JP catalogue above, reproduce the actual family experiment with:

```sh
python3 tools/luck-tables/validate_minimum_basis.py work/full48-inputs/benchmark.json \
  --case short-newcomer-score --case long-newcomer-score \
  --generator tools/search-harness/target/release/luck_response \
  --output work/minimum-basis-pr40 --mode u24
python3 tools/luck-tables/validate_minimum_basis.py work/catalogue/jp/inputs/benchmark.json \
  --case jp-10000300-easy --generator tools/search-harness/target/release/luck_response \
  --output work/minimum-basis-jp --mode u24
```

Use empty output directories. The validator derives its specs from the actual supplied master rows, preserves and hashes all original inputs, identifies the full grid, generates the warmup, tests unseen levels and multiplicities with generation disabled, independently compares six original programs, and runs the changed-controller control. To inspect only those generated specs, use `python3 tools/luck-tables/make_minimum_basis_specs.py --data DATA --out work/minimum-specs`. The CI `interactions` job applies the same validator to the complete-catalogue anchors; those TW snapshots are distinct from the original PR40 newcomer snapshots used in the measured table.

## Measure compression and validate real interactions

The checked-in [`fixtures`](fixtures) contain actual master-source parameter cases: a mixed smoke set, all 240 catalogue variants with additional writers, four-kind position permutations, five occupied holders including all 15 source slots, an exact minimum-CDF witness, and an original PR40 veteran deck. Parameter-kernel fixtures do not assert that one player owns every level or combination. Physical-deck validation uses the unchanged original snapshot and native legality checks.

To inspect the complete program dictionary and retain deterministic deployable bundles:

```sh
python3 tools/luck-tables/analyze_programs.py --root work/results \
  --inputs work/catalogue --output work/program-analysis.json \
  --bundle-dir work/deployable --compression gzip9,xz6
```

The output directory must be separate from input and result roots. `--compression gzip9,xz6,zstd9` also measures the optional installed `zstandard` package. `--report FILE` adds an explicitly named native program report. Analysis separates raw reports/job labels from deployable index/blob bytes, detects conflicting content for one source/program address, and records compression hashes. Unpack `dictionary.tar.xz` or `dictionary.tar.gz` before seeding its contained `program-index.json`; native random access operates on the extracted blobs. Compression files are portable packages, not directly readable native archives.

The algebra analyzer derives its probabilities from the actual master rows, including native binary32 rounding:

```sh
python3 tools/luck-tables/analyze_start_operators.py DATA work/start-operators \
  --phase-life 1000 --max-main 5 --max-support 5 --enumeration-limit 20000
```

It reports its phase-LIFE assumption, exact source classes, enumerated degrees and degrees left unenumerated by the limit. Its output is algebraic analysis, not native score or performance validation. Start gauge and miss-triggered gauge effects remain separate mechanisms; their native integer rounding cannot generally be replaced by adding raw percentages.

For the legal original veteran deck with five main LUCK writers and one LUCK Snap, first prepare the complete original corpus, then supply its unchanged data, veteran snapshot and either original short/long veteran request:

```sh
python3 tools/luck-tables/validate_interactions.py DATA ORIGINAL_VETERAN_SNAPSHOT REQUEST \
  tools/luck-tables/fixtures/veteran-five-decks.json \
  --generator tools/search-harness/target/release/luck_response \
  --output work/veteran-validation --mode u24
```

Use a new output directory for each chart. This command computes an independent full native nominal reference for **all 120 original orders**, extracts their actual ordered LUCK keys, performs cold/warm program-dictionary queries, and compares joint intervals plus score-at-mean proxies. Add `--backend basis` to validate the conditional dictionary through the same original native reference and every physical order; the default is `--backend programs`. `--reference FILE` reuses only a source/input/spec-bound reference. The report preserves actual differences rather than converting them to a pass. The completed direct-program short/long comparison covers 240 orders with joint enclosures and zero observed U24 score-proxy error; the [interaction audit](../../docs/luck-response-interactions-audit.json) records those inputs and timings.

The [conditional audit](../../docs/luck-response-conditional-audit.json) also validates the same original owned deck through `--backend basis`, with all 240 original native references regenerated under the newer `0164248…` source. Each chart's 120 orders identify **eight conditional programs**: eight DP calls cold and none warm. All joint intervals enclose the reference endpoints, joint/weighted intervals overlap, and probes, transition masks and range moments agree. Every U24 score proxy matches exactly in both cold and warm queries; the quantized probability endpoints need not be bit-identical.

| Original request, 120 orders | Cold basis query and predictions | Warm basis query and predictions | Separate current-source native reference |
| --- | ---: | ---: | ---: |
| Short veteran | 3.752 s | 1.990 s | 26.29 s |
| Long veteran | 5.321 s | 2.408 s | 30.57 s |

The earlier direct-program path used one DP for each of these decks, while this conditional path generates eight terms. That is the cold-family tradeoff discussed above. The timings come from different source revisions and are separate observations, not a controlled speedup ratio. These physical-deck checks retain the original inventory and all performance orders; the 1,250-job parameter grid has a separate purpose and scope.

## Legacy PR40 entry-pipeline corpus

The default `--corpus-mode real-benchmark` requires the complete PR40 `full48` manifest before any explicitly named generation selection. It discovers LUCK from the original master mission rows, not case names. The source remains `tools/search-harness/fixtures/full48/source.json`; dataset identity is verified against both the manifest and owned snapshots.

Its 36 LUCK requests use six **Expert** charts and ten chart/play contexts: short, dense, and best/Miss variants of long, mixed-short, sparse and mixed-long. The three complete inventories select 18 distinct LUCK chain source/level pairs, with at most 20 formation variants. A base plus every selected variant at each of five positions gives **650 base/single entries** after shared-context deduplication. This describes the earlier, limited entry-pipeline experiment; the complete published catalogue workflow above supersedes it for chart and master-skill coverage.

`newcomer-combinations.json` explicitly declares all 26 position subsets of zero through three copies of Gekisou skill 7 level 1 on five positions, for the original short/long newcomer requests. Those two requests require 52 entries including their base/single entries. Appending the specification to all 650 single entries adds 40 entries, rather than duplicating the 12 existing ones. No full combination Cartesian product is generated.

Other charts or difficulties can be supplied through `--corpus-mode declared-manifest`. This accepts the same `ournotes-deck.search-benchmark/1` format with any nonempty unique case list. Every case must still supply complete original data/snapshot/request inputs and matching dataset identities. The first planner supports Mission requests; it refuses unsupported scenes explicitly. This mode does not claim those additional inputs have already been measured.

### Generate and resume legacy entries

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

### Legacy per-entry identity and shared source rules

Dataset SHA, original request/snapshot hashes and source paths are provenance, not the sole cache key. The native generator exports:

- A shared descriptor: resolved chart/play/setup/frame clock and shared native rules, complete score-shape/probe catalogue and row-to-shape mapping, actual neutral/probe dependencies, and algorithm schema.
- Each job's descriptor: ordered entry keys, selected skill rows at their actual levels, and referenced trigger/condition/release/reset, target and cumulative definitions.
- `sharedFingerprint` for incremental reuse. The archive's `context.fingerprint` additionally binds **all entries in that archive**, so it is intentionally not the per-job cache key.

Changing one skill level rebuilds jobs that depend on it; changes to an actual neutral/probe or shared lottery rule affect their dependent contexts. New members or Snaps that use existing skills only add provenance or needed jobs. A new unrelated chain writer adds its own jobs without invalidating old entries when it leaves the shared score shapes, neutral and probes unchanged. New score-probe catalogue behavior can invalidate the shared context. Every requested writer still receives independent native catalogue validation. Unrelated dataset facts can therefore change the whole-file SHA without changing a job, and a new combination leaves the old single entries reusable. The initial source digest conservatively covers all crate Rust source, manifests/lockfiles and the native generator: even an unrelated ordinary-score code edit can invalidate the tables. This over-invalidation is intentional until a narrower algorithm dependency contract is established. Actual dataset-change experiments are separate from these dependency contracts.

Planning verifies the binary's embedded simulation source hash against the selected source tree, using the same length-prefixed hash as `ournotes-sim/build.rs`. The plan also records the actual generator executable hash; running with a different executable requires replanning, even on a full cache hit. The executable hash is provenance rather than a task key, so identical semantic sources can reuse data across builds. Search/harness source identity is not yet embedded in the executable: the CI same-checkout build provides that outer guarantee, and local callers must likewise supply the source used to build their generator. Use one writer per store.

Entry order, multiplicity, owner performance position, source type, level and formation variant are retained. The Python selector does not grant native equivalence or silently drop missing definitions. All native planning results must cover every requested entry exactly once.

## Small engineering checks

```sh
python3 -m unittest discover -s tools/luck-tables -p 'test_*.py'
```

These checks use small JSON fixtures and a fake native transport to test selection, dependency invalidation, corruption, cancellation and resumability. They do not measure synthetic solver performance. Native correctness, actual response curves, complete 120-order comparisons and original real-request performance remain separate validation work.
