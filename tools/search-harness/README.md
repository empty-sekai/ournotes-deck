# Live / Snap search harness

This harness targets played Live with Snap skills, Gekisou score, specified event-song score and expected client event PT. It shares the current fixed-deck evaluator while independently enumerating the bounded physical search domain. Power and Skip are not substitute acceptance targets.

The scorer and latest-game certification remain separate dependencies. Agreement proves search equivalence for these declared model inputs, not native-game equivalence, a real root distribution or server-issued rewards. All synthetic inputs are labelled. Real datasets and account inputs must stay in ignored `work/` directories.

## Repeatable loop

From the repository root, with `CARGO_TARGET_DIR` unset (the harness is its own workspace and builds into `tools/search-harness/target`):

```sh
export BDON_HARNESS_OUT=work/corpus
cargo test --release --locked --test adapter_fixture_export export_search_harness_inputs -- --ignored
cargo build --release --manifest-path tools/search-harness/Cargo.toml --locked
python3 tools/search-harness/run.py run \
  --binary tools/search-harness/target/release/ournotes-search-harness \
  --suite work/corpus/suite.json --out work/reports/baseline
```

`run.py compare --baseline A/free-score.json --candidate B/free-score.json --out comparison.json` checks two reports of the same suite, for example from two builds. It rejects changed inputs, changed oracle values and changed ordered returned results. A scorer change is a separate model experiment, not a search change.

`check.sh` runs the whole local gate: formatting, Clippy (default features, `search-diagnostics`, `native-fixtures` and the harness), the core and harness tests, the Python tests, the recommendation WASM build, the synthetic suite export and a harness run over it. It reads `WORK` (default `work`) for the corpus and reports. `Dockerfile.framework` provides a matching toolchain:

```sh
docker build -f tools/search-harness/Dockerfile.framework -t ournotes-deck-check tools/search-harness
docker run --rm -v "$PWD":/src -w /src ournotes-deck-check bash tools/search-harness/check.sh
```

The first harness build generates its standalone lockfile; subsequent builds use it frozen. Experiments run in a fixed order within a report.

## Domain and result contract

Each case supplies immutable dataset, roster and search-request JSON paths, `oracleMaxCandidates`, experiments and optional dominance proposals. Paths resolve relative to the case file. Requests use the `ournotes-deck.search-request/1` contract. They must explicitly declare the scene, play and metric. Snap skills are included in the shared full simulation.

The independent domain enumerates every team once: five different characters, the leader in slot 2, the other members in slots 0, 1, 3 and 4 in ascending card ID order (the canonical layout), optional unique Snaps, and explicit includes/excludes. It does not use production allowed-pool resolution, member enumeration, assignment, bounds, cache or Top-K. Every team is evaluated by `evaluate_fixed` over its 120 performance orders. The harness ranks exact integer payoff numerators, then power, member IDs and Snap IDs (None first) of the canonical layout, matching the adapter contract.

The configured cap is checked before simulation. A domain exceeding it fails without emitting a complete oracle. The harness uses bounded correctness cases sized for exhaustive enumeration.

Every prefix cap the search can use is checked against the exact value of every oracle team: joint prefixes, per-order leaf caps (each against that order's exact payoff), character and resource prefixes, classes and class bindings, compositions and teams, bound modules, bonus and expected-bonus prefixes, per-member PT caps, suffixes and next pairs. An inadmissible cap fails the run. Composition and Snap-pairing prefixes are audited separately from joint pair prefixes. The power-only assignment DP has its own exhaustive binding test, including empty choices, negative increments and ties; PT integration also tests fewer legal bindings than K and nonmonotone reward tiers.

Gekisou score experiments may set `"schedule":"classes"` or
`"schedule":"classesWithResource"` beside `name`, `patch` and `repeats`.
The default is `production`. These diagnostic schedules enumerate bound-effect
classes and then every surviving Snap binding; they do not merge simulations or
assume score monotonicity in power. The latter schedule also retains
unique Snap resources in a rounded `P + r*A` assignment upper bound. All other
request semantics, oracle comparison and deadlines remain shared. Unsupported
objectives fail explicitly. The recommendation request has no schedule switch.

Class-prefix and within-class binding-prefix caps are checked against every
oracle team, with repeated caps cached only inside one immutable problem.
Cache capacity limits repeated computation, never which witnesses are checked.
Reports identify each schedule, its exact returned values and work counters;
same-policy binary comparisons reject a changed schedule.

For full-domain runs without an exhaustive oracle:

```sh
search_variant classesWithResource DATA ROSTER REQUEST OUTPUT
```

This diagnostic binary writes the schedule, ordinary outcome and bound explanations
of the best order of every returned deck. `TimedOut` remains an unproved incumbent, even if
it beats another schedule's incumbent.

`evaluate_fixed DATA ROSTER REQUEST DECKS OUTPUT` scores a JSON array of
`{"members":[...],"snaps":[...]}` proposals through the ordinary fixed evaluator.
It is useful for checking a score-search incumbent under a PT request. It performs
no search and grants no global optimality certificate; inspect each completion.

Experiments can change `strategy`, `limits`, or add member/Snap exclusions. They cannot change the scorer, metric, scene, play or K. Reports keep original-domain identity even for reduced-pool trials. `Complete` from a reduced experiment certifies only its reduced domain; the harness separately checks whether its results equal the original full-domain Top-K. Heuristic/timeout results must still match fixed-evaluator values. A full-domain wrong `Complete` or incorrectly evaluated returned deck fails the run.

Reports retain complete returned results with their best orders, oracle Top-K, candidate/simulation/cache/bound counters, exact Top-1 gap, input hashes, binary identity and source manifest.

`sourceManifest` hashes the current checkout's `crates/` (both scorer and search),
legacy `src/` and `tests/`, the harness, and the root Cargo manifest and lockfile.
Build output directories (`target`) and Python caches are excluded. Uncommitted
crate edits therefore change the manifest even when `sourceHead` is unchanged.
The manifest records the source files observed before the run; `binarySha256`
identifies the executable. These hashes alone do not attest that the executable
was built from that checkout; preserve the build command and toolchain separately.

## Fixed-deck audits

The following binaries take a `DECKS` JSON array of `{"members":[...],"snaps":[...]}`, write their report, and exit nonzero on any violation. They need no exhaustive oracle, so they apply to full-size rosters; the decks are typically the returned results of a search.

- `cutoff_audit DATA ROSTER REQUEST DECKS OUTPUT` settles after every frame of a complete simulation for each deck at ten audited performance orders (the five cyclic shifts of the slot order and their reverses) and checks that no settled score frame is undone later, that every settled total equals the final scores of its frames, and that every cutoff cap is at least the final score. Orders without a finite cutoff table are counted as unavailable, which is partial coverage rather than a pass.
- `slack_profile DATA ROSTER REQUEST DECKS OUTPUT` pairs each audited order's fine cap with the actual simulated score and per-note scores. The per-entry terms are attribution, not bounds; the cap itself must not be below the score.
- `idle_audit DATA ROSTER REQUEST DECKS OUTPUT [--search]` evaluates each deck with and without the deterministic idle trigger plan and requires identical outcomes, including the score distribution, best order and non-timing counters. `--search` repeats the comparison for a whole search, which needs `timeLimitMs:null`.
- `explain_fixed DATA ROSTER REQUEST DECKS OUTPUT` separates a fixed deck's candidate-specific floating margin from coefficient/window slack. Its `diagnosticScoreWithZeroMargin` is explicitly not an admissible bound and must never authorize pruning.
- `score_program DATA ROSTER REQUEST DECKS POWERS OUTPUT` records exact score programs for the declared roots, then compares evaluations at every supplied power with fresh complete simulations. `POWERS` is an array of i32 values. The audit also attempts an explicit nondecreasing-score certificate on power 0 through 2,000,000, reported separately from sampled replay equality. These diagnostics do not search, merge decks or certify PT. See [score programs](../../docs/score-programs.md).

## WebAssembly verification

Both runners read a manifest with `cases` of `name`, `data`, `snapshot`, `request`
and `reference` paths relative to itself; each reference is the complete native
answer `ournotes-recommend --data DATA --snapshot SNAPSHOT --request REQUEST`
(`ournotes-deck.snapshot-recommendation/1`) for the same inputs.

`node tools/search-harness/wasm-node.cjs MANIFEST.json WASM_PACKAGE OUTPUT_DIR`
loads a package built with `wasm-bindgen --target nodejs`. For each case it checks
the dataset identity against the SHA-256 of the deck data text, compares the WASM
answer with the reference, runs the case again with a progress callback (manifest
`progressIntervalMs`, default 250) and checks that the answer and all
telemetry fields other than times are unchanged, that every report is a `TimedOut` result with the same
context, and that node counts and the best payoff never decrease across reports.

`node tools/search-harness/browser.cjs MANIFEST.json WASM_PACKAGE OUTPUT_DIR` runs
a `--target web` package in real Chromium Workers, with Playwright installed in the
Node module path. The optional `timeoutMs` bounds each Worker externally. It checks
the SHA-256s of the original UTF-8 strings inside the Worker.

Both compare every semantic answer field (`json-tokens.cjs`). Object keys are
canonicalized while JSON number tokens remain strings, so integers above
JavaScript's safe range cannot silently compare equal. Only diagnostics/timing
fields are excluded. They record runtime, runner, manifest and WASM identities.
This verifies the declared corpus and transport, not every browser or game parity.

## Mock rosters

```sh
python3 tools/search-harness/mock_rosters.py DATA MOCK_DIRECTORY --extended
```

`DATA` is an `nnnotes.deck-data/1` file. The generator writes six general
profiles (max-items, veteran, midcore, newcomer, band-skewed, snap-poor) and
two seeds of each card-kind profile:

| profile | what it owns |
|---|---|
| conversion-rich | every converting Snap, few others |
| luck-heavy, combo-heavy | mostly members whose Gekisou skill serves the LUCK or COMBO mission, grown further than the rest |
| single-attribute | members and Snaps of one attribute |
| five-characters, six-characters | members of exactly five or six characters |
| near-ties | uniform cultivation, equal character ranks, no furniture |
| gacha-newcomer | a few fully grown top-rarity cards in an otherwise young account |
| rank-skewed | one band at the top character rank with its furniture, the others at the bottom |

`--extended` adds eighteen seeded ownership/skill profiles plus twenty-four
paired furniture controls. Each of six control families preserves its cards and
non-furniture facts across four item states.

Every draw is keyed by the profile seed and the card or fact it decides, so a
card added to the master changes no other card's draw. The manifest records
seeds, family, semantic identities, represented master domains, missing-domain
warnings and the provenance hash of every master table the generator read (null
for an absent table). Each roster repeats those hashes under `mockSource`;
`ournotes-recommend` rejects a roster whose tables differ from the deck data's,
so a master update that only adds songs keeps the rosters valid and one that
changes cards asks for regeneration. An absent MasterVip table is not inferred
from character ranks; profiles other than the six general ones explicitly use
VIP 1. These are synthetic input profiles, not claims about real account
populations.

## Dominance is an experiment with proof obligations

Propose `{kind: "member" | "snap", from: id, to: id}`. Member replacement must preserve character. For every original team, the audit substitutes the proposed replacement for the same member (or Snap) and consults the exhaustive oracle with the canonical layout of the result. It counts canonical regressions, unavailable replacements and Snap-occupancy conflicts, retaining the first counterexample with both complete scored candidates.

Even an audit with zero counterexamples authorizes no production pruning. Its scope is the configured finite domain and metric. A general rule needs a completion-safe argument covering:

- leader/slot, attributes/tags, event bonuses and conditional skills;
- effect timing/order, life, judgement conversion and Gekisou missions;
- unique Snap resources and occupied replacements;
- canonical ties, cultivation identity and restoration of distinct Top-K alternatives;
- event-grade/PT steps after each performance order, rather than applying a reward formula to an average score.

Use the exclusion experiment to expose the actual loss from an unproved removal. Preserve counterexamples as regression cases. A bound needs per-pruned-node witnesses and an independent exhaustive completion check before entering a fast path.

## Synthetic suite

### Search correctness matrix

The `correctness_matrix` integration tests construct synthetic member, Snap,
chart, and payoff tables in memory. Run the complete matrix with:

```sh
cargo test --release --locked --features search-diagnostics \
  --test adapter_fixture_export correctness_matrix -- --nocapture
```

The ordinary CI integration-test command includes this matrix. Each case covers
K = 1, 3, 5, and 100 with cache capacities of 0 and 64. K = 100 also checks the
returned cardinality when the physical domain contains fewer candidates.

| Family | Cases | Physical candidates | Coverage |
|---|---:|---:|---|
| Scene and objective | 36 | 216 | Free, COMBO Mission, Battle, Arena; theoretical play and a complete mixed-judgement stream; expected score, score-target probability, capped score, client event PT, and score with terminal life |
| Growth and length | 8 | 48 | Low member growth with strong skills; band-rank skew and delayed range ranks 1, 3, 5; 12-note and 256-note charts; score and client event PT; deterministic repeated nodes and completed simulations |
| Constraints and ties | 8 | 152 | Same-character alternatives, every eligible leader, member requirements and exclusions, Snap exclusions, optional unique Snap bindings, empty domains, and canonical ties |
| Nominal LUCK | 11 | 55 | Expected score, score-target probability, capped score, client event PT, terminal-life probability, equivalent leader programs, and certified ranking |

For deterministic cases, an independent enumerator forms five-character member
combinations, chooses each eligible leader, and enumerates every injective
optional Snap assignment. The fixed evaluator supplies each team's 120 order
outcomes. Separate aggregation and rational comparison code establishes the full
canonical ranking, which both exhaustive and branch-and-bound search reproduce.
The matrix checks 49,920 oracle order outcomes. With `search-diagnostics`, each
candidate also supplies witnesses for five joint prefixes, eleven composition
and team prefixes, and all 120 per-order caps: 6,656 prefix checks and 49,920
order-cap checks in total.

The scene fixtures include recovery and judgement conversion, COMBO ranges,
timed score factors ending at a range snapshot, and network confirmations with
distinct arrival frames and bonus percentages. Mixed streams include Miss,
Bad, Good, Great, Perfect, and Just judgements. Score and life objectives consume the
complete stream's terminal life.

The LUCK cases use the declared independent nominal lottery tables. Their
leader variants have equal power and the same performer multiset, so averaging
all 120 orders gives the same score law. The independently enumerated canonical
keys therefore determine the tie order. Every returned rank is certified;
constant-payoff cases also check exact probability or capped-score values.

The matrix establishes search enumeration, ranking, and bound contracts under
the shared fixed-deck model. Its model inputs and charts are synthetic.

### Score-path corpus

```sh
export OURNOTES_SCORE_PATH_OUT=work/score-paths
cargo test --release --locked --test adapter_fixture_export export_score_path_matrix -- --ignored
python3 tools/search-harness/run.py run \
  --binary tools/search-harness/target/release/ournotes-search-harness \
  --suite work/score-paths/suite.json --out work/score-path-results
```

The exporter crosses low member growth with strong skills or band-rank-skewed
growth, 12-note or 256-note charts, and LUCK or lottery-free play.
The LUCK range stays at 150–400 ms for both chart lengths. Subsequent ranges use
Just and COMBO missions. All inputs include their complete optional Snap domain.
The low-growth family uses Solo Mission. The skewed family uses Battle with
delayed range confirmations at ranks 1, 3, and 5 and a declared room-score policy.

`suite.json` contains eight lottery-free score/PT cases, each with 60 physical
teams and K = 5. The existing independent oracle, cap audits, and `run.py compare`
apply to these cases. `matrix.json` also lists bounded nominal-LUCK cases and
eight larger PT requests with 13,600 physical teams, K = 5, and a 60-second
request budget. Those entries include owned snapshots and the exact dataset
identity for the existing `profile_case` and `benchmark_case` binaries. They are
ordinary search requests; their reported completion determines whether their
returned ranking is certified.

The harness's deterministic oracle requires exact payoff fractions. General
nominal-LUCK interval results use the nominal LUCK integration matrix and the
separate exact-law tests. A deterministic oracle report does not certify the
nominal-LUCK cases or the larger domain.

### Exported corpus

The generator reuses existing synthetic effect tables: six members across five characters, two different Snap programs, a short synthetic chart, normal Free and lottery-free Gekisou Mission scenes. Both scenes test final score and client event PT. Each case compares exhaustive search, bounded candidate proposals, and an intentionally unproved Snap removal, plus member/Snap substitution audits. Each of the four cases enumerates 310 teams: two legal five-member sets, five leaders each, and all 31 optional unique assignments of two Snaps, each team over its 120 performance orders. `BDON_HARNESS_STRESS=1` exports a larger 13,600-team variant. `check-errors.py BINARY CORPUS OUTPUT` checks that an oracle cap below the domain size is refused before scoring and leaves no report.

The suite exercises represented Snap and Gekisou behavior; it does not claim every skill family, realistic whole-chart behavior, latest patch parity, human input prediction or server settlement. Real-data cases use a deck data file written by `nnnotes deck-data`, rosters from `mock_rosters.py` and explicit requests, with their own input identities.

## Independent PT plateau certificate

`pt_power_certificate` exports model bonus values and complete per-slot power
matrices; `pt_certificate.py` independently resolves the PT ceiling and runs a
Top-K DP over the five-slot subset mask while scanning each Snap exactly once.
It enumerates every allowed physical member layout and preserves None-first ties.
This path uses neither production search pruning nor its Hungarian assignment.

The certificate is deliberately conditional: the global PT ceiling must already
be attained by K incumbents, exactly five members must remain eligible under the
independent member caps, and the power-leading assignments must attain that PT
ceiling when replayed by the fixed evaluator. Slot power must be nonnegative,
integral, additive with the checked nonwrapping sum. Other inputs return an
inapplicable/error result; they do not receive a false proof.

```sh
cargo build --release --locked --manifest-path tools/search-harness/Cargo.toml
pt_power_certificate export DATA ROSTER REQUEST INCUMBENT matrix.json
python3 tools/search-harness/pt_certificate.py matrix.json proposals.json
pt_power_certificate verify DATA ROSTER REQUEST INCUMBENT proposals.json verified.json
```

Keep all input, exporter/verifier source and binary hashes with the certificate.
The shared model boundary remains explicit: this certifies search/pairing and
canonical ranking for the frozen input, not additional native-game correctness.
