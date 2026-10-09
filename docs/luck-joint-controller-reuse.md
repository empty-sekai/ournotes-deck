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
