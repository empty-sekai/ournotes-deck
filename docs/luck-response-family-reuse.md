# Reusing native recordings and quotienting Miss-gauge interactions

This extends the [conditional response experiment](luck-response-tables.md) in PR43.
The implementation removes repeated native recording within a proven minimum family and adds an
opt-in identity quotient for separately rounded Miss-gauge additions. The original controller still
propagates every distinct retained response. Both paths remain experimental probability prediction;
complete full-roster LUCK Top-K search within 20 seconds is not established.

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

CI shares one pinned TW/JP input bundle and native binary, adds 2 regions × 4 family shards, and
requires a separate complete-family audit before publishing the compressed family dictionaries.
It also runs the real Miss experiment. Existing full-catalogue base/single generation, incremental
dependency validation and original interaction experiments remain in place. Raw family shards are
retained for seven days; full audits and compressed dictionaries for 30 days. Daily publication
refresh becomes active on the default branch after merge.

## Measurement status

The source implements the above gates and workflows. Local semantic validation passes 116 Python
checks, the new native Miss/family tests, the existing minimum tests and workspace Clippy with
warnings denied. Full-catalogue measurements for this extension must be tied to its own successful
same-source binary and completed audit; the older measurements in the linked research record belong
to their recorded earlier sources. Development-only timing is not used as final performance evidence.
