# Compiled search envelopes

The solver preserves the complete legal physical domain. Prepared chart and skill
metadata provide upper bounds; the fixed evaluator remains the authority for
candidate scores and per-outcome PT. These bounds do not establish equivalence
between native skill programs or monotonicity of their actual scores in power.

## Resources before all members are known

For a positive scale `r`, each member/Snap pair contributes an outward-rounded
integer upper bound on `power + r * gain`. The table is indexed by leader profile,
native skill position, scale and Snap (including None). Each cell retains the five
largest values from distinct characters. At an unfinished five-member prefix,
at most four characters are occupied, so this table retains the exact largest
unoccupied-character value for that cell, even with duplicate member variants.

The remaining slots are matched to unused Snaps; assigned slots use dummy columns.
Row shifts restore the None contribution exactly. Remaining character uniqueness
and required-member restrictions are relaxed, while already occupied characters
and Snaps remain excluded. The resulting sum bounds `P + r*A`, hence
`P*A <= (P+r*A)^2/(4*r)`. Native-root positions remain fixed. Per-root integer caps
are combined using checked integer masses.

Storage and numerical limits disable this optional table, never truncate the
candidate domain. A bounded prefix probe controls whether production pays its
runtime cost; the independent oracle still audits the cap when that probe skips
it. Prune counts and bound-check counts are reported separately.

The dual table indexes leader profile, native position, scale and character.
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
guards restrict applicability. Other cases retain the original envelope. The
fine/raw candidate caps and original scorer overflow guard remain unchanged.

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
envelopes. The original command-count, factor-peak and undo/replay drift bounds
remain in force. A counter ramp is not an exact simulation trace.

## Constant-cost bounds when judgements cannot change

For a particular class and native position, the existing conservative conversion
bitmap can prove that no chart entry's judgement can change. This includes
conversion programs whose targets never occur inside their possible windows;
the mere presence of a conversion skill is not sufficient to reject the shortcut.
Budgeted conversions participate in this applicability check too.

In that domain, raw judgement weights replace the pool-wide judgement maxima.
Compile interval sums for base/note factors and the four judgement-specific
factors. Each prefix sum stores both a lower and an upper endpoint, so an interval
uses `upper[end] - lower[start]`; subtracting two rounded-up prefixes would not
be a safe upper bound. Card/class/position gains are then constant-size lookups.

The floating-state margin is also compiled into per-slot statistics: factor
command executions, the maximum active factor sum, the maximum sum intersecting
a 40 ms neighborhood, and the factor-window count. Since a maximum of a sum is
no greater than the sum of the individual maxima, five packets bound the existing
candidate-specific drift calculation in constant work. Neighborhood intervals
are closed; the implementation handles their integer-time boundaries explicitly.
The cap intersects this margin with the original pool-wide margin and rounds
arithmetic outward. It does not substitute ordinary integer arithmetic for the
scorer's binary32 calculations.

If any selected program can change a judgement, search proceeds to the existing
per-note envelope. Every surviving candidate still receives full evaluation.
For PT, score caps pass through the maximum reward among reachable tiers for
each atom before masses are summed; no reward is applied to an average score.

## LUCK Rush timing refinement

The optional LUCK replay produces possible skill-phase values of the native
`LuckRushPlaying` checker for each mission gate and each declared frame. Its
branch union is an upper possibility mask, not a predicted play. If a branch is
unsupported, a replay limit is reached, or any mask has the wrong shape, the
existing wide score windows remain available.

The search admits this mask only when every reachable converted judgement inside
a LUCK range has the same controller behavior as the raw judgement. The classes
are `{0,7}`, `{-1,1}`, `{2}`, `{3}`, `{4}`, and `{5,6}`. Wait/Pass skip the
controller's judgement path; Miss can consume a pending lottery despite having
zero base points. Those cases cannot share a class. The current reach bitmap
does not represent negative raw judgements, so those inputs retain the wide
envelope. This check uses each search partition's complete conversion reach,
including limited conversions.

A refinable score row has effect 2000 or 2004 and exactly one trigger condition:
one positive 7021 in one condition set. The condition updater checks this trigger
on every open-gate frame before testing its pool, execution limit or activation
phase. It therefore observes the same sticky state as the replay's gate probe.
A 7021 inside `And`/`Or` can be skipped and keep a stale flag; its negation also
cannot be bounded by a union of true outcomes. Compound, negative, missing and
otherwise unsupported trigger groups keep their original windows.

A direct 7021 supplies no trigger-time override. Each possible execution starts
at its processing frame time, without the generic range/count backdating. For
untimed sustained effects, the first later open-gate frame whose union mask is
false must terminate any execution that is still active. Conditions and release
checks can end activity earlier. The union of those intervals is emitted as
disjoint windows with multiplicity one; a closed gate does not force an end.

For one-shot effects, every possible start gets the existing duration and
release upper bound. Turning Rush off does not prematurely terminate a timed
one-shot execution. Up to eight possible starts retain separate windows; larger
sets use the original five-updater pool envelope. Sustained effects with a
modeled duration and cumulative score rows retain their original treatment.

A pooled window's concurrency limit is not its lifetime execution count: updater
slots can be reused. Compiled score rows therefore charge start/end commands for every
possible activation, and cumulative replacement commands are bounded by both
per-execution changes and concurrent updaters per processing frame. Command
representation error charges two roundings per lifetime command: the integer
mill value is cast to binary32 and then divided by100000. Both are needed at
saturation and integer precision boundaries. Factor amplitude
windows and conversion budgets retain their separate existing relaxations.

For direct untimed Rush rows with a fixed formation condition, no release and a
2000/2004 applier, each actual true run can start at most one execution. Replay
records the maximum run count over completed branches independently of its flag
union; the number of components in the union would be unsafe. Root unions take
the maximum of those counts. Candidate margins can use this smaller count while
prefix margins retain the generic activation bound.

Native drift checks its rounding-feedback coefficient and uses an outward
`1/(1-alpha)` amplification, or a one-percent reserve when that is larger. If no finite certificate exists, the optional cap is unavailable;
the physical domain is retained. Drift and score-chain allowances are composed
multiplicatively rather than dropping their cross term.

The refined difference arrays separately account for binary64 accumulation:
with `W` emitted windows and outward endpoint absolute sum `S`,
`gamma_(4W+1)*S` bounds endpoint construction, nonzero prefix additions and one
input multiplication. This absolute error is safely converted to a relative
allowance because each selected score-up factor is at least one after the
envelope's nonnegative clamps. Nonfinite arithmetic disables the fine cap. No
unspecified reserve in the native binary32 margin pays for arbitrarily many new
window operations.

Fine-bound diagnostics and slack traces use the same optional mask path.
Cached intervals retain the mask/spec allocations
used for identity and clear at 1024 entries, so allocation-address reuse cannot
return stale windows and storage remains bounded.

Validation of this refinement checks native checker-mask inclusion
(`rush_audit`), per-note caps against actual scores and exhaustive ordered Top-K
comparisons under the same finite root law, including LUCK-changing member and
Snap skills.

## Validation

The harness checks resource-prefix, character-prefix, Rush and compiled leaf caps
against independently enumerated physical completions, in addition to the class,
binding, composition and per-note audits; see [validation](search.md#validation).
Only `Complete` certifies canonical Top-K under the declared model and finite root
law. Agreement of the model with the game is a separate question.

[Exact score programs](score-programs.md) record one execution as a
power-parameterized expression with optional checked monotonicity certificates.
They do not replace physical search or establish equality between different deck
programs.
