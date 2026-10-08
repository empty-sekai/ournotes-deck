# Offline LUCK response tables and skill interactions

The complete published TW and JP catalogues have been generated and audited: **692 regional
chart/difficulty records are accounted for, including all 292 charts with actual LUCK missions**.
The 350,692 base/single-skill labels required 14,236 native DP propagations across chart jobs. Merging
the verified program dictionaries leaves **7,216 distinct responses**. Their complete U24 index and
blobs occupy 16,429,514 bytes, or **4,591,788 bytes in the measured XZ bundle**.
The [catalogue audit](luck-response-catalogue-audit.json) records the source, input hashes, every
artifact and compression round trip for the completed
[`de589e3` workflow](https://github.com/empty-sekai/ournotes-deck/actions/runs/37820020030).

The implementation also accepts interacting skills through a persistent native-program dictionary.
The newer conditional-basis path factors the probabilities of range-start minimum guarantees away
from their remaining native controller. On three actual chart contexts, **1,250 skill-combination
jobs per context identify only 27 conditional programs**. After two warmup jobs generate those 27
terms, previously unseen levels and multiplicities need zero additional DP. The three stored
families total **27,104 bytes in U24/XZ**. These are measured fixed-controller families; the
[conditional audit](luck-response-conditional-audit.json) records their separate algorithm source,
original inputs and independent native comparisons. The complete catalogue measurements above
concern the direct program path.

This remains an experimental prediction path. It does not replace production recommendation or
authorize search pruning. The original requirement for complete LUCK Top-K search within 20 seconds
remains unmet. Probability-law agreement and agreement between two score predictors are reported
with their own meanings, without treating them as full native expected-score or ranking proofs.

## Complete published inputs

Acquisition follows the actual [bdon.moe](https://bdon.moe) publication pointers for
[TW](https://storage.bdon.moe/moenotes/music-data/build.json) and
[JP](https://storage.bdon.moe/moenotes/jp/music-data/build.json), then verifies the replay manifest,
deck-data SHA and embedded note coverage. Every chart must agree with the original master music
and score tables. The following is the pinned published inventory acquired on **2026-10-08**:

| Region | Songs | Each of Easy/Normal/Hard/Expert | All charts | Natural LUCK | Actual non-LUCK |
| --- | ---: | ---: | ---: | ---: | ---: |
| TW | 87 | 87 | 348 | 148 | 200 |
| JP | 86 | 86 | 344 | 144 | 200 |
| Total regional records | 173 | 173 | 692 | 292 | 400 |

Regional totals include shared songs and charts; they are not counts of globally distinct music.
TW is pinned to dataset
`de867d2df3020e9430c164cdc889cd114113e50b2ab12003b6978665905493bf`, and JP to
`76446232396ba61cbc499f9e31313a39ab635534426354197b29d81fd0a87d47`.
The audit records **zero missing charts and zero chart errors**. Its independent aggregate is
byte-identical to the published CI aggregate, with SHA-256
`a273c1786409482e41a90b7025cfb875cc3f12f94c95d17ebacf527c017ba68c`.

Each region supplies 61 master tables and 140 actual writer source/ID/level records: 40 Gekisou and
100 GekisouSupport. Formation variants expand these to 240 native catalogue keys. The represented
writer effects are 11001, 11002, 11003 and 11005. A base plus every key in every performance position
gives `1 + 240 × 5 = 1,201` labels per applicable chart, hence **350,692 labels** over all 292 LUCK
charts. The 400 other charts remain explicitly `notApplicable` according to their real played
missions. Their missions are not changed to manufacture additional LUCK cases.

The catalogue anchor uses five distinct actual master characters with minimum represented card and
player parameters, without Snaps. It is explicitly an isolated kernel construction, not a claim
about a player's inventory. All master writer levels are requested independently of those five
cards. The default play profile is theoretical best. Other judgment streams, LIFE inputs and
scenarios require their own complete native context; complete chart coverage does not mean every
possible play profile has already been tabulated.

The [full-catalogue audit](luck-response-catalogue-audit.json) verifies all 18 downloaded artifacts,
including 16 regional shards, the shared input/binary bundle and the aggregate. SHA, byte length,
ZIP CRC and extraction paths were checked. All 292 applicable charts were cold generations at this
source. Per-chart native counters sum to 688.283 seconds of compilation and 391.837 seconds of
propagation across independent CI workers. Those sums are **not workflow wall time**. There were
336,456 complete-program aliases within the chart runs; 7,020 of the 7,216 merged program identities
are shared across regions.

## What is precomputed

Repeated multi-seed simulation can estimate a convergent coefficient, but each coefficient discards
timing and can require many whole lives. For mechanisms admitted by the native recorder, this
implementation instead propagates the independent nominal lottery law with deterministic DP.
Outward intervals enclose that law; generating the table does not require a Monte Carlo stopping
criterion.

At each stored event time, a response carries four joint buckets:

| Bucket | Rush | Supported direct-7021 score probe |
| --- | --- | --- |
| 0 | Off | Off |
| 1 | Off | On |
| 2 | On | Off |
| 3 | On | On |

The representation retains the original frame transition masks, probe flags, available range
moments and work counters. Keeping the joint response matters when guarantees, caps, thresholds,
previous-frame conditions and ordinary scoring effects interact.

| Quantity | How it is obtained | Interpretation |
| --- | --- | --- |
| Nominal joint response | Native-recorded controller plus outward DP | Supported independent-draw probability law for that context |
| Native Monte Carlo score | Complete native lives on recorded seeds | Empirical mean and uncertainty for those samples |
| `scoreAtMean` / `scoreAtLookup` | Native weighted-score replay using probability weights | Score proxy, generally not `E[full native score]` |

Integer floors, binary32 operations, rank bonuses, history rewrites and correlations with other
effects remain relevant to full score. Optional Monte Carlo controls and independent whole-deck
reference calculation are therefore separate from ordinary table lookup.

## Multiple skills: preserve the native controller, then factor a bounded part

### Complete program dictionary

`programs.py` compiles every requested combination through the original native recorder without
running DP. Its retained identity contains the full ordered controller program, frame and note
inputs, lottery settings and observer contract. The in-process registry uses a fingerprint to find
candidates and then compares the **entire identity** before establishing an alias. An equal skill
count or equal hash alone is insufficient.

Only distinct missing program responses are propagated. The persistent mapping is
`program fingerprint → response blob`; it does not include a global index of all possible skill
combinations. Every new query retains its exact original labels, ordered skill entries, multiplicity,
source capacity and formation variants. It materializes aliases only for that query. Identifying a
new combination still requires compilation, but an already stored resulting program needs no DP.

All five performance positions may contain LUCK writers. The native full-holder path validates an
explicit virtual direct-7021 observer while retaining all writers and records the observer mode in
the complete program identity. This supports the real five-holder and 15-source-slot cases without
replacing a writer with a probe. Unknown observer or writer behavior remains an explicit refusal.

This dictionary is useful even when different skill descriptions yield the same controller.
Different controllers can still grow combinatorially. A more specific
factorization is needed to remove a whole interaction dimension rather than merely discover aliases.

### Maximum guarantees have a small probability distribution

At a native start, let writer `i` supply guarantee `a_i` with independent nominal probability `q_i`.
Let `M_r` be the maximum guarantee newly supplied by that start's triggered writers, or zero when
none trigger. The native state then updates to `max(existing minimum, M_r)`; this definition does
not assume that the existing minimum was reset. The newly supplied guarantee has cumulative law

$$
F_r(m)=\Pr(M_r\le m)=\prod_{i:a_i>m}(1-q_{ri}),
\qquad w_r(m)=F_r(m)-F_r(m-1),\quad F_r(-1)=0.
$$

The compiler uses the actual native binary32 probabilities, including their rounding. For example,
the real GS66 level 5 and GS71 level 3 rows have an exact failure-product relation to GS71 level 5:

$$
(1-\operatorname{f32}(0.60))(1-\operatorname{f32}(0.50))
=\frac{3355443}{16777216}
=1-\operatorname{f32}(0.80).
$$

When their native minimum and surrounding behavior match, two writer actions and one writer action
can thus describe the same maximum law. The checked-in
[cross-multiplicity fixture](../tools/luck-tables/fixtures/start-minimum-cross-multiplicity.json)
preserves the real source IDs, levels, formation variants and positions for native comparison.

The optional `canonicalStartMinimum` transformation uses exact reduced dyadic CDF identities for
contiguous minimum-action runs. It preserves every other action barrier and retains the original
block when its bounded exact arithmetic cannot represent a transformation. This merges equal laws;
it does not make every different CDF share a response.

The actual cross-multiplicity witness was run on the original PR40 short/long newcomer requests and
JP 10000300 Easy. In **each context, eight requested jobs require four original programs and only
two canonical programs**. Independent original/canonical comparisons cover 17,376 joint buckets
with no disjoint intervals; probes and transition masks match. The largest endpoint difference is
`2.4424906541753444e-15`, reflecting the different outward arithmetic paths rather than a claim
of bit-identical intervals. These measurements use the newer source recorded in the
[conditional audit](luck-response-conditional-audit.json).

### Whole-live conditional basis

For an admitted native recording, hold the remaining controller `C` fixed: all gauge and miss
actions, their probabilities and integer conversions, speed timing, notes, frame order, lottery
tables and observations. Let its number of relevant native starts be `R ≤ 3`. For each possible
vector of start minima, compute the conditional **whole-live** response once:

$$
\mathbf{P}_{C}(t)
=\sum_{\mathbf m\in\{0,1,2,3\}^{R}}
\left(\prod_{r=1}^{R} w_r(m_r)\right)
\mathbf{P}_{C\mid\mathbf M=\mathbf m}(t).
$$

This is the law of total probability with nonnegative weights. The conditional curves still contain
the complete interactions of the remaining skills and controller. They are not independently
generated single-skill curves. Each term records the entire live, including COMPLETE/FINISH tails;
the decomposition does not require proving that one range resets every controller state before the
next range.

The current real minimum values are among 0, 1 and 2, giving at most `3^R ≤ 27` terms for a fixed
remaining controller. The general four-value implementation has an explicit `4^R ≤ 64` cap.
Zero-mass choices need no term. Changing minimum probabilities, source multiplicities or equivalent
writer groups can change only the small weight vector while retaining the conditional program
addresses. `basis.py` persists those addresses in the same source/mode dictionary as other programs,
using a separate native identity domain.

Basis weights are computed by forward propagation over the four possible minimum values with
outward intervals. They do not require the exact-CDF transformation's bounded dyadic denominator.
A long list of probabilistic writers can therefore retain a small conditional basis even when its
exact CDF is too large for the optional canonical identity rewrite.

The bound applies **per remaining controller and chart/play context**. A changed miss-triggered
gauge command, start-gauge distribution, speed command or LIFE-dependent condition can change `C`
and require another family. Support miss-gauge effects cannot be combined by adding percentages
before native flooring: for example, two separate 500-unit updates at maximum 50 yield `2 + 2`,
while one 1,000-unit update yields `5`. Such operations remain in their original ordered transcript.

One direct program may need only one propagation. An isolated new query requiring eight or 27 basis
terms can therefore be more expensive cold. The basis is intended to amortize work over families of
probabilities and combinations; it is not an unconditional faster default.

`basisIdentify` returns each original job's conditional program addresses, probability intervals
and start choices without DP. `basisPrograms` receives the full original jobs plus only missing
addresses. `basis.py` compares complete mappings, weights, source/context/input provenance and the
exact propagation count before storing successful terms. Its CONTROL labels are an adapter for
binary program storage, not physical decks. `basis-materialize` performs the requested mixture with
outward arithmetic. The standard query disables repeated original-job reconstruction and extra
verification; `verifyBasis: true` explicitly requests an independent original-program comparison
in native diagnostic runs.

### Measured conditional families on real charts

The [conditional audit](luck-response-conditional-audit.json) binds the following experiments to
native source [`753ad76`](https://github.com/empty-sekai/ournotes-deck/commit/753ad7624d1f01584595290a6af0f72bb4c98029),
simulation hash `0164248e95314dc38d8154f7fa93f7ec0f7e445697e191a7289097ec89d830d6` and binary
`d01307109fab12c6cc640e2378dec0075f16fa511da066ed319e20fb66dc436f`. This source adds the exact-CDF
and conditional identity rules; its measurements are distinct from the completed `de589e3`
catalogue generation. The original data, snapshots and requests remained byte-identical throughout
the experiment.

`make_minimum_basis_specs.py` reads and validates the actual published master keys before
constructing `2 × 5^4 = 1,250` labelled jobs per context. Every grid job occupies all five main and
ten support writer slots. Each holder has GK8 level 3, GS66 and GS71 level 3, where GK means
`gekisou` and GS means `gekisouSupport`. The GS66 levels at holders 0–3 vary independently from 1
through 5; the fifth remains at level 5. Both support sources share the same actual formation target.
The two formation patterns are all matched, and `[false, false, true, true, true]`. Actual matched
and unmatched support actions contribute minima 2 and 1; the five GK8 gauge-speed writers fix the
rest of the controller. These are parameter kernels using actual master skills, without asserting
ownership of all skill levels in one physical deck.

In all three contexts, including JP Easy, native recording finds three relevant starts. The 1,250
jobs reference 21,875 conditional terms but only **27 distinct complete conditional identities**.
The all-matched warmup needs eight terms and the mixed-match warmup needs 27; their union generates
exactly 27 DP programs. Grid identification performs no DP and retains all original labels. It does
not materialize 1,250 duplicate response curves merely to demonstrate those mappings.

Next, the validator requests 12 level profiles absent from the warmup and six new combinations:
varying GS71 levels across all five holders, removing one support from every holder, and alternating
the remaining support source. It repeats the 12 level queries once more. **All three phases run
with generation disabled and reuse only the existing 27 programs in every context**. Repeated
queries produce byte-identical materialized archives. The stored dictionary has no global grid or
all-combination alias index.

| Actual context | Identify 1,250 jobs, 0 DP | Cold 2-job warmup, 27 DP | 12 unseen-level jobs, 0 DP | 6 new combinations, 0 DP | Repeat 12 jobs, 0 DP |
| --- | ---: | ---: | ---: | ---: | ---: |
| Original PR40 short newcomer | 7.847 s | 3.243 s | 0.236 s | 0.156 s | 0.215 s |
| Original PR40 long newcomer | 10.804 s | 3.134 s | 0.270 s | 0.184 s | 0.265 s |
| JP 10000300 Easy | 9.279 s | 1.513 s | 0.273 s | 0.165 s | 0.243 s |

Times are individual phase wall times. Query phases include native identification, dictionary
validation, missing-term generation when allowed, mixture materialization and archive packing.
They exclude score prediction and the separate verification phase. The 1,250-job identification
column makes the remaining compilation cost visible; warm queries here contain 12 or six jobs.

Six selected unseen profiles per context were separately reconstructed and compared with six
independent original-program DP propagations. All **18 comparisons pass over 13,056 joint buckets**:
there are zero disjoint intervals, probe flags and transition masks match, and the maximum endpoint
difference is `1.1102230246251565e-14`. This comparison uses the original native program as well as
the conditional computation. It is separate from simply decoding a previously stored response.

The control changes one holder's real GK8 level 3 to real GK20 level 3, introducing a different
nonminimum start-gauge command. Each context first reports 27 missing programs with generation
disabled, then generates exactly those 27 new programs when allowed. Thus the measured reuse
removes level/probability/multiplicity growth inside the admitted minimum family while preserving
the dependency on the other native actions. It does not imply that all gauge, speed, miss-trigger,
LIFE and chart/play combinations fit into one universal 27-program table.

### Compression of those conditional families

The following payload contains only the two-job warmup's **27 programs per context**, all usable by
that context's 1,250 identified grid jobs and the measured new combinations. It excludes the
additional 27-program nonminimum control family per context, input data, original reports and
query aliases. These **81 programs cover three fixed-controller families**, not the full catalogue.

| U24 family | Unique programs | Index + unique blobs | XZ-6 bundle |
| --- | ---: | ---: | ---: |
| Original PR40 short newcomer | 27 | 153,087 B | 9,704 B |
| Original PR40 long newcomer | 27 | 143,673 B | 9,400 B |
| JP 10000300 Easy | 27 | 115,980 B | 9,620 B |
| Three families merged | 81 | 412,080 B | 27,104 B |

All four native encodings were created by `program-pack` without further DP. The merged payload is:

| Encoding, 81 conditional programs | Index + unique blobs | gzip-9 bundle | XZ-6 bundle | Zstandard-9 bundle |
| --- | ---: | ---: | ---: | ---: |
| Lossless | 712,069 B | 482,863 B | 107,812 B | 135,667 B |
| U16 | 351,240 B | 68,988 B | 20,332 B | 25,052 B |
| U24 | 412,080 B | 121,799 B | 27,104 B | 34,025 B |
| U32 | 468,429 B | 183,450 B | 34,476 B | 41,541 B |

Each separate family and the merged dictionary was measured in all four native encodings and all
three compression formats. **All 48 compressed bundles** decode to their exact original TAR byte
length and SHA. The [conditional audit](luck-response-conditional-audit.json) includes those
receipts. The merged U24 gzip is larger than the sum of the three separate gzip bundles: dictionary
merging changes blob adjacency, and compressor window limits matter. Merging and compression are
therefore measured independently; sharing more program addresses does not guarantee a smaller
compressed package in every codec.

## Measured queries and physical decks

The [interaction audit](luck-response-interactions-audit.json) covers actual master parameters on
JP 10000300 Easy and the unchanged original PR40 short/long newcomer requests, at the earlier
simulation source `303f64fedfac7cc2c5ab96cbe3143fb1900c7f25a8f5cafdec31b8a601c7f735`. The direct compiled
program experiment completed **1,563 mixed requests**: the following 521-request set in each of
the three contexts, with no hidden failures or reduced holder limits.

| Actual parameter case | Requested labels per context | Distinct native programs per context |
| --- | ---: | ---: |
| Mixed smoke cases | 30 | 10 |
| Base plus every one of 240 master variants with two additional writers | 241 | 50 |
| Four writer kinds over all position assignments | 120 | 1 |
| Five-holder permutations, all 15 source slots, and a five-main case | 122 | 4 |
| Cross-multiplicity minimum-law witness, original ordered programs | 8 | 4 |

The 241-request persistent query generated exactly 50 missing programs cold and zero warm in all
three contexts. Corrupting one privately copied response blob required one new propagation rather
than regenerating the whole dictionary. These fixtures are actual skill-parameter experiments;
their coverage does not assert ownership of all source levels in one inventory.

| Context, 241 requested mixtures | Cold query | Warm query | Cold / warm DP programs |
| --- | ---: | ---: | ---: |
| JP 10000300 Easy | 4.585 s | 2.734 s | 50 / 0 |
| PR40 short newcomer | 8.491 s | 2.041 s | 50 / 0 |
| PR40 long newcomer | 8.801 s | 2.446 s | 50 / 0 |

These are complete dictionary-query process observations, including identification, packing and
materialization; they do not include physical-deck predictions. Filesystem caches were not forced
cold. Zero propagation does not make native compilation and file validation free.

Physical score validation uses the unchanged original PR40 veteran inventory and this legal deck:

| Position | Member | Paired Snap |
| --- | ---: | ---: |
| 0 | 1 | 42 |
| 1 | 28 | 1 |
| 2 | 43 | 2 |
| 3 | 49 | 3 |
| 4 | 64 | 4 |

All five members are LUCK writers; Snap 42 also contributes a real LUCK support skill. Native
legality checks retain five distinct characters and the original owned cultivation. Every one of
the 120 orders was compared on both original short and long veteran requests, for **240 orders**.
The lookup joint intervals enclose the independent native nominal reference, and all U24
`scoreAtLookup` values equal the corresponding `scoreAtMean` values on these inputs.
Each chart's 120 native-compiled orders identified one complete program, requiring one propagation
cold and none warm. This equality was checked from the complete programs in those contexts; it is
not a rule that performance positions can be ignored in other queries.

| Original request | Cold query and 120 predictions | Warm query and 120 predictions | Separate full native reference generation |
| --- | ---: | ---: | ---: |
| Short veteran | 1.848 s | 1.573 s | 28.19 s |
| Long veteran | 2.598 s | 2.243 s | 32.35 s |

Query times include native identification, dictionary work and full weighted-score replay for all
120 orders. Reference times are separate process observations, not Monte Carlo runs or end-to-end
search timings. The zero observed score-proxy difference is specific to these decks and inputs.

### Conditional dictionary on the same original owned deck

The same legal member/Snap deck and unchanged short/long veteran inputs were then rerun through
`validate_interactions.py --backend basis` at the newer `0164248…` simulation source. Independent
references were regenerated with that same binary for every original order. Fresh empty dictionaries
identify each chart's 120 orders as **eight unique conditional programs**: eight propagations cold,
zero warm. Each query retains all 120 order labels and all 960 conditional-term references.

Across all **240 physical orders**, joint and weighted intervals overlap the independent original
native responses, lookup intervals enclose their reference endpoints, and transition masks, probe
flags and range moments agree. The intervals themselves are not bit-identical after quantization.
Every U24 `scoreAtLookup` equals its corresponding native `scoreAtMean` in both cold and warm queries,
with zero maximum or mean absolute score-proxy error. Cold/warm materialized responses and packed
archives are byte-identical. The [conditional audit](luck-response-conditional-audit.json) records
the original input hashes, native reference receipts and complete validation summaries.

| Original request | Cold / warm DP terms | Cold query and 120 predictions | Warm query and 120 predictions | Separate current-source native reference |
| --- | ---: | ---: | ---: | ---: |
| Short veteran | 8 / 0 | 3.752 s | 1.990 s | 26.29 s |
| Long veteran | 8 / 0 | 5.321 s | 2.408 s | 30.57 s |

The full validation process, including planning, reference validation, both queries and comparison,
took 6.937 seconds for short and 8.848 seconds for long; the separately generated references are
excluded from those totals and from the query columns. The earlier direct-program and newer
conditional measurements use different source revisions, so they are not a controlled timing ratio.
Their generation counts illustrate the intended tradeoff for this particular deck: the direct path
used one whole-program DP, while the conditional path needs eight terms that can later be shared
across admitted minimum-probability families.

## Complete dictionary compression

The following measurements cover all **7,216** merged complete-program responses from the audited
TW/JP run, not a sampled chart. The payload is one open program index and its unique ONLRSP blobs;
original datasets, native generation reports and combination/job labels are excluded.

| Encoding | Index + unique blobs | gzip-9 bundle | XZ-6 bundle | Zstandard-9 bundle |
| --- | ---: | ---: | ---: | ---: |
| Lossless | 29,570,573 B | 20,880,501 B | 16,176,040 B | 17,755,106 B |
| U16 | 14,517,010 B | 6,400,335 B | 3,222,908 B | 4,140,881 B |
| U24 | 16,429,514 B | 8,249,710 B | 4,591,788 B | 5,712,103 B |
| U32 | 18,322,347 B | 10,009,450 B | 5,849,572 B | 7,101,634 B |

The 292 raw native reports occupy 2,671,191,324 bytes and retain per-job metadata for audit. They
are not needed in the deployed dictionary. The compressed columns include a deterministic USTAR
container: index first, blobs in SHA order, fixed file metadata, and deterministic codec settings.
**All 12 compressed bundles** were decompressed and checked against the exact original TAR byte
length and SHA. The [audit JSON](luck-response-catalogue-audit.json) records every compressed and
decoded digest. The smallest published payload in this table is U16/XZ; U24 is the default query
precision because its probability grid is finer and the real-deck checks showed no U24 score-proxy
difference on the measured inputs.

`ONLRSP02` delta-encodes time, run-length encodes repeated buckets, XOR-encodes successive lower
endpoints and pairs each upper endpoint with its lower endpoint. Lossless encoding preserves the
binary64 endpoint bits. U16/U24/U32 use grids of `2^-16`, `2^-24` and `2^-32`, with lower endpoints
rounded down and upper endpoints rounded up. Exact zero/one and the original range moments are
preserved. Every packed key is decoded to check exact lossless equality or quantized enclosure.

Quantization therefore widens probability enclosures. It does not imply a full-score error bound.
The predictor's midpoint choice is separate from storage. The earlier PR40 pilot below measured a
nonzero U16 score-proxy error while U24/U32 agreed with its reference. Smooth curve fitting was not
assumed safe across native discontinuities.

Unpack the outer compression before lookup. The extracted native index supports individual
program reads. Default codec limits separately bound an archive to 64 MiB, its index to 8 MiB and
decoded ownership to 32 MiB. These are not a total process-RSS bound. Native identities, compiled
programs, responses, serialized reports and scorer memory have separately reported allowances.

## Incremental source updates and automatic generation

The current [workflow](../.github/workflows/luck-response-tables.yml) acquires both complete
published catalogues once, builds one source-bound native binary, and shares that exact bundle with
all 16 workers. The versioned partition is `scoreId % shardCount`; new music cannot move an existing
score into another shard's cache. A full result requires exact union of every shard, original
chart/difficulty and native task label, plus verification of all blob bytes.

The earlier completed `de589e3` run used catalogue ordinal partitioning. Its receipt continues to
describe that historical source and rule. The current stable partition and cache prefix are
versioned independently; old artifacts are not relabelled as a new run.

The mechanisms have deliberately different invalidation granularity:

| Path | Unit of reuse | When new work is needed |
| --- | --- | --- |
| Whole catalogue runner | Verified complete chart with all requested master keys | A current native plan must match; changed chart dependencies or key sets rebuild that chart |
| Direct program query | Complete compiled controller address | Only requested missing or corrupt program blobs need propagation |
| Conditional basis query | Complete conditional controller address | Only requested missing terms need propagation; new weights can reuse existing terms |
| Legacy entry pipeline | Native shared and per-entry dependency descriptors | Only changed/missing entry tasks regenerate, subject to shared-context invalidation |

For an unchanged input pin and valid archive bytes, whole-chart resume makes no native call. After
a dataset or snapshot repin, a fresh native `plan` binds the current data/snapshot/request/spec
bytes, the entire requested catalogue and every selected dependency. Reuse requires the same
algorithm source, shared descriptor and full response context as the verified earlier generation.
The original report and its input provenance are preserved. Separate hashed `resumePlan` receipts
and `reusedThroughPlan` establish current applicability, and aggregate validation checks them again.

This was exercised on the unmodified JP 10000300 Easy input with the full 1,201 jobs and a private
copy for controlled invalidation checks. The [resume audit](luck-response-resume-audit.json) records:

| Stage | Process wall | Native plan calls | Generated programs |
| --- | ---: | ---: | ---: |
| Original input, cold | 4.983 s | 0 | 49 |
| Original input, warm | 0.367 s | 0 | 0 |
| Unrelated member field changed and dataset/snapshot repinned | 0.785 s | 1 | 0 |
| Same repin, warm | 0.547 s | 0 | 0 |
| Selected GK7 level 1 effect changed in the private copy | 5.140 s | 1 | 49 |

The unrelated update preserved the old generated report SHA exactly. The selected skill update
changed the native context and correctly forced generation. All 348 original input files remained
byte-identical. The modified-master stages are controlled cache-invalidation checks, not additional
published gameplay inputs or performance acceptance cases.

Source identity conservatively covers crate Rust source, manifests/lockfiles and the generator.
An unrelated Rust change can therefore invalidate responses. The native simulation source hash
is embedded; executable SHA and the same-checkout CI build supply further provenance. Local callers
must supply the source tree used for their generator. A persisted hash is a data address, not a
signature or a transferable native search capability.

After the complete audit succeeds, CI creates a separate `luck-catalogue-program-dictionaries`
artifact with gzip/XZ bundles. Raw shard evidence is retained for 7 days; deployable dictionaries and
the full audit are retained for 30 days. The current workflow supports PR/push/manual runs and a
daily **13:17 UTC** refresh once it is on the default branch. Scheduled execution does not begin
merely because the workflow exists on a PR branch. It has read-only repository permissions and
does not commit generated tables or send comments.

The additional `interactions` job consumes the same pinned catalogue bundle and same-source binary.
It runs `validate_minimum_basis.py` on TW 10006103/10010703 Expert and JP 10000300 Easy, preserving
the complete 1,250-job identity grid, warmup, generation-disabled reuse, independent original-DP
comparisons and nonminimum control receipts. The artifact `luck-real-multiskill-validation` is
retained for 30 days. Its TW catalogue anchors are distinct from the unchanged original PR40
newcomer snapshots used for the measured conditional table above. Full chart generation and
these interaction checks have separate job outcomes.

## Reproduction entry points

The [tool README](../tools/luck-tables/README.md) gives acquisition, generation and query commands.
The principal entry points are:

| Tool | Purpose |
| --- | --- |
| `catalogue.py` | Acquire verified real TW/JP data; construct an explicit anchor; declare every chart |
| `stream_runner.py` / `aggregate_catalogue.py` | Generate complete stable shards, resume by native dependencies, audit the exact union |
| `programs.py` | Compile requested real skill combinations and persist only missing complete programs |
| `basis.py` | Identify whole-live conditional minimum terms, persist missing terms and materialize requested mixtures |
| `make_minimum_basis_specs.py` | Derive the complete 1,250-job minimum grid and query/control subsets from actual master rows |
| `validate_minimum_basis.py` | Validate real-chart conditional families, original-DP comparisons, unseen combinations and nonminimum invalidation |
| `validate_interactions.py` | Compare legal original decks over every native 120-order label, using direct or conditional cold/warm dictionaries |
| `analyze_programs.py` | Audit dictionary coverage, deduplicate actual blobs and measure deterministic compressed bundles |
| `analyze_start_operators.py` | Derive exact source-class and maximum-law relationships from actual master probabilities |
| `pipeline.py` / `analyze_responses.py` | Reproduce the legacy entry pipeline and its response/fit analysis |

The checked-in [interaction fixtures](../tools/luck-tables/fixtures) supply actual master skills and
the legal PR40 veteran deck. The [full48 source](../tools/search-harness/fixtures/full48/source.json)
retains the original PR40 dataset pin. Inputs to measurements come from those published charts,
master rows and original inventories. Small synthetic descriptors are used only for correctness
tests of hashing, error handling, native transport, codec and resume contracts.

After preparing the original full48 corpus and complete JP inputs as shown in the tool README,
reproduce the measured conditional cases with:

```sh
python3 tools/luck-tables/validate_minimum_basis.py work/full48-inputs/benchmark.json \
  --case short-newcomer-score --case long-newcomer-score \
  --generator tools/search-harness/target/release/luck_response \
  --output work/minimum-basis-pr40 --mode u24
python3 tools/luck-tables/validate_minimum_basis.py work/catalogue/jp/inputs/benchmark.json \
  --case jp-10000300-easy --generator tools/search-harness/target/release/luck_response \
  --output work/minimum-basis-jp --mode u24
```

Each output directory must be empty so its warmup starts from an empty dictionary. The validator
checks original input hashes before and after its work. For physical decks, add `--backend basis`
to `validate_interactions.py` to run the same native 120-order reference comparison through the
conditional dictionary; `--backend programs` is the direct-program default. A saved reference can
be reused only when its source, original inputs, deck specification and provenance still agree.

`validate_interactions.py` retains all orders even if a lookup is missing or differs. A deck's
uniform-120 proxy mean is published only when every order succeeds. The native predictor rebuilds
the current dependency context before decoding: changed chart/play/source data causes
`contextMismatch`; absent combination keys remain missing; older `ONLRSP01` payloads are refused.


## Appendix: limited PR40 pilot on 2026-10-08

The following measurements predate the full-catalogue run. They cover only the original PR40
corpus: 36 LUCK requests, six Expert charts and ten chart/play contexts. They are retained for
comparison of the early entry pipeline and codecs, not presented as current full-game coverage.

### First codec prototype

These observations belong to the earlier [probability-table prototype](https://github.com/empty-sekai/ournotes-deck/commit/17cf77d17a749cbf07323599d518f8c00460794e),
tree `81f798f920795779fe37eb0a9a6d55784f548edb`, and are distinct from the subsequent limited
`ONLRSP02` generation and prediction measurements below:

- The two 26-entry newcomer subset runs completed all 52 jobs. Each context recorded 26 cache
  lookups, 22 hits and four actual DP propagations: eight propagated curves in total. With the
  cache disabled, each context propagated all 26 entries. All distinct position keys were retained.
- Four legal original newcomer decks per chart, containing zero through three writers, were compared
  over all 120 orders on both charts: 960 order observations. Every corresponding four-bucket curve,
  transition mask, probe flag and range-moment payload matched, and every `scoreAtLookup` equaled
  the corresponding `scoreAtMean`. This compares two weighted-score predictions; it is not a native
  expected-score or global-ranking oracle.
- Additive single-response reconstruction and a truncation through pairwise interactions did not
  describe the complete observed combinations accurately. The maximum Rush-probability residual was
  approximately 0.94 for the single-interaction model and 0.924 for the pairwise model on these
  responses. Some reconstructed bucket values left the probability range. These are measured
  counterexamples to those compositions, not a claim that every possible fitted model must fail.
- Piecewise-linear fits were checked against the complete integer-millisecond domain of each
  supplied step response, including both sides of discontinuities and a rounding allowance. At a
  requested `1e-4` tolerance, four unique responses required 1,277 knots on the short chart and
  1,206 on the long chart. The tested fit representation did not beat the tested fixed-grid archive
  representation in compressed size. Its residual bounds apply to those probability curves only,
  not to unseen combinations or native total score.

The prototype observations can be reproduced from `short-warm.json`, `long-warm.json`, their cold
counterparts, the two `full-orders` reports and `subsets-analysis.json`. The two full-order reports
have SHA-256 values `3c49e53d04376b9212c556b8ab50813af5b410941c81ba2b5b84b778e31d6822`
and `9ff6877001de9f89fdef69593be86c7bb52fa1182de35ee582b75ef8d6bbba8f`; the analysis report has
SHA-256 `60fe02f491f80a3925d50fe9ad19209e860bf78be7c2f609e7e9c069e939f344`.

### Entry-pipeline measurement at `0fb5e03b`

These pilot generation, compaction and prediction measurements use the tree published at
[public source `0fb5e03b`](https://github.com/empty-sekai/ournotes-deck/commit/0fb5e03b21e62ec6f253d69bb83efd3c6b37e808)
in [PR #43](https://github.com/empty-sekai/ournotes-deck/pull/43), tree
`0e4df7cd503be10b4cbc465010ce7a0599596240`, and generator SHA-256
`294ee1bc5b205469186e868d3fc9389bd9167f3a6c0e7194a340c6fc085b0041`.
The pinned dataset SHA-256 is
`de867d2df3020e9430c164cdc889cd114113e50b2ab12003b6978665905493bf`.
These are local native measurements of the offline pipeline, not search-request runtimes or a CI
performance comparison. All selected entries succeeded; no refusals or capacity errors were omitted.

| Phase | Coverage/work | Wall seconds |
| --- | --- | ---: |
| Plan base/single entries | 650 entries, ten contexts; no DP | 7.377918 |
| First generation | 650 generated, ten native batches, 100 DP propagations | 13.351420 |
| Unchanged generation rerun | 650 reused, zero generated, zero native batches | 1.533951 |
| Plan with declared combinations | 690 entries, ten contexts; no DP | 7.278064 |
| Add combinations to existing store | 650 reused, 40 generated, two native batches, four DP propagations | 2.569208 |
| Compact complete contexts | All 690 entries, all four encodings; no DP or MC | 13.862906 |

Planning, input materialization, compilation and compaction are **not included** in the 13.351420-second
first-generation phase. Across its ten batches and the two incremental combination batches, the native
cache reports 690 lookups, 586 hits and 104 propagated curves. Summed DP propagation time is
4.529419 seconds; summed native recording time is 1.675450 seconds. The 12 native generation-function
reports total 6.777674 seconds including their context/descriptor work; their individual job timers
sum to 6.284634 seconds. These nested counters are not additive with the wall-time table.

All ten contexts were compacted independently and every key was decoded and verified in every mode:

| Encoding | Total archive bytes | Sum with gzip-9 outer compression | Verified entries | Maximum observed endpoint widening |
| --- | ---: | ---: | ---: | ---: |
| Lossless | 666,655 | 480,146 | 690 | 0 |
| U16 | 302,745 | 114,132 | 690 | `2^-16` |
| U24 | 355,535 | 164,046 | 690 | `2^-24` |
| U32 | 408,411 | 212,419 | 690 | `2^-32` |

The archive column includes each archive's own index and payloads, without an outer compression
container. The gzip column sums ten independently compressed context archives. The original response
JSON tables occupy 27,578,560 bytes across twelve generation batches; this is not an identical
container layout to the ten compact archives, so the size difference includes compaction and payload
sharing as well as encoding. Outer compression must be removed before indexed lookup.

For each mode, the ten resident indexes total 76,570 bytes, and the largest single decoded response
owns 23,749 bytes. Complete verification lookup time totals 10.330 ms for lossless, 6.918 ms for U16,
7.832 ms for U24 and 8.265 ms for U32; this is source-comparison work during compaction, not native
score prediction or a cold-file read benchmark. Compaction reports complete coverage and no missing
entries. Its manifest SHA-256 is
`0bf94935418c3c28d8085c414650e20f737789e44d8a30cc9c060c424f81e8b5`;
the generation/compaction phase receipt SHA-256 is
`5734b695d8c4700d3d03d51c7f34467aac314e3e896ef0b51909513a064fa32f`.

The pilot consumer was run in a fresh process for each of the four encodings on each of the two
original newcomer requests. Each run scored four legal decks over all 120 orders, with no decoded
cache, DP or MC. All eight files have success status: 960 matched order observations per encoding,
3,840 predictions in total, with no missing or refused orders. The independent comparison checks
original dataset/request identities, the original inventory or its existing complete matrix roster
projection, physical member/Snap order, and the exact 120 permutation labels. The reference remains
the earlier full-deck `scoreAtMean` predictor, with its different source identity recorded explicitly.

| Encoding | Short: 480 predictions, process wall seconds | Long: 480 predictions, process wall seconds | Exactly equal to reference, out of 960 | Maximum absolute score difference | Mean absolute score difference |
| --- | ---: | ---: | ---: | ---: | ---: |
| Lossless | 1.543732 | 1.997093 | 960 | 0 | 0 |
| U16 | 1.494247 | 2.045106 | 130 | 21 | 6.208333 |
| U24 | 1.410407 | 2.222255 | 960 | 0 | 0 |
| U32 | 1.444645 | 2.211279 | 960 | 0 | 0 |

Process times include loading inputs, checking context and native score replay, and do not include
archive generation or compilation. They are single observations with no application response cache;
the operating-system filesystem cache was not forced cold. Across the eight runs, all 480 lookups in
one run take approximately 6.2–12.2 ms, while native scoring takes 1.33–2.13 seconds. Each run accesses
26 distinct keys. Peak decoded ownership is 19,922 bytes on the short request and 22,455 on the long
request. Thus native weighted replay, rather than blob decoding, dominates this prediction workload.
The zero observed U24/U32 score difference is restricted to these inputs and does not establish a
zero score-error bound for other teams or charts.

A further scalar fit groups the 480 reference observations per chart by their complete ordered writer
key, including level, formation, position and multiplicity. Within each group it chooses the
least-squares coefficient `c` for `s0 + c * (s1 - s0)` using exact rational accumulation. All 26 groups
on each chart show coefficient variation. The largest residual of this fit against the weighted-score
reference is 2,427.668 points on the short chart and 1,691.146 on the long chart. The writer key omits
the timing and identities of other ordinary effects; those inputs still matter to score. This finding
supports retaining the time response for native replay rather than claiming a universal exact scalar
from the selected LUCK skills. It does not measure error against full native expected score.

The pilot prediction comparison report SHA-256 is
`b75b9721fa7744f6c6480e4263fc869ac9c7a987554067d2f2590674ea5eab59`;
the compression summary SHA-256 is
`0159bbcaa053fcbac4d3568e1bed8a2175192c626fde4aa3b14ddea676461db8`.
These are source-pinned local pilot measurements. The completed full-catalogue CI audit and its
separate source identity are documented at the start of this report.

## Remaining search boundary

The [search benchmark status](search-benchmark-status.md) records original LUCK requests that remain
TimedOut/unproven. A measured short-newcomer baseline spent about 41 seconds in family preparation
even after controller-program reuse. That preparation includes complete original physical-pair
admission; other sources expose expensive terminal recording and scoring instead. Moving or shrinking
one stage does not establish faster completed ranking.

Importing a controller response does not discharge the original pair checks, prove a LIFE/conversion
closure for an arbitrary team, or eliminate native terminal scoring. The present offline path therefore
does not claim to remove that roughly 41-second admission/preparation barrier. Integrating it into
certified search would require a separately justified complete-input admission and score-bound
contract, with truthful fallback for unsupported inputs and full original-domain/120-label ranking
semantics. That integration is outside the current experimental predictor.
