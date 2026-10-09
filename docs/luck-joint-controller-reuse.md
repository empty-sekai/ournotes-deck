# Joint minimum families and native speed profiles

The conditional response dictionary separates a complete controller from the probability law of
its range-start minimum guarantees. `canonicalMissGauge: true` supports the same bounded recording
family session as the ordinary minimum basis. Every requested holder configuration still passes
native construction and admission. The optimization changes recording and identity work; responses
remain independent-nominal controller probabilities.

## Reusing a joint Miss/minimum recording

For one immutable master, chart, play, setup and observer context, a recording-family key contains:

1. The complete initialized model after removing admitted minimum-only condition updaters.
2. Every retained effect row paired with its original native row index.
3. The ordered nonminimum actions and their conditions.
4. The validated observer flags and observer mode.

A minimum updater qualifies only when all its effects are minimum writers and every effect
condition is statically false under the native weighted recorder. LIFE-dependent or unknown
conditions retain the independent recording path. Each omitted minimum row must belong to an
admitted updater, and a live effect cannot reference an omitted row. No projected model executes:
the original rows, conditions, notes and events are restored before recording or returning.

This representation admits a minimum row between two Miss rows when only its level or probability
changes. The retained Miss rows keep the same indices and complete metadata. Inserting or removing
an interior row can change those coordinates and therefore changes the family. A trailing suffix
of minimum-only rows contributes no retained coordinates, so its multiplicity can vary.

Let `K(x)` denote this complete private key and let `A(x)` be the ordered minimum actions for an
originally admitted request `x`. If `K(x) = K(y)`, the weighted recorder has the same initial state,
retained updater order, frame geometry, notes and nonminimum actions for both requests. Each omitted
updater has no effect on that recorded path. Induction over the native frame updates therefore gives
the same remaining recording. The first recording must additionally match `A(x)` at every actual
range start before it establishes a reusable family.

The minimum probability law remains request-specific. For independent minimum writers with chance
`p_i` and minimum `m_i`, the maximum newly written minimum `M` has

\[
\Pr(M\le m)=\prod_{i:m_i>m}(1-p_i).
\]

Each whole-live conditional program fixes the new minimum at every actual start. It retains the
existing minimum state, Rush and native completion tails. A later family member changes only these
mixture weights and the supported minimum choices. Conditional identities are cached by the private
family ID and the actual start-choice tuple, with complete equality establishing every first entry.

### Preserving the Miss quotient's admission

The family selects its identity mode before its first request. The first member is completely
recorded, then passes the existing Miss-gauge domain checks before any basis is retained. These
checks cover separately rounded increments, all admitted gauge maxima, nonwrapping arithmetic,
frame dependencies, original action coverage and workspace limits. Minimum-only reweighting changes
none of these dependencies, so the same admitted domain remains valid for later members.

Original Miss rows and their original effect values remain in the recording-family key. Two
different Miss configurations can require separate recordings even when their increment vectors
agree. Their conditional responses share only after the complete canonical program identities
compare equal. This separates the authority to reuse a recording from the authority to reuse a DP
response.

The session retains at most 256 families, subject to its byte allowance. Its conditional choice
index remains bounded by 64 choices per family. A family that does not fit uses an independently
recorded canonical basis. Original native construction failures and unadmitted Miss domains remain
errors. The diagnostic `familyRecordingContract` is `minimum-only-original-row-indices/2` when the
session is enabled; actual recordings, hits, fallbacks, identity work and retained payload bytes are
reported separately.

## Complete declared joint parameter grid

`validate_joint_families.py` uses the actual GK8, GS66 and GS96 source rows. Every kernel fills the
five main and ten support slots. The declared grid varies four GS96 levels independently over 1–5
and two GS66 levels independently over 1–5, with the remaining levels fixed at 3 and unmatched
formation branches:

\[
5^4\times5^2=15{,}625\text{ requested kernels per chart.}
\]

The five deterministic shards each contain 125 complete Miss tuples and all 25 minimum pairs.
Each shard has an explicit 128 MiB family allowance and preserves the native 256-family limit.
Both the family path and the independently recorded control identify every job without DP. Their
complete conditional fingerprints, start choices and interval weights must agree exactly.

The verifier then selects two different minimum distributions for each distinct conditional
mapping shape. Every conditional program referenced by the complete grid must participate in this
subset. Selected mixtures are compared with independently propagated original whole-live programs.
Cold lookup generates the missing conditional responses; warm lookup forbids generation. Both
materialized and final U24 responses are checked, and final U24 intervals must contain the original
reference endpoints. This is mixture-level original-DP validation with complete conditional-ID
coverage, rather than a separate original-DP run for every deterministic conditional term.

Sequential shard payloads are reported through their maximum retained bytes. These counters exclude
the input objects, output JSON, temporary admission data, allocator overhead and native DP workspace;
they do not represent total process RSS.

### Measured joint-grid results

The original PR40 short and long newcomer requests and JP 10000300 Easy all pass the full declared
grid. The long request keeps its original four Miss judgments. These are actual-master parameter
kernels with all fifteen skill slots occupied; no claim of owned-card or distinct-character deck
legality is made for the synthetic slot combinations. Exact input and result hashes are recorded in
[the machine-readable audit](luck-joint-controller-audit.json).

| Original context | Grid requests | Independent recordings | Family recordings | Family hits | Conditional programs |
| --- | ---: | ---: | ---: | ---: | ---: |
| PR40 short newcomer | 15,625 | 15,625 | 625 | 15,000 | 352 |
| PR40 long newcomer, four Miss | 15,625 | 15,625 | 625 | 15,000 | 352 |
| JP 10000300 Easy | 15,625 | 15,625 | 625 | 15,000 | 352 |

All 46,875 complete job mappings and 375,000 conditional-term references agree exactly with the
independently recorded path. Identity comparison runs no DP. All three runs have zero family
fallbacks. The recording reduction is 25-fold within each declared grid. The 625 original Miss
tuples remain different recording families; the Miss quotient then gives 44 conditional mapping
shapes. This GS66 grid writes a new minimum of 0 or 1 at each of three starts, giving eight whole-live
choice tuples per shape. Thus response generation uses 352 conditional programs
per context. This count does not imply that different original Miss configurations can share a
recording before the complete canonical identity check.

| Original context | Independent identification | Family identification | Observed ratio |
| --- | ---: | ---: | ---: |
| PR40 short newcomer | 89.217 s | 17.056 s | 5.23× |
| PR40 long newcomer, four Miss | 120.984 s | 20.231 s | 5.98× |
| JP 10000300 Easy | 107.333 s | 18.784 s | 5.71× |

These are sums of the five sequential shard wall times, including process startup and report I/O,
observed on one shared machine. They are single measurements, not an isolated CPU benchmark or a
full-roster search result. The same Rust 1.90.0 release binary was used throughout, with debug data
disabled, eight code-generation units and incremental compilation disabled. Its SHA256 is
`dbc55333eb421acba09e134ef005de2e000f36ba06d641c246c2d388bafe24fd`.

The maximum retained family payload per shard is 40,031,972 B, 41,103,472 B and 29,339,222 B,
respectively, with 125 retained families. These are component counters, not process RSS or the sum
of all five sequential shards. The 128 MiB shard allowance and native 256-family limit stay enforced.

### Independent probability and archive checks

For each context, two different minimum distributions are selected for each of the 44 shapes.
The resulting 88 independent original whole-live DPs cover every conditional program referenced by
the grid. Across the three contexts, that is 264 original mixture references and 1,056 conditional
program propagations. No Monte Carlo samples are used.

The saved native conditional responses were additionally packed losslessly and re-materialized
without any new recording or DP. Comparison with the saved independent original DPs gives:

| Original context | Joint intervals | Separated native intervals | Maximum native endpoint difference |
| --- | ---: | ---: | ---: |
| PR40 short newcomer | 70,752 | 0 | 9.10×10⁻¹⁵ |
| PR40 long newcomer, four Miss | 66,176 | 0 | 9.44×10⁻¹⁵ |
| JP 10000300 Easy | 54,560 | 0 | 7.44×10⁻¹⁵ |

All 191,488 native interval pairs intersect; probe flags, transition masks and range moments agree.
Native endpoints are not all identical, and native reconstructed intervals do not always contain
both reference endpoints. The lossless codec round trip is exact.

The separately checked final U24 archives contain both original-reference endpoints in every one
of those 191,488 intervals. The largest final-U24 endpoint difference is approximately
1.192093×10⁻⁷, after conditional-response quantization and final-response quantization. That value is
an archive enclosure measurement, not the native DP rounding error. Cold and warm mappings,
materialized response bytes and final ONLRSP archives agree exactly.

### Cold generation and warm queries

| Original context | 88 direct original DPs | Cold dictionary generation and query | Warm query for 88 selected jobs |
| --- | ---: | ---: | ---: |
| PR40 short newcomer | 36.574 s | 150.477 s | 0.887 s |
| PR40 long newcomer, four Miss | 34.475 s | 139.126 s | 1.196 s |
| JP 10000300 Easy | 14.712 s | 59.408 s | 1.024 s |

Each cold query generates 352 conditional programs. Each warm query forbids generation and performs
zero DP, while still constructing and admitting all requested holders, recording 44 remaining
controllers and obtaining 44 family hits. This table measures the 88 selected materialized queries;
it does not time materialization of all 15,625 grid requests. Building the reusable dictionary costs
more than directly evaluating these 88 mixtures once. Later requests with admitted controller
identities can reuse those responses, which is the purpose of this decomposition.

### Measured dictionary storage

The existing dictionary import/save and deterministic compression utilities were used to assemble
portable indexes with the generated U24 response blobs. Index normalization retains `fingerprint`,
`sourceVersion`, `status` and each archive's `path`, `sha256`, `bytes`, `mode` and `verifiedEntries`;
other diagnostic fields are omitted. Every probability payload remains unchanged. The original diagnostic
index-plus-response sizes were 2,130,079 B, 2,007,125 B and 1,636,486 B; index cleanup is accounted for
separately from response compression.

| Declared joint-grid context | Programs | Portable index plus U24 responses | gzip9 | XZ6 |
| --- | ---: | ---: | ---: | ---: |
| PR40 short newcomer | 352 | 2,048,658 B | 1,073,203 B | 212,892 B |
| PR40 long newcomer, four Miss | 352 | 1,925,938 B | 976,327 B | 204,740 B |
| JP 10000300 Easy | 352 | 1,554,946 B | 729,059 B | 157,260 B |
| One archive of these three contexts | 1,056 | 5,528,922 B | 2,971,456 B | 573,120 B |

Pairwise intersections of complete native fingerprints and blob hashes are empty across these
contexts. The combined 1,056 programs therefore include no additional cross-context aliases.
The three separate XZ packages total 574,892 B, so joining them saves only 1,772 B. Joining the gzip
packages increases their total by 192,867 B. All four gzip/XZ archive pairs restore identical expected
TAR bytes and every member matches its original canonical index or U24 response blob.

This audit invokes neither the native generator nor DP. It verifies that 1,446 protected source,
input, report, index and response files remain unchanged. Its 573,120 B package covers only these
three declared joint grids; it is separate from the full-catalogue base/single and minimum-family
dictionaries. No RSS or query-speed improvement is inferred from these byte counts.

## Speed requires the complete activation profile

Current admitted integer speed effects can be organized by their native activation schedules.
For a fixed schedule class `d`, let `c_d` be the sum of its integer speed coefficients. At a native
update time `t`, the aggregate modifier is

\[
v(t)=\sum_d c_d\,\mathbf{1}\{d\text{ is active at the native update at }t\}.
\]

The native phase, mission gate, timer boundary and sustained lifetime define whether a schedule is
active. A total integral or one initial value cannot replace this profile. For example, two unit
speed effects lasting 2 and 5 seconds have the same initial modifier and total duration integral
as two effects lasting 3 and 4 seconds; their intermediate profiles differ.

The actual positive integer coefficients permit exact binary32 sums within the validated bounded
domain. This argument does not extend automatically to fractional future effects or a different
update order. The speed verifier checks the complete source rows and still asks the native compiler
to establish every complete-program identity.

### Other effects remain controller dependencies

GK22 includes a damage-reduction effect in addition to its speed effect. That additional effect can
alter LIFE and therefore later LIFE-dependent controller conditions. Equality of the speed profile
alone cannot identify such a mixed controller. The current speed experiment uses isolated speed
kernels and retains the complete actual source-row receipts; it does not authorize erasing
damage-reduction effects from a controller that reads LIFE.

`validate_speed_profiles.py` enumerates the complete zero-, one- and two-source grid of the declared
20 actual source/level entries, including both orders. It checks schedule-profile grouping against
complete native identities, independently propagates selected original witnesses, and retains an
equal-integral counterexample. Arithmetic profile counts through five source slots describe the
declared parameter domain; card ownership, distinct-character restrictions and other skill effects
are additional constraints.

### Measured speed profiles on three unchanged requests

The original PR40 short and long newcomer requests, and JP 10000300 Easy, were checked with
the same release binary built from feature commit
[`c9fdb6ff20d248eecebe7e244b48d863213420cb`](https://github.com/empty-sekai/ournotes-deck/commit/c9fdb6ff20d248eecebe7e244b48d863213420cb).
The original long request retains its four Miss judgments. Each experiment enumerates
`1 + 20 + 20² = 421` empty, single-source and ordered two-source kernels. The source keys are
GK7, GK8, GK9 and GK22, each at all five levels. Source values 10000, 20000 and 30000 give exact
integer coefficients 1, 2 and 3 in this domain. The six schedule coordinates are native lifetimes
2, 3, 4, 5 and 6 seconds, and the sustained schedule.

| Original context | Native recordings | Complete native programs | Independent original DPs | Compared joint intervals |
| --- | ---: | ---: | ---: | ---: |
| PR40 short newcomer | 421 | 138 | 18 | 6,432 |
| PR40 long newcomer, four Miss | 421 | 138 | 18 | 6,016 |
| JP 10000300 Easy | 421 | 138 | 18 | 4,956 |

All equal schedule profiles in each complete declared grid have equal complete native identities.
The 24 selected alias-pair comparisons cover 17,404 joint intervals with identical endpoints,
probe flags and transition masks. The equal-integral counterexample has different complete native
identities. These runs perform all 421 native recordings per context; no speed recording-family
fast path is implemented.

The exact arithmetic enumeration of schedule vectors gives:

| Number of source slots | Profiles with exactly that many entries | Profiles with up to that many entries, including empty |
| --- | ---: | ---: |
| 1 | 16 | 17 |
| 2 | 131 | 138 |
| 3 | 696 | 724 |
| 4 | 2,726 | 2,810 |
| 5 | 8,544 | 8,754 |

Thus the declared `21⁵ = 4,084,101` ordered source-level-or-empty choices map to 8,754 arithmetic
profiles. Only the zero-, one- and two-source grid above has complete native identity enumeration
in this experiment. The five-slot number is not a count of generated responses, valid owned decks,
or mixed controllers that read LIFE. Source-row checks retain GK22's second damage-reduction row;
its effect value is 2000 and its lifetime accompanies the speed effect.

## Reproduction and CI

Use unchanged acquired data, snapshot and request files:

```sh
cargo build --release --locked --manifest-path tools/search-harness/Cargo.toml --bin luck_response
python3 tools/luck-tables/validate_joint_families.py DATA SNAPSHOT REQUEST \
  --generator tools/search-harness/target/release/luck_response --source . \
  --output work/joint-families --timeout-seconds 1200
python3 tools/luck-tables/validate_speed_profiles.py DATA SNAPSHOT REQUEST \
  --generator tools/search-harness/target/release/luck_response --source . \
  --output work/speed-profiles --timeout-seconds 1200
```

The LUCK response workflow runs both gates on the pinned TW short and long catalogue contexts and
JP 10000300 Easy. These jobs consume the same verified input bundle and native binary as the complete
base/single and minimum-family catalogue runs. Each context retains its own success or failure
evidence. Nominal response reuse does not provide the expectation of the complete nonlinear native
score or a full-roster Top-K ranking certificate.

### Full-catalogue regression on the new source

The current feature's full-catalogue run accounts for all 692 published regional chart/difficulty
records: 292 natural LUCK records and 400 explicit `notApplicable` records. Its independently checked
base/single and minimum-family artifacts have the following actual U24 sizes:

| Declared full-catalogue table | Requests | Cross-region programs | Index plus response bytes | XZ6 bytes |
| --- | ---: | ---: | ---: | ---: |
| Base and every single source-level key/position | 350,692 | 7,216 | 16,429,514 | 4,596,340 |
| Complete 1,250-configuration minimum grid per LUCK chart | 365,000 | 2,364 | 9,464,195 | 748,976 |

The full minimum grid again records 292 families and obtains 364,708 hits. Its 1,752 original-DP
references cover 686,712 joint interval comparisons, with maximum endpoint difference
1.3877787807814457×10⁻¹⁴. Codec repacking generates no DP. These full-catalogue tables and the three
joint dictionaries have separate declared scopes; their program counts are not an assumed global
union of every possible controller.

This feature's base/single aggregate records 14,236 generated regional responses and zero reused
charts. The earlier source's zero-generation cache-hit result remains historical evidence for that
earlier run. Source-bound program keys prevent reuse across an unvalidated native source change.
Current sizes and hashes therefore come from the current downloaded artifacts.

The CI bundle uses native generator SHA256
`c30bbee2919524d0d408b8955cdea82db7db3dc8a328dfae4a74e4acda8a7d68`.
It is a different binary build from the local timing binary. Both bind simulator source
`ad74234b0ba6e6618dda331d667869335fe52e5de499a2b4d77727d3d130f7e4`
and algorithm digest `934a8058759eacf725651c511c0a3fb0c73d135dfcf71d3e59099e95b36d90f4`.
CI TW catalogue-anchor request and snapshot hashes differ from the original PR40 inputs; JP hashes
match. The machine audit records the two sets separately.

### Feature CI result and independently checked artifacts

All three workflows pass on feature commit
[`c9fdb6ff20d248eecebe7e244b48d863213420cb`](https://github.com/empty-sekai/ournotes-deck/commit/c9fdb6ff20d248eecebe7e244b48d863213420cb):

- [General CI](https://github.com/empty-sekai/ournotes-deck/actions/runs/37887856142): formatting,
  Clippy, complete release tests, diagnostics, the 134-test LUCK Python suite, synthetic correctness
  and the MSRV gate pass.
- [Full catalogue and real interactions](https://github.com/empty-sekai/ournotes-deck/actions/runs/37887856172):
  all base/single and minimum-family shards, aggregates, the existing complete Miss grids, and the
  three added joint-grid/speed-profile gates pass.
- [Chromium Worker](https://github.com/empty-sekai/ournotes-deck/actions/runs/37887856211): both
  reported and real smoke contexts pass.

The independent artifact audit checks eight downloaded input, joint, base and family artifacts
against their GitHub ZIP byte counts and digests, then verifies 3,083 extracted files, 715 bundled
input files, 327 algorithm source files and 36 orchestration scripts at the exact feature commit.
It recomputes the three CI joint mapping comparisons and saved U24/speed comparisons. It also checks
all five dictionary indexes and all 31,228 blob contents across the four base/single encodings and
the minimum U24 encoding. All ten gzip/XZ packages restore their expected identical paired TARs.

These dictionary indexes remain open collections of measured native programs; their own
`complete: false` does not contradict the complete declared chart/grid coverage in the separate
aggregate reports. The artifact audit does not rerun all raw base/single DP shards. Detailed
receipts, exact input differences and scope flags are recorded in the machine-readable audit.
