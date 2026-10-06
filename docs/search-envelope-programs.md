# Compiled search envelopes

The solver preserves the complete legal physical domain. Prepared chart and skill
metadata provide upper bounds; the fixed evaluator remains the authority for
candidate scores and per-outcome PT. These bounds do not establish equivalence
between native skill programs or monotonicity of their actual scores in power.

## Resources before all members are known

For a positive scale `r`, each member/Snap pair contributes an outward-rounded
integer upper bound on `power + r * gain`. The table is indexed by leader profile,
skill position, scale and Snap (including None). Each cell retains the five
largest values from distinct characters. At an unfinished five-member prefix,
at most four characters are occupied, so this table retains the exact largest
unoccupied-character value for that cell, even with duplicate member variants.

The remaining slots are matched to unused Snaps; assigned slots use dummy columns.
Row shifts restore the None contribution exactly. Remaining character uniqueness
and required-member restrictions are relaxed, while already occupied characters
and Snaps remain excluded. The resulting sum bounds `P + r*A`, hence
`P*A <= (P+r*A)^2/(4*r)`. Gains are position means (see
[uniform member-order search](search.md#uniform-member-order-search)); the cap times 120 bounds the payoff
numerator.

Storage and numerical limits disable this optional table, never truncate the
candidate domain. A bounded prefix probe controls whether production pays its
runtime cost; the independent oracle still audits the cap when that probe skips
it. Prune counts and bound-check counts are reported separately.

The dual table indexes leader profile, position, scale and character.
Its five best distinct Snap choices retain the best available edge after at
most four prefix Snaps are excluded; None is always reusable. Match remaining
positions to distinct unoccupied characters, relaxing future Snap uniqueness
and required-member constraints. This retains position capacity that the
per-character maximum scan relaxes. Both assignments are upper bounds and can
be intersected. The resource-stage probe measures its marginal tightening beyond
the correlated stage when that stage is enabled.

For `k <= 4` remaining positions, the character assignment only needs each row's
best k characters. At most k-1 of those characters can be occupied by other rows,
so any omitted edge can be replaced without decreasing the value. Enumerating
the retained combinations, at most 256, gives exactly the same bound value.
No physical binding or canonical tie decision is taken from this scalar solver.

## Stable cumulative commands and absolute drift

A count-based command bound requires the requested cumulative factor to survive
the native binary32-to-mill-to-binary32 round trip. Otherwise an unchanged count
can still cause a replacement on every execution frame. For example, the factor
from value 45 quantizes to 449 mill and reconstructs below its requested value;
the native approximate-equality test fails. Preparation verifies every
reachable discrete factor, including the final clamped value, within bounded
work. Unstable or unverified cases retain the frame-count command bound.

Factor-state error is additive in the note factor plus the selected judgement
factor. The total command budget covers rounding across both fields. For the
joint-prefix relaxation, let `D` conservatively include the existing absolute
drift allowance, and let `B` bound the sum of note sensitivities using the maximum
of **all** judgement percentages. Using the no-skill coefficient alone would be
unsafe because it excludes budgeted conversions.

The transformed coefficient is `A + D*B`, followed by the separate multiplicative
chain allowance. This changes the prepared base and global coefficient; per-pair
gains and the existing assignment machinery remain valid. Outward arithmetic,
a rounding-amplification check, a positive-factor check and finite normal-range
guards restrict applicability. Other cases retain the enclosing envelope.
The fine/raw candidate caps and scorer overflow checks apply independently.

The harness separately reports candidate-specific margin and a zero-margin
diagnostic. The latter is never an admissible cap. Candidate margin, the
remaining coefficient/window slack and the pool-wide prefix margin are different
quantities and must not be conflated.

## Cumulative score ramps

Without a ramp, effect 2001 uses its maximum cumulative factor throughout every
possible execution window. For one-shot Gekisou rows whose cumulative condition
counts judgements (types 1000–1002), a tighter bound follows the processing clock.

For each possible start frame, count every judgement that could match any target
under the whole-pool conversion closure. Duplicate targets still contribute at
most one increment per judgement, matching the native checker's `any` operation.
Ignore conditions and gate failures after start. This only increases the count.
Apply the actual binary32 division/floor, cumulative maximum and effect-value
maximum. The accepted integer domain excludes counter/product wrapping.

The first factor can be backdated to its trigger time. Its bound therefore uses
the entire start frame's possible count from the earliest allowed trigger time.
Subsequent factor replacements are filed at the current processing-frame time.
Each potential execution gets a fresh count baseline: one-shot updaters reset
their cumulative counter before returning to their available stack. Multiple
possible starts remain separate optimistic executions.

This refinement applies only to the finite-start timing representation, currently
at most eight possible starts. Own-event rows, sustained rows, other cumulative
conditions and unsupported numerical cases retain their existing conservative
envelopes. The command-count, factor-peak and undo/replay drift bounds also
apply. A counter ramp encloses the count of every admitted execution.

## Constant-cost bounds when judgements cannot change

For a particular class and position, the existing conservative conversion
bitmap can prove that no chart entry's judgement can change. This includes
conversion programs whose targets never occur inside their possible windows;
the mere presence of a conversion skill is not sufficient to reject the shortcut.
Budgeted conversions participate in this applicability check too.

In that domain, raw judgement weights replace the pool-wide judgement maxima.
Compile interval sums for base/note factors and the four judgement-specific
factors. Each prefix sum stores both a lower and an upper endpoint, so an interval
uses `upper[end] - lower[start]`; subtracting two rounded-up prefixes would not
be a safe upper bound. Card/class/position gains are then constant-size lookups.

The floating-state margin is also compiled into per-slot statistics: lifetime
factor commands, their execution count including replays, the maximum active
factor sum, and the maximum sum intersecting one native score frame. A factor
span `[a,b]` projects to the closed interval `[frame(a),frame(b)]` using the
scorer's binary32 time calculation and final-frame clamping. The frame limit
uses a nonzero `score_music_length_ms` when supplied, otherwise `music_length_ms`.
Closed endpoints retain transient start/end pairs and clamped final-frame terms.
The compiled sweep uses the equivalent half-open integer interval
`[frame(a),frame(b)+1)` and outward accumulation. Its maximum bounds every
projected frame's factor sum. Since a maximum of a sum is no greater than the
sum of the individual maxima, five packets bound the candidate-specific drift
calculation in constant work. Lifetime commands contribute two representation
roundings each, for the integer-to-binary32 conversion and division.
The cap intersects this margin with the prepared pool-wide margin, using outward
arithmetic while retaining the scorer's binary32 calculations.

Sources: [`ScoreFrames`](../crates/ournotes-search/src/search/snaps/score_windows.rs),
[`FineView::cand_counts`](../crates/ournotes-search/src/search/snaps/fine_view.rs),
[`RawEnvelope`](../crates/ournotes-search/src/search/snaps/raw.rs).

If any selected program can change a judgement, search proceeds to the existing
per-note envelope. Every surviving candidate still receives full evaluation.
For PT, the score cap of each performance order passes through the maximum reward among reachable tiers before
the orders are summed; no step reward is applied to an average score.

## Validation

The harness checks resource-prefix, character-prefix and compiled leaf caps
against independently enumerated teams, in addition to the class, binding,
composition and per-note audits; see [validation](search.md#validation).
Only `Complete` certifies canonical Top-K under the declared model and the uniform
member-order target. Agreement of the model with the game is a separate question.

[Exact score programs](score-programs.md) record one execution as a
power-parameterized expression with optional checked monotonicity certificates.
Their execution-context identity governs reuse within physical search.

## Conversion partition preparation

A Gekisou conversion partition retains its member domain and permitted Snap choices together with its slot rules.
Each rule set owns one traversal. These traversals are disjoint and their union is the requested team domain.

The envelope for a partition is prepared immediately before its first traversal. Traversals of the same partition
share that envelope; its final traversal releases it. The incumbent-first order and the static order enumerate the
same rule sets and use the same canonical Top-K comparison. Preparation changes the lifetime of compiled tables,
not the set of candidate teams or the meaning of a bound.

A stop before preparation or before a traversal leaves the remaining partitions covered by the pool-wide root
bound. A stop inside a traversal combines that traversal's open-prefix bounds with the pool-wide bound for later
partitions. The maximum also includes retained incumbents when the final whole-domain bound is reported.
If a partition cannot compile, a traversal using the pool-wide envelope covers the outstanding domain.

The synthetic regression `conversion_envelopes_are_prepared_between_traversals_and_stops_cover_later_parts`
checks preparation order, canonical results against exhaustive search, and whole-domain bounds at deterministic
candidate-budget stops. Run it with:

```sh
cargo test --release --test adapter_fixture_export conversion_envelopes
```
