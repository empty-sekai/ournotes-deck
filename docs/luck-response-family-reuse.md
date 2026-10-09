# Reusing native recordings and quotienting Miss-gauge interactions

This extends the [conditional response experiment](luck-response-tables.md) in PR43.
The implementation removes repeated native recording within a proven minimum family and adds an
opt-in identity quotient for separately rounded Miss-gauge additions. The original controller still
propagates every distinct retained response. Both paths remain experimental probability prediction;
complete full-roster LUCK Top-K search within 20 seconds is not established.

The [source-pinned measurement audit](luck-response-family-audit.json) records the complete
TW/JP family run: **692 regional chart/difficulty records, all 292 regional natural-LUCK records, 365,000 parameter
jobs and only 292 grid recordings**. After regional sharing, 2,364 conditional programs occupy
**744,752 bytes in U24/XZ**. Same-binary comparisons on the original PR40 short/long cases and a
real JP Easy chart reduce 1,250-job identification from 7.38–9.38 seconds to 0.97–1.05 seconds.
These are minimum families with their other controller dependencies fixed, not every possible team.

## Minimum families: record the remaining controller once

Conditioning the new range-start minimum on its possible values already bounds a fixed controller
to at most 27 responses for the current real 0/1/2 guarantees and three LUCK starts. Previously,
identifying each new combination still rebuilt the entire recorded live. `LuckTableMinimumFamilySession`
now retains a bounded recording family for one immutable master, chart, play and observer context.

Every request still passes the original holder construction, source/capacity/formation checks,
native model initialization and mechanism admission. The fast path erases only whole minimum-only
updaters that form a suffix of the initialized updater rows and whose effect predicates are
statically false in the native weighted recorder. Empty updaters, mixed effects, LIFE-dependent
predicates, unknown predicates and shifted nonminimum row indices do not qualify. The complete
remaining initialized model, nonminimum actions and observer plan must compare equal in process.

The first member records its complete original live and checks that its minimum actions match the
admitted static plan at every actual start. Later members reuse that recording, reweight the
conditional terms and reuse their complete identities. A fingerprint alone never admits a member.
Failed admission, unsupported shapes and bounded-cache exhaustion use the original recording path.
The retained family and original compiled program have separate budgets. Temporary key bytes are
reported separately; these counters are not a process-RSS measurement.

`basisFamilyReuse` defaults to `true`; set it to `false` for an independently recorded control.
`basisFamilyBytes` bounds the family index. `compiledJobs` keeps its original logical meaning;
`nativeRecordings`, `familyHits`, `familyFallbacks`, `familyIdentityComputations` and
`familyIdentityHits` expose the actual saved work. Independent original-DP verification recording
and propagation are reported separately.

## Miss gauge: add the actual native increments

For an admitted gauge maximum `g` and effect value `v`, the native action first forms its integer
product, converts to binary32, divides by 10,000 and floors. A set of Miss actions is represented by
the vector of sums of those **separately rounded** increments over every reachable gauge maximum.
Adding raw percentages before flooring would change the program.

The actual TW and JP master settings acquired on 2026-10-09 use maxima 140 and 70. The conservative
native domain also retains constructor maxima 50 and 100. For example, actual unmatched GS96 level
1 contributes `[2,3,5,7]` over `[50,70,100,140]`. Two copies contribute `[4,6,10,14]`, whereas one
level-2 copy contributes `[5,7,10,14]`. Those programs must remain distinct. Level 1 plus level 2
equals level 3 over this domain, and those can share a complete canonical identity when all other
native dependencies agree.

`canonicalMissGauge: true` enables the quotient. Its admission checks cover integer/product bounds,
binary32 note increments, gauge headroom, active-range geometry, complete frame/action coverage,
same-frame state dependencies and bounded workspace. Non-Miss actions keep their relative order;
a per-frame Miss marker remains even for a zero vector. The proof relies on nonnegative additions,
no overflow, a fixed maximum within each frame, and unchanged Miss/once flags until the frame ends.
The original native tape is retained for propagation. Interval endpoints can differ by harmless
association rounding, so independent original comparisons require overlapping joint enclosures,
equal probes and equal transition masks rather than bitwise endpoint equality.

This mode explicitly disables minimum-family recording reuse and cannot be combined with the older
CDF canonicalization. Its complete-program contract is `canonical-miss-gauge-deltas/1`; the
conditional transport contract is `conditional-start-minimum+canonical-miss-gauge-deltas/1`.
Materialization binds the requested contract and refuses mixed or missing contracts.

### Two sufficient integer statistics in the current master domain

The actual GS96–105 sources supply 50 source/level records and 100 candidate effect rows in each
region. Each record selects one of two mutually exclusive formation branches; these are not 100
simultaneously active writers. Their 12 distinct values are `500k`, with
`k ∈ {1,2,3,4,5,6,7,8,10,12,14,20}`. Let `S` be the sum of `k`, and `O` the number of odd `k`.
The separately rounded aggregate over the conservative current domain is:

| Gauge maximum | Aggregate Miss increment |
| ---: | --- |
| 50 | `(5S − O) / 2` |
| 70 | `(7S − O) / 2` |
| 100 | `5S` |
| 140 | `7S` |

For these values the native products and half-integer quotients are exactly representable in
binary32. Independent enumeration of the four-dimensional native increments agrees at every
multiplicity with the `(S,O)` recurrence. Exactly ten effective rows have **669** increment classes;
zero through ten rows together have **724**, including the empty combination. This counts an
algebraic parameter domain, not owned decks or 724 precomputed whole-chart responses. The native
implementation computes the actual reachable-maximum vector and does not hardcode these two
statistics as a rule for future master data.

The nominal response still depends on when Miss happens and how its gauge increment changes future
draws. The quotient retains that interaction with minimum guarantees and Rush. Different remaining
speed, start-gauge, LIFE, judgment, frame and observer dependencies require a different complete
identity. The parameter vector alone never authorizes a response alias.

## Complete real-chart validation and CI

The new family validator accepts only a complete declared regional manifest, verifies exact equality
with its actual master chart set and retains every non-LUCK record as `notApplicable`. For each
natural LUCK chart it identifies all 1,250 actual GK8/GS66/GS71 parameter kernels, independently
records six configurations and runs their original DP plus every conditional term. Those six must
cover every conditional program in the full grid. Six unseen minimum combinations and three real
nonminimum controls test reuse and separation. These master-skill kernels are distinct from legal
owned-deck search benchmarks.

```sh
python3 tools/luck-tables/validate_basis_families.py work/catalogue/tw/inputs/benchmark.json \
  --generator tools/search-harness/target/release/luck_response --source . \
  --output work/families/tw
python3 tools/luck-tables/validate_basis_families.py work/catalogue/jp/inputs/benchmark.json \
  --generator tools/search-harness/target/release/luck_response --source . \
  --output work/families/jp
python3 tools/luck-tables/aggregate_basis_families.py \
  --manifest work/catalogue/tw/inputs/benchmark.json \
  --manifest work/catalogue/jp/inputs/benchmark.json \
  --root work/families --generator tools/search-harness/target/release/luck_response --source . \
  --output work/family-audit/aggregate.json --bundle-dir work/family-dictionaries
```

Use `--shard-index I --shard-count N` for stable score-ID partitioning, and
`--full-reference-grid` when every one of the 1,250 jobs should also be recorded without reuse.
The independent aggregate checks every input/spec/report receipt, reconstructs each response
projection from the verified native report, repacks every blob with the same binary through the
codec (zero DP), and compares its exact bytes. It merges only those audited program addresses and
verifies gzip/XZ decompression against the complete original TAR hash and length. The deployed
dictionary has no global skill-combination alias catalogue.

`validate_miss_gauge.py` compares 625 actual full-capacity Miss/minimum/speed kernels and six real
rounding witnesses per selected real chart against independently propagated original programs.
The default selections use the original PR40 short/long newcomer cases when that manifest is
supplied, or the pinned TW short/long catalogue requests and JP 10000300 Easy.

```sh
python3 tools/luck-tables/validate_miss_gauge.py work/full48/benchmark.json \
  --generator tools/search-harness/target/release/luck_response --source . \
  --output work/miss-pr40
```

The complete Miss experiment uses an explicit 256 MiB identity allowance and 32 MiB per-program
allowance. It first identifies both full original and canonical job sets with zero DP, requires
complete admission, and binds those identities to the subsequent propagated responses. The initial
128 MiB reference allowance admitted only 501/461 of 631 original programs on the two TW charts;
that run correctly failed and is retained in the audit. The larger allowance is an experimental
reference setting; no native capacity guard was removed. The preflight prevents spending minutes
propagating a reference that cannot retain all requested identities.

`validate_miss_basis.py` additionally exercises the combined quotient and conditional transport
through cold generation, generation-disabled warm lookup, materialization and final U24 decoding:

```sh
python3 tools/luck-tables/validate_miss_basis.py DATA SNAPSHOT REQUEST \
  --generator tools/search-harness/target/release/luck_response --source . \
  --output work/miss-basis --timeout-seconds 600
```

CI shares one pinned TW/JP input bundle and native binary, adds 2 regions × 4 family shards, and
requires a separate complete-family audit before publishing the compressed family dictionaries.
It also runs the real Miss experiment. Existing full-catalogue base/single generation, incremental
dependency validation and original interaction experiments remain in place. Raw family shards are
retained for seven days; full audits and compressed dictionaries for 30 days. Daily publication
refresh becomes active on the default branch after merge.

## Same-binary measurements on original PR40 inputs

The following measurements use the successful CI generator built with Rust 1.99.0, binary SHA256
`9c4560819c8259bc564e91c0d0c682ad3dc00f1869e53812c9d13b62b223e6ed`, simulation source
`c18b5846c45ccf29109220d3a6a0afa08d9d1dd167c2771d4c49211c2cdec44c` and semantic algorithm digest
`748bec38e41505d8944600a4ac99b3e40a11c7a635bb19109d4a41397e525282`. They were run sequentially on
an eight-CPU-quota, 8 GiB environment without competing compute workloads.
Times include the native process and report I/O. They are observed single-run wall times,
not distributions from repeated timing trials.

| Unchanged real request context | Record every job independently | Family reuse | Measured ratio |
| --- | ---: | ---: | ---: |
| Original PR40 short newcomer | 7.380 s | 1.043 s | 7.08× |
| Original PR40 long newcomer | 9.379 s | 1.045 s | 8.97× |
| JP 10000300 Easy | 8.466 s | 0.974 s | 8.69× |

Each row identifies all 1,250 actual-master jobs without DP. Every one of the **3,750 complete
conditional mappings**, including all **65,625 term references**, agrees exactly with independent
recording. Each fast run performs one native recording, 1,249 family hits, 27 complete conditional
identity computations and 21,848 conditional identity hits. The retained family payload is
290,876 B short, 303,288 B long and 220,054 B Easy; these are accounted cache bytes, not RSS.
Six independent original-DP comparisons per context cover all 27 terms: 18 comparisons,
13,056 joint buckets, no disjoint intervals, equal probes/masks and maximum endpoint difference
`1.1102230246251565e-14`. New minimum combinations and the three actual nonminimum controls pass.

### Actual held deck and all original orders

The original veteran deck `[1,28,43,49,64]` with Snaps `[42,1,2,3,4]` was independently reevaluated
on both original PR40 veteran requests. All **240 original orders** pass. Every cold/warm U24 joint
response encloses its original nominal-DP endpoints; probe flags, transition masks and available
range moments agree. Both score-at-mean predictors give exactly equal values on all orders.

| Original veteran context | Cold lookup + 120 predictions | Warm lookup + 120 predictions | Cold / warm DP |
| --- | ---: | ---: | ---: |
| Short | 3.651 s | 1.760 s | 8 / 0 |
| Long | 4.559 s | 2.253 s | 8 / 0 |

The original 120-order references were generated independently, taking 27.714 s and 30.597 s.
This comparison concerns one actual held deck and its score proxy. It does not establish full-roster
search time, a Top-K certificate or the expectation of the complete nonlinear native score.

### Complete Miss-level grids and actual rounding witnesses

The corrected 256 MiB experiment completed on all three unchanged input contexts. Every chart
first admitted all 631 original identities and all canonical identities with zero DP, then bound
every propagated job to its own preflight fingerprint. The full 625-job grid fills five main and
ten support slots with actual GK8 Lv3, GS66 Lv3 and GS96 writers. Four GS96 levels range independently
over 1–5 and the fifth remains Lv3; every formation branch is unmatched. The six additional jobs
exercise the actual separately rounded equivalences and counterexample.

| Unchanged context | Complete grid: original / canonical programs | Grid plus witnesses: original / canonical DP | Joint buckets compared |
| --- | ---: | ---: | ---: |
| Original PR40 short newcomer | 625 / 44 | 631 / 48 | 507,324 |
| Original PR40 long newcomer, retaining its four Misses | 625 / 44 | 631 / 48 | 474,512 |
| JP 10000300 Easy | 625 / 44 | 631 / 48 | 391,220 |

All **1,893 independently propagated job comparisons** pass: **1,373,056 joint probability
intervals**, no disjoint intervals, equal probe flags and transition masks, and maximum endpoint
difference `2.7755575615628914e-15`. Each measured 44-program grid matches the independent
actual-master increment-class enumeration. The full original reference uses 1,893 DP calls;
the canonical references use 144. No partially admitted reference contributes to these results.

| Context | Original / canonical U24 index + blobs | Original / canonical XZ6 |
| --- | ---: | ---: |
| Original PR40 short | 3,904,240 / 298,661 B | 160,796 / 99,736 B |
| Original PR40 long | 3,685,526 / 281,741 B | 154,544 / 93,928 B |
| JP Easy | 3,036,381 / 232,225 B | 126,156 / 66,324 B |

All six dictionaries pass both gzip and XZ round trips. These tables contain 631 or 48 complete
programs respectively, including the witnesses; they are separate from the complete-catalogue
minimum-family table. Three chart processes ran concurrently, so their recorded phase times are
preserved as work observations rather than an isolated-machine speed comparison.

A separate read-only audit rebound all four reports per chart to the original manifests and
unchanged source/binary, repeated every job comparison, and independently decompressed all 12
gzip/XZ archives. Every TAR member matches its actual index/blob bytes; all 2,105 audited files
remained unchanged. This second audit invoked no native process and performed zero DP.

### Combined Miss/minimum transport

The six actual rounding witnesses also pass the portable `validate_miss_basis.py` gate on the
original PR40 short input. Fixed GS66 Lv3 has a minimum guarantee of one, so each request has eight
conditional terms over three starts: 48 references share 32 programs. Cold lookup generates 32
programs; the generation-disabled warm run uses zero DP. Both materialized and final re-encoded U24
responses contain all **4,824 independently computed original joint intervals**, with equal
probes/masks/moments and identical cold/warm JSON and archive bytes. The final two-stage U24 encoding
expands an endpoint by at most `1.1920910114593397e-7` in this measurement.

The real rounding counterexample is observable on this chart: two level-1 GS96 writers and one
level-2 writer differ in 668 of 804 joint buckets, with maximum endpoint difference about
`2.05e-4`. Level 1 plus level 2, the reversed ordering and one level-3 writer share the same complete
conditional mapping. The six direct original references need only six DP calls, so this cold
32-program basis query is not a cold-query speedup. Its benefit is reuse across later combinations.

## Complete family generation and independent publication audit

The successfully completed family jobs used [GitHub Actions run 37880796087](https://github.com/empty-sekai/ournotes-deck/actions/runs/37880796087) at head `6fb669cd177fa8b0857bee500e86f7f46bdfb1f8`, simulator source `c18b5846c45ccf29109220d3a6a0afa08d9d1dd167c2771d4c49211c2cdec44c`, and native binary SHA256 `9c4560819c8259bc564e91c0d0c682ad3dc00f1869e53812c9d13b62b223e6ed`. TW and JP came from the complete pinned bdon master catalogues. No synthetic chart or replacement skill row was used.

| Regional catalogue | All chart/difficulty records | Natural LUCK records | Explicitly not applicable | Conditional programs before regional sharing |
|---|---:|---:|---:|---:|
| TW | 348 | 148 | 200 | 2,364 |
| JP | 344 | 144 | 200 | 2,256 |
| Total | 692 | 292 | 400 | 4,620 |

Each natural LUCK chart received all 1,250 configurations of the same declared master-skill experiment: five GK8 level-3 main writers and ten GS66/GS71 support writers occupy all 15 slots. Four GS66 levels independently range over 1–5; the fifth GS66 remains level 5 and every GS71 remains level 3. Both all-matched and mixed-match formations are included. These are real-master parameter kernels, not a claim that every corresponding skill configuration is owned by the supplied roster. This audit covers the complete chart/difficulty catalogues for this fixed non-minimum family; it does not certify every possible team or play profile.

All 365,000 grid jobs passed. The fast path recorded 292 native families and served 364,708 family hits. Six configurations per LUCK chart were independently checked against original DP: 1,752 original evaluations, 686,712 joint-bucket comparisons, and a maximum probability endpoint difference of `1.3877787807814457e-14`. Their conditional-program union covered every grid term. Each chart also passed new-minimum combinations and distinct start-gauge, Miss-gauge and gauge-speed controls. The 400 non-LUCK records were explicitly retained with no native work.

The complete evidence was downloaded independently: eight shard archives plus the CI audit and published dictionary. All ten downloads matched GitHub API byte lengths and SHA256 digests; every ZIP passed CRC checks and path validation before extraction. Local `aggregate_basis_families.py` then repeated all receipt, actual-master scope, mapping, curve and dictionary checks. It repacked all 4,620 regional conditional programs with the identical native binary, using zero DP calls. Each generated archive exactly matched the retained archive.

The independently reproduced and CI aggregate/analysis JSON agree after excluding only newly measured compression durations and the enclosing analysis-file digest/length, which are separately checked against each full file. The complete index, gzip archive and XZ archive are **byte-for-byte identical** between the independent reproduction and the CI publication.

| Complete merged dictionary | Actual bytes |
|---|---:|
| Program index | 960,039 |
| Unique native U24 blobs | 8,504,156 |
| Index plus blobs | 9,464,195 |
| Deterministic TAR | 11,274,240 |
| gzip level 9 | 5,231,483 |
| XZ preset 6 | 744,752 |

The merged dictionary contains 2,364 exact program identities. Of these, 2,256 occur in both regional catalogues; no report has unknown chart scope. Both compressed forms round-trip to the same exact TAR. The dictionary stores conditional program identities and response blobs, without a global list of 365,000 skill-combination aliases.

The independent base/single dictionary remains a separate table on this same native source:
350,692 jobs produce 14,236 regional program records and 7,216 merged identities, with a U24
index-plus-blob size of 16,429,514 B and XZ size of 4,599,344 B. Its downloaded audit summaries,
all 28,864 blobs across four encodings and all eight compressed bundles were independently
validated. This publication check did not download the 16 raw base/single shards or rerun their DP.
The family size of 744,752 B includes neither that table nor a future complete Miss-combination
table; the two identity counts cannot be added without computing their union.

Independent publication receipt: `publication-comparison.json`, SHA256 `054caee98392d643b112575415e03c95241ee00f294e5401ee5838aa758470eb`. Published XZ SHA256: `8e374352a575fdd700e7b0a98f9396e998593ffa4db2fb0198658998c0572113`.

## Source-pinned CI verification

Commit [`2f17bac`](https://github.com/empty-sekai/ournotes-deck/commit/2f17bac2b12c5459c56b4e87b647c83465821204)
passes [general CI](https://github.com/empty-sekai/ournotes-deck/actions/runs/37883577481),
[Chromium Worker checks](https://github.com/empty-sekai/ournotes-deck/actions/runs/37883577449), and the
[complete LUCK workflow](https://github.com/empty-sekai/ournotes-deck/actions/runs/37883577487).
The full TW and JP Miss comparisons and the combined Miss/minimum cold/warm gate all pass.
These CI interactions use the declared catalogue-anchor snapshots; the original PR40 measurements
above retain their own request and roster identities.

The published input bundle matches the measured bundle exactly, including all 715 catalogue/input
files, 327 native source files and the generator. Independent verification checks all five merged
indexes and all ten gzip/XZ archives from the base/single and minimum-family publications. Every
published index and compressed file is byte-identical to the audited counterpart, and every
compression round trip recovers its complete TAR.

The base/single stage reuses **all 292 LUCK charts through exact-input cache hits**, with **zero
new native dependency plans and zero generated programs**. These are current-run counters for
that stage. Its 14,236 retained regional program records describe the existing table. The
minimum-family validation remains a separate cold grid and independently covers all 692 regional
records, including the 400 explicit non-LUCK records.

The [measurement audit](luck-response-family-audit.json) includes the workflow and publication
verification receipts. Receipt roles identify inputs, outputs and verification records by content
hash; downloadable CI records additionally retain their public artifact identities.

## Reversible block sharing on the measured native source

The block-sharing experiment uses three source-pinned dictionaries: the original PR40 `short-newcomer-score` and `long-newcomer-score` request/roster contexts, and `jp-10000300-easy`. The substituted skill experiment remains the same declared real-master parameter kernel. Each dictionary contains 27 conditional programs; these measurements are three real samples, not a full-catalogue estimate for the prototype codec.

| Context | Native / prefix index + payload | Native / prefix gzip9 | Native / prefix XZ6 |
|---|---:|---:|---:|
| PR40 short newcomer | 164,113 / 43,455 B | 20,589 / 14,116 B | 10,400 / 10,716 B |
| PR40 long newcomer | 154,662 / 42,480 B | 23,267 / 14,066 B | 9,776 / 10,340 B |
| JP easy | 126,875 / 39,737 B | 16,138 / 13,075 B | 10,056 / 10,836 B |

Conditional-prefix sharing reduces raw index-plus-payload bytes by 68.68–73.52% and gzip bytes by 18.98–39.55%. XZ becomes **3.04–7.76% larger** in these same samples. Every tested alternative—including fixed blocks and native sections—also exceeds the original XZ size in all three samples. XZ already captures much of the repeated structure, while explicit pooling changes byte adjacency and adds reference metadata. These results support retaining native U24 plus XZ as the default transport format.

Every source archive was checked against its native index, source and payload SHA. The minimum-choice tuple is bound through native `programFingerprint` keys, independently of filename order. All 81 original archives were reconstructed exactly by each pool scheme; all 42 outer gzip/XZ round trips passed, and all input bytes were unchanged. No native simulation was performed for this analysis. Smaller stored raw bytes do not establish smaller decoded memory or RSS: the prototype reconstructs complete original archives, and its runtime lookup costs were not measured. No browser or native codec replacement was introduced.
