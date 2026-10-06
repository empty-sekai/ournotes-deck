# Exact deck search

The played Live/PT recommendation facade searches teams (a leader, four other members and the Snap paired with
each member) under the uniform member-order target: the five members perform in a uniformly random order, and a
team's value is its mean payoff over the 120 performance orders. Its result order and proof contract are specified
in [uniform member-order search](#uniform-member-order-search); the bounds it uses are described after it, and
[validation](#validation) lists the reproducible correctness experiments.
The opening sections describe the canonical member-set solvers and the power/score components that the team
solver reuses. Each route declares its result identity explicitly. The recommendation facade first tries the
applicable [team power and deck-payoff routes](#canonical-team-power-and-deck-payoff-routes). Played-live
continuations use the [composition/Snap decomposition](#member-compositions-snap-pairings-and-power-frontiers)
with Gekisou off, and joint member/Snap traversal with Gekisou on. Compiled envelopes are derived in
[compiled search envelopes](search-envelope-programs.md).

## Proof scope and route contracts

An exact search claim fixes four things: the legal candidate domain, the value of a candidate, the identity of a
result, and the complete tie order. The following contracts use different identities and targets.

| Entry point or route | Value being ranked | Result identity | Empty Snap tie |
|---|---|---|---|
| `search::search`, Power and Skip score | Power, or the deterministic Skip score with power as its next key | One best representative per set of five member cards | After every Snap ID |
| `search::search_best_order_diagnostic` | Best performance order for the declared diagnostic objective | One best representative per member set | After every Snap ID |
| Recommendation facade, Power and deterministic Skip metrics | Declared deterministic payoff, then power | Leader and five member/Snap pairs; nonleader pairs in canonical layout | Before every Snap ID |
| Recommendation facade, deterministic played Live | Mean terminal payoff over all 120 orders, then power | The same canonical team | Before every Snap ID |
| Recommendation facade, LUCK | Expected terminal payoff under the declared order and lottery law | Canonical team, with certified interval ranking | Before every Snap ID |
| Deterministic `SearchSession` v1 | Its supported deterministic payoff, then power | Five physical member slots and their Snap bindings | Before every Snap ID |

`search::search` accepts Power and Skip; best-order Live optimization is exposed through the explicitly named
diagnostic entry point. The recommendation facade selects its route in
[`handler.rs`](../crates/ournotes-search/src/handler.rs) and
[`physical.rs`](../crates/ournotes-search/src/search/physical.rs).
The session's physical-slot contract is specified in [search sessions](search-session.md).
A fixed-deck evaluation proves the value of its requested deck under its declared inputs; its domain contains that
deck alone.

Here, “legal” means admitted by the resolved pool, cultivation, leader/include/exclude constraints and declared
execution context. The search theorem is relative to the supplied scorer and probability law. Account prerequisite
coverage is specified in [account input](account-input.md), and scorer compatibility evidence in
[native validation](native-validation.md). A search proof, an exact numerical expectation, a certified rank and a
model-compatibility claim are separate statements.

The real-arithmetic inequalities below require the indicated numerical certificates in their implementations.
Optional bounds return no cap when their certificate is unavailable; the caller retains the complete continuation
or uses an exhaustive route. An input, model or arithmetic error remains an error. In particular, an unavailable
upper bound has the meaning “unknown,” rather than a numerical value of zero.

## Problem

A deck has five member cards in slots 0..4 and up to five snaps, one per slot. The member in slot 2 is the leader.
The members belong to five different characters; a snap appears at most once. Snaps are optional.

Deck power is computed per stat, per slot, and summed. Let `Q(B, p)` denote the sum of the three stat-point
contributions produced by `B.mul(p).to_floor()`. For one stat, with base `b` and percentage `p` both in the
fixed-point unit, its contribution in whole points is

    q(b, p) = sat_i32(floor(RN32(RN32(trunc0(b * p / 10000)) / 10000)))

`RN32` is the binary32 rounding used by the integer cast and floating division; `trunc0` is integer division
toward zero. `sat_i32` is the finite-value saturating conversion of the native numeric model. The admitted table
products are nonwrapping and the floating values are finite; infinity has a separate native conversion rule.
This is the operator
in [power.rs](../crates/ournotes-sim/src/power.rs) and
[`tables::term`](../crates/ournotes-search/src/search/tables.rs).
It is monotone in `p` for nonnegative `b` on that domain, but it can differ from flooring the real-number
percentage directly: 99,999,999 BP rounds to 100,000,000 BP through `to_floor`.

For a slot holding member `m` and Snap `s` under leader skill profile `L`
(the leader card's leader skill at its level), the slot total in points is

    S(m, s, L) = A(m) + LEAD_L(m) + W(m, s)

- `A(m)`: every term that depends on the member alone (its own power, character rank and total rank bonuses, band
  items, song type and tag bonuses, memory, VIP, and the member's event bonus when event parameters are used);
- `LEAD_L(m)`: `Q(B_m, leaderPercent_L(m))`, where `B_m` is the resolved member power plus its member-event
  power increment and flat character-rank, total-rank and slot-memory terms. The member-event increment is applied
  before this base multiplies leader, Snap and other percentages.
  `leaderPercent_L(m)` sums the leader skill effects targeting `m`;
- `W(m, s)`: `Q(B_m, snapPercent_s + snapEvent_s) + Q(B_m, typeLink(m, s))`, with `W(m, none) = 0`.

The additive identity needs both integrality and range checks. `Tables::new` checks that resolved base and
member-only totals are multiples of 10,000 BP on every stat. Percentage terms return multiples of that unit by
construction. Hence each stat sum divided by 10,000 is an integer, and the regular deck total equals the sum of
the slot-point totals as long as the sum and its conversion are nonwrapping. Table construction checks base/stat
values below `2^40` BP and table percentages below `2^20`; each product is therefore below `2^60`.
The consuming route additionally certifies the whole-deck power range needed by its score or payoff reduction.
The checked decomposition is relative to the resolved values produced by the regular power calculator.

The final stat conversions need their own range argument. Let `x_j` be a completed deck's integer point sum for
stat `j` before that conversion, and `b_j >= max(x_j,0)` its sum of nonnegative component upper contributions.
The prepared leader upper bound and the whole-domain lower proof establish

    sum_j b_j <= Pmax <= i32::MAX,       sum_j x_j >= 0.

For each stat, `x_j <= b_j <= Pmax`, while

    x_j >= -sum_{k != j} x_k >= -sum_{k != j} b_k >= -Pmax.

Every final stat is therefore representable, its whole-point BP value divides exactly by 10,000 in binary64,
and the sum lies in `[0,Pmax]`. This proves the final conversion step of the additive identity, including for
configurations pruned before evaluation. The member-set route checks the interval for all its targets; team
bound preparation supplies its corresponding certificate or leaves the facade's exhaustive route available.

Leader percentages additionally have a nonwrapping accumulation certificate. For the entire profile,
`LeaderProfile::absolute_sum_bound` checks `C = sum_e |value_e| * c_e <= i64::MAX`, with `c_e = 5` for a cumulative
row and `c_e = 1` otherwise, without applying targets or conditions. Every component, conditional subset and
addition prefix has absolute magnitude at most `C`. Thus both the exact percentage and its positive-effect
relaxation are ordinary integer sums before the percentage-range check and the application of `Q`.
Checking only an already-wrapped final percentage would not justify that comparison.

Permuting the four non-leader pairs preserves power; changing the leader or a member/Snap pairing can change it.

A leader profile is *simple* when none of its effects has a condition group or a cumulative count: then
`leaderPercent_L(m)` depends on `m` alone. Otherwise (conditions on the other members or on the song, counts over the
deck) it depends on the whole member set; the search evaluates it exactly for every complete member set and bounds
it before.

## Result identity and order

This section, through the core algorithm proof, specifies the direct member-set search contract in the first two
rows of the route table.

One result per set of five member cards. Its representative is the best deck with those members under this order,
better first:

1. objective, descending;
2. deck power, descending (for score objectives);
3. the five member card ids, ascending, compared as a sorted list;
4. the leader card id, ascending;
5. the snap ids in slot order, ascending, with "no snap" after every snap;
6. the performance order, ascending.

The non-leader members sit in slots 0, 1, 3, 4 in ascending card-id order. The function `identity` in
`crates/ournotes-search/src/search/topk.rs` is the only place that defines what makes two results the same.

## Outcomes

- `Complete`: the results are exactly the first `min(K, number of legal member-set identities)` entries of the
  order above, each represented by its best legal deck.
- `TimedOut`: every result is a legal deck whose values are computed exactly, but some better deck may be missing.
  Preparation, traversal and final verification share one cooperative deadline. Only completely verified
  candidates are returned. An atomic verification can finish after expiry, but another candidate is not started.
- No legal deck (fewer than five characters, or constraints that exclude every deck): `Complete` with no results.
- `Error::Domain`, `Error::Input`, `Error::Game`, `Error::Capacity`, `Error::Unsupported`: no results.

Every returned deck on these member-set routes is evaluated again with the regular deck-power path (and, for the live objective with snap
skills, simulated again); a difference from the search's value is an error, not a result.

`K = 0` is an input error, including with a zero budget or infeasible pool. A zero budget returns `TimedOut`
without preparing tables or visiting decks. K is a result limit, so requesting more results than exist does not
allocate K entries up front. Cooperative deadlines do not provide preemption, resumability or a browser Worker
step API; these remain separate requirements.

The recommendation facade can report progress (`engine::recommend_with_progress`, `Progress`). A report is a
`TimedOut` snapshot with completely evaluated deterministic candidates, or the current certified LUCK intervals,
and the telemetry so far. The report's telemetry is closed on a copy, so its proof is not complete and
has no upper bound of the unexplored part, which only a stop computes. Reports are made at the deadline checks and
after Top-K insertions, at most once per interval. Building a report only reads the search state, so a complete
search visits the same nodes and returns the same result with or without reports.

Every member-set target, including pure Power, requires a proof over the entire feasible power domain. The solver
combines target-aware lower bounds for signed leader effects with required members and distinct-character
constraints, and takes the largest prepared leader bound as its power upper bound. It rejects a domain unless
power is nonnegative and within `i32`. Skip additionally requires every intermediate binary32 operation at that
upper bound to be finite and nonnegative, with a nonwrapping note sum. Live applies its score-chain certificate
below. These conditions are conservative: a rejected domain need not contain an actual
overflow. The unrestricted evaluator retains the native conversion and wrapping behavior, including infinity
converting to MIN; it never substitutes a fabricated saturating score. This search proof and synthetic oracle
coverage do not certify the latest JP model, account resolver, or timing-policy search.

## Algorithm

For each allowed leader card: its profile `L`, the per-member value `u_L(m) = A(m) + LEAD_L(m) + Wmax(m)` with
`Wmax(m) = max(0, max over allowed snaps of W(m, s))`, and for every other character the best `u_L` among its cards.
Characters are sorted by that best value, descending. A depth-first search picks the remaining members character by
character. At a node that still needs `r` members and may use the characters from position `j` on, the bound is

    current sum + best(j) + best(j + 1) + ... + best(j + r - 1)

(the `r` best remaining characters, a prefix sum). A complete member set gets its exact leader terms and its best snap
assignment (a maximum-weight assignment of snaps to the five slots, solved exactly; ties go to the lexicographically
smallest snap ids in slot order).

Leaders are visited in descending order of their own bound; a Top-K holding at most one entry per member set keeps
the K best representatives.

## Why no result is lost

Let `T` be the K-th value currently held (minus infinity while fewer than K results are held). `T` never decreases.
A branch is discarded only when its bound is **strictly** below `T`; equal values are always explored, because an
equal-valued deck can still win on the later keys of the order.

1. *The bound is at least the value of every deck below the node.* For a member `m` in any deck: `A(m)` is exact;
   `W(m, s) <= Wmax(m)` for every snap and for an empty slot, since `Wmax` is a maximum over all allowed snaps and 0
   (relaxing the rule that snaps are distinct only enlarges the maximum); `LEAD_L(m)` is exact for simple profiles. For
   other profiles the search uses `Q(B_m, P)` with `P` the component-wise sum over the effects that could target
   `m` of `max(0, value x c)`, where `c` is the largest count the effect can take (at most 5, or its cap), counting every
   conditional effect as active. The profile accumulation certificate above covers both sums. The exact percentage
   is a sum over a subset of those effects with counts at most `c`,
   so it is at most `P` component-wise. The multiplication, truncating division, binary32 operations and final floor
   in `Q` are non-decreasing in `p` for `B >= 0` on the validated domain, so the exact
   term is at most the bounded one. The prefix sum over the best remaining characters is at least the sum over any
   `r` distinct remaining characters, since the characters are sorted and each character contributes at most its best
   card.
2. *So a deck that belongs in the final result is never discarded.* If a deck `D` has value `v >= T_final`, every node
   on its path has bound `>= v >= T_final >= T` at the time of the test, so the strict test keeps it. When its
   member set is evaluated, the exact snap assignment gives at least `D`'s value, and among equal values the
   canonically smaller assignment.
3. *The Top-K keeps the right representative.* A member set's entry is replaced only by a better representative of
   the same set; an entry leaves only when K better entries of other sets are held, so it cannot be in the final
   result.

The same holds per leader: a leader is skipped only when its own bound (the leader's `u` plus the best four other
characters) is below `T`.

### Enumeration and assignment lemmas

**Member coverage.** Fix an allowed leader. Required members fix their characters; a conflicting required member
or too few remaining characters makes that leader infeasible. Every remaining legal member set has one increasing
sequence of character indexes, and one card choice for each such character. DFS takes exactly that path. Sorting
characters by their maximum and sorting cards within each character changes only visitation order.

The sorted maxima also justify loop termination. Moving the starting character index to the right cannot
increase the sum of the next `r` maxima. Within a character, the remaining card values cannot exceed its current
value. A bound rejection can end one of these loops only when the bound covers its entire remaining suffix;
rejection of an individual card proves only that card's subtree.

**Canonical assignment.** Let `s` be the number of allowed Snaps, `b = s + 1`, and `B = b^5`. Rank the Snaps
by ascending public ID, with None at rank `s`. The five-slot penalty

    penalty = rank_0*b^4 + rank_1*b^3 + rank_2*b^2 + rank_3*b + rank_4

lies in `[0, B-1]`. Thus maximizing `total_weight*B - penalty` first maximizes integer power, then minimizes the
whole Snap key. Five dummy None columns allow every slot to be empty independently. Dummy-column permutations
have the same result identity. The constrained matcher assigns a forbidden pair a cost exceeding the largest
possible cost advantage of any legal assignment, and verifies legality on its output.

These are the objectives implemented by [matching.rs](../crates/ournotes-search/src/search/matching.rs).
The pool constructor limits each card pool to 65,535 entries, hence `B <= 2^80`. The admitted nonnegative
power-table edge is below `2^37` points, so its encoded cost and the forbidden cost
`16*(max_abs_weight+2)*B` fit well inside the matcher's `i128` range. Optional weighted-resource callers have
their own tighter edge/cardinality gates.

**Top-K witnesses.** After each offer, the table holds the best K representatives among all identities handled
so far. Replacing an identity by a better representative preserves this invariant. Removing another identity
requires K distinct better identities. Those K witnesses remain available or improve later, so the K-th primary
cutoff never decreases. A discarded branch lies below K witnesses at the moment it is discarded; it remains
excluded at every later cutoff. This proves preservation without presupposing that the search's final cutoff
already equals the mathematical optimum.

## Skip score

The skip score is a function of the deck power alone: every note is scored as Great with a fixed combo and life,
through a chain of binary32 multiplications and divisions by the note's constants, a floor, and a sum. Each operation
is non-decreasing in its input when the constants are non-negative, so the skip score is non-decreasing in the power
(the search checks the constants and that the note sum stays within 32 bits). With the order above (skip score
descending, then power descending, then the identity keys), sorting by skip score gives the same order as sorting by
power. The skip search therefore runs the power search with the chart's song and reports each result's skip score.

## Live score without snap skills

The live objective with `exclude_snap_skills` scores a stated play (every judged note with its judgement, life and
combo, and the life in the frame where each skill event fires) with live skills only: Gekisou off and snap skills
excluded. The member at performance position k fires the chart's skill event k; its live skill's effects add factor
commands for a fixed duration. The default play (`Play::theoretical_best`) judges every judged note Perfect, which is
a full combo: the combo a note reads is the number of judged notes at earlier chart times and the life stays at its
base value (a Perfect costs no life in the data; a table where it does is reported as unsupported).

The representative reduction also requires a numeric certificate over the whole feasible power interval.
The diagnostic search checks nonnegative power within `i32`, positive finite score adjustment and difficulty,
a positive converted-note count, and finite nonnegative per-note multipliers. Generated score-factor commands
must be paired nonnegative start effects with matching negated finishes in time order. Note, event and command counts
are limited to `2^20`, and a bound on accumulated factor error must stay below `1/2`.

Let `u = 2^-24`, `n` be the maximum command count and `M` the maximum sum of absolute command factors.
Charge two representation roundings and one state addition per command, plus the final note/judgement-field
addition. With `alpha = (3*n+1)*u < 1`, the feedback factor is `gamma = alpha/(1-alpha)` and the absolute
allowance is `d = gamma*(1+M)`; when there are no commands, `d = 0`. The count limit `n <= 2^20` keeps
`alpha` below 1. Command magnitudes, sums, the feedback denominator and the resulting allowance are all evaluated
outward in binary64. Paired nonnegative windows have ideal score-up state at least 1; the separate `d < 1/2`
gate therefore gives a positive rounded-state lower bound. The implementation rounds the endpoints
`[1-d, 1+M+d]` outward to binary32.

For each note, the positive score chain at power 1 and the lower factor endpoint must have normal nonzero
intermediates, and the chain at the power/factor upper endpoints must remain finite. Zero factors remain exact
zero. After the first integer floor, the final life/assist chain is also checked at its least positive integer
input, 1. The sum of the resulting integer note caps must be at most `i32::MAX`.
These checks are in [the live bound](../crates/ournotes-search/src/search/live.rs) and
[`LiveModel::prove_search_domain`](../crates/ournotes-sim/src/live/model.rs).
Coefficient construction uses the converted-note divisor after its native binary32 conversion, then rounds every
binary64 product, quotient, contribution and sum upward. The final power-times-coefficient cap is upward-rounded
as well. A real-arithmetic inequality and a binary32 chain margin do not by themselves certify an unbounded number
of nearest-rounded binary64 additions.

With these certificates, the factor sequence is independent of power and every per-note operation is
nondecreasing in it, including finite integer saturation. Nonnegative, nonwrapping note sums preserve that
order. Maximizing over performance orders preserves it too. The member set's best representative is therefore
its highest-power leader/Snap choice with its best order. An unproved numeric domain returns `Error::Domain`;
the regular evaluator retains its declared conversion and wrapping behavior.

Bound: for note `n`, the score is at most `P * k_n * U_n * (1 + e)`, where `k_n` is the product of the note's fixed
constants (assist, life, judgement and note percentages, score adjustment, level factor, combo factor, divided by
the converted note count), `U_n` bounds the score-up factor and `e = 2e-6` covers the float roundings of the chain
(at most 16 roundings of relative size 2^-24). `U_n = 1 + S_n + d`, where `S_n` sums, over the skill events whose
effect window can contain the note, the largest factor any available performer gives at that event (for the note's
judgement), and `d` bounds the accumulated rounding of the running factor state (one rounding of at most 2^-24 of a
value below `1 + sum |factor|` per command). The floors only lower the score. The search compares `P_bound * A` with
the K-th live score, `A = sum_n k_n U_n (1 + e)`, first with every pool member as a possible performer and, for a
complete member set, with that set's members only. A complete member set that passes gets every distinct assignment
of its live skills to the five positions evaluated exactly.

## Live score with snap skills

Without `exclude_snap_skills` the live objective is the score of the whole-live simulation (`live::full`) with
Gekisou off, under a judgement stream: the frames of a play and the notes judged in each, with their judgement before
conversion and their judgement time, plus the random seed and the assist flag (format in
`live::model::JudgementStream`). The performer at position `k` is the member in slot `performance_order[k]`, with its
live skill and the support skills of the snap in the same slot at the levels of the snap's rank. Conditions that read
the Gekisou state are reported as unsupported.

The default stream (`JudgementStream::theoretical_best`) plays at 60 fps: frame `i` is at `floor(i * 1000 / 60)` ms,
from `i = 0` while that time is at most `T + 2000`, where `T` is the latest time of a judged note or a skill event.
Every judged note is judged Perfect in the first frame whose time reaches its chart time, with the chart time as
judgement time, in the order (chart time, note id) within a frame; the seed is 0 and assist is off. With Gekisou off
the Just judgement is never enabled, so Perfect is the highest judgement.

A snap's skills act for its slot's member at that member's position, so a member set's best representative is no
longer its highest-power leader and snap choice: the search looks for the best (leader, snap placement, performance
order) together. The argument has five parts.

**1. Power enters only the per-note chain.** The deck power sets the band total power of the score calculator, which
no effect changes, and nothing in the simulation reads the score or the power (the score-rank condition never holds).
Judgements, conversions, combo, life, random draws and the start and end of every effect are therefore the same for
every power, and so is the sequence of factor states each note is scored and rescored with, including the frames that
are undone and executed again. The score is the 32-bit sum over the judged stream entries of each note's last computed
score, and each note's score is non-decreasing in the power when every multiplier of its chain is non-negative. The
compiler must establish both that sign condition and a finite, nonwrapping numeric domain. A small final real-valued
score estimate alone cannot establish either one: an intermediate binary32 product can overflow before a later
small multiplier, and a finish filed before its start can give an earlier note a negative factor.

The admission checks in
[`SnapLive::new`](../crates/ournotes-search/src/search/snaps/snap_live_build.rs) and its consuming search routes
require the following certificates:

| Quantity or transition | Required domain | Role in the proof |
|---|---|---|
| Clock values | Frame, chart-note and skill-event timestamps are nonnegative; each note is judged in a frame at or after its chart time. | Count-triggered execution times cannot be in the future relative to the update that creates them. |
| Music-length finish clamp | For a positive music length, every chart note maps to a native `ScoreFrames` frame strictly before the finish-clamp frame. Play frames may continue after the clamp. | A finish clamped before its own start remains strictly after the scored prefix, including binary32 time projection and the last-addressable-frame clamp. |
| Live duration extensions | Every compiled `15000` row has nonnegative raw value. | Once an effect is executing, an extension cannot move its duration backward through its start. |
| Score and life effects | Score additions are nonnegative; life base is in `[1, 2^29]`, damage in `[0, i32::MAX]`, and native `i32` recovery in `[0, 2^30)`. | Factor windows have nonnegative ideal contributions and the life interval used for classification is valid. |
| Score adjustment and difficulty | Adjustment is in `[2^-16, 2^16]`, difficulty in `[2^-8, 2^8]`; converted note count is positive. | Positive products and division have bounded exponents. |
| Post-floor factors | Assist and life-zero factors are either zero or in `[2^-16, 2^16]`. | Zero stays exact; nonzero post-floor multiplication is finite and normal. |
| Percentages and factor norm | Note and judgement percentages are integers in `[0, 10^6]`; the outward total factor norm is at most `2^16`. | Nonzero percentage factors have a positive lower bound, and intermediate upper bounds are finite. |
| Accumulated rounding | The certified factor and chain allowance exists and `eps < 1/2`. | The rounded score-up state stays positive, not merely below an upper envelope. |
| Power and total | The search proves the whole feasible power interval lies in nonnegative `i32`; the score envelope stays below half the `i32` range. | The power cast and the nonnegative note sum preserve order. |

Forward nonnegative windows have ideal note-plus-judgement factor at least 1. The absolute drift certificate below
then gives a rounded factor greater than `1/2`. For a positive input, power and the converted-note divisor are
between `1` and `2^31` after conversion to binary32; positive note and judgement percentages divided by 100 are
between `2^-7` and `2^14`. The ordinary combo factor is in `[1,2]`, since the supported rows exclude changes to
ordinary `combo_score_up`. With Gekisou enabled, its admitted positive combo factor is at least `2^-24` and at most
2, and its positive luck percentage factor is greater than `2^-7` and at most 2. Disabled Gekisou factors and the
whole-live event factor are 1. The rounded note-plus-judgement factor is below `2^17`.

Multiplying these exponent bounds in the native chain's order bounds every positive pre-floor intermediate by

    2^(-16-8-7-7-24-1-7-31) = 2^-101,
    2^(16+31+8+14+14+1+1+17+1) = 2^103.

Both endpoints are inside the normal binary32 range. After the first floor/cast, a positive value converted back
to binary32 is in `[1,2^31]`; the life and assist multiplications stay in `[2^-32,2^63]`. Zero operands stay exactly
zero because all the other intermediates are finite. These ranges exclude intermediate overflow and underflow of
a nonzero product. They concern the ordinary per-note chain, including its admitted Gekisou factors, rather than
the separate expression for retained network snapshots. Floors and finite integer saturation preserve monotonicity, and the
total cap excludes wrapping of the final sum. For fixed performers and stream, the final score is therefore a
non-decreasing function of power. These are conservative admission conditions; rejection does not imply that a
particular deck would score incorrectly.

The best-order diagnostic also rejects external rank confirmations: monotonicity of the ordinary per-note chain
does not prove monotonicity of an arbitrary signed difference of retained network snapshots. The formal facade's
network traversal uses its separate bounds and retains physical team identities. A failure to compile the optional
Snap Live bound makes that facade use its complete fallback; the diagnostic entry point reports the unsupported or
unproved domain directly.

**2. The leader.** Performers follow the members, whatever their slots, and the snap terms of the power `W(m, s)` do
not depend on the leader; so the leader changes only the member-only part of the power. The best representative's
leader is the allowed member with the largest exact member-only power (ties: smallest id); a leaf reached with another
leader returns at once. If the canonical leader's leaf is pruned, its bound is below the K-th value, and every other
leader gives the same performers at a power no larger, so the member set cannot enter the result.

**3. Snap classes.** For each allowed member `m` and each allowed snap, every effect row of the snap's support skills
is classified. Two facts come first, from the stream and every allowed card:

- the *reachable judgements* of each stream entry: its raw judgement, and the target judgement of every conversion an
  allowed card has that lists the raw judgement and can see the entry. Conversion functions only see raw judgements. A
  conversion of a live skill, or of a snap row with another trigger or a release checker, can see every entry in the
  relaxation; a snap conversion triggered only by its performer's own skill event, without release, is registered in
  the frame where the event fires and sees the entries
  judged in the next frames up to the first frame later than its start plus its activation time (with a margin of
  `2^-22` relative and 1 ms);
- the *life interval* `[lo, hi]`: `lo` is the base life when no reachable judgement of any entry costs life, else 0;
  `hi` is twice the base when some allowed card recovers life, else the base. Every life value the simulation computes
  lies in it, whatever the order of the life commands and the state of the life frame cache: recovery caps at twice
  the base, damage floors at 0, and without damage nothing lowers the life (the search checks that damage is
  non-negative and native recovery in `[0, 2^30)`, so no 32-bit sum wraps). Life is *rigid* when every life condition of any
  allowed card is decided on `[lo, hi]` (true for every life in it, or false for every life in it) and the note's life
  factor is fixed (`lo > 0` or the life-zero factor is 1); then no life value can change the score.

A checker is *impure* when asking it can change something else: a probability condition (it draws from the live's
random stream) or, unless life is rigid, a life condition (it queries the life controller, whose frame cache depends
on the queries). A row is

- *never*: its trigger cannot hold for `m` (no trigger, or a trigger that is fixed false for `m`: a member target
  that does not match, the score rank, a lottery, which has no results without Gekisou) or its condition cannot hold,
  and none of its trigger and reset checkers (and, when the trigger can hold, its condition checker) is impure. It
  never starts, and checking it changes nothing;
- *inert*: it can start but cannot reach the score, and none of its trigger, reset, condition and release checkers
  is impure and its cumulative condition is valid: life recovery, guard and damage reduction when life is rigid;
  a judgement conversion
  when no raw judgement of the stream is one of its targets other than the one it converts to (a conversion that
  never converts changes neither the judgement nor the order of the other conversions);
- *active*: anything else (score factors, extensions of the member's live skill, and every row with an impure
  checker).

Removing never and inert rows, and skills left without rows, from a performer leaves the simulation's score
unchanged: their factor commands do not exist, the life values they change are read by nothing that can change the
score, and the other rows keep their order. A snap's *class key* for `m` is the list, per skill with active rows (in
the performer's order), of its active rows by row id, each written with every field the simulation reads except its
id; a condition that is decided true and pure is written as none.

Deleting a row ID requires an additional identity certificate in
[`rows.rs`](../crates/ournotes-search/src/search/snaps/rows.rs). A condition effect's native key is
`100*row_id + 10*kind + position`, for `kind` in `3..=5` and position in `0..=4`. The compiler checks
`row_id.checked_mul(100).and_then(|x| x.checked_add(54))`: every admitted key is nonwrapping and its order within
a skill is the raw row order kept by the class key. Different actual source rows may not share an ID within their
source table, and a single Snap's program may not register the same condition identity twice within one skill
kind. Repeated references to one actual row by different candidate cards remain legal.

Conversion targets have a separate alias rule. Their native cache is keyed by raw row ID across live, support and
Gekisou sources, so compiled conversion rows sharing that ID must have identical target judgement vectors. Merely
preserving sort order would not prove this cache behavior. The checks cover rows of the allowed cards at their
selected levels; an unrelated row elsewhere in the master table does not invalidate the certificate.

Under these conditions, replacing equal class keys preserves effect order, a one-to-one correspondence of private
states, and every conversion cache lookup. The fixed member retains its live-skill identity and order. Thus equal
keys are interchangeable at `m`; this is conditional equivalence, not a claim that arbitrary native row IDs are
irrelevant. The empty key (class 0) holds the snaps that cannot change anything at `m`, together with "no snap".

For a member set, a performance order and a class for each slot, part 1 applies: the score is a non-decreasing
function of the power alone. The best snaps for that choice are the maximum-weight assignment of snaps to slots
restricted to each slot's class (class 0: no snap or a snap of class 0), each snap at most once, ties to the smallest
snap ids in slot order: the folded assignment of the power search, with forbidden pairs priced below any assignment
that respects the restriction.

**4. Bounds.** For a judged stream entry `e` at chart time `t_e`, its last computed score is at most

    floor(Z_e * floor(P * K_e * V_e * (1 + eps)))

- `K_e`: score adjustment times level factor times the note's percentage times the largest combo factor at any combo
  up to `c_e`, divided by the converted note count. The combo a note reads counts the entries at earlier chart times
  since the last one that breaks it. An entry whose reachable judgements are all Miss or Bad breaks it whatever the
  conversions; `c_e` counts the entries at chart times before `t_e`, from the first entry at the time of the last
  such entry before `t_e`;
- `Z_e`: assist factor times the largest life factor (1 when `lo > 0`, else the larger of 1 and the life-zero factor;
  for some candidates the life-zero factor alone, below);
- `V_e = max_j jp(j) * (1 + N_e) + sum_j jp(j) * J_e(j)`, over the reachable judgements `j` of the entry, `jp(j)` the
  judgement percentage, `N_e` and `J_e(j)` the note and judgement factors whose windows contain `t_e`. The note ends
  with one reachable judgement `x`, and its score-up value `jp(x) * (1 + N + J(x))` is at most `V_e`.

Above the candidates these coefficients use the reachable judgements of every allowed card. For one candidate (a
member set, a performance order and a class for each slot) only its own performers register conversions, and one
triggered by its performer's skill event only at that position's events; the per-entry bound of a candidate uses the
reachable judgements of its own conversions, and the combo breaks that follow from them. They are a subset of the
pool-wide ones, so the candidate's combo counts and percentages are never larger.

Ordinary life damage. For a candidate whose life state consists of base life and judgement damage, a note reads
at most `max(0, base - filed minimum damage)`. The filed damage comes from entries judged in earlier frames, or earlier in
the same frame, the note itself included, with chart times up to the
note's: damage only lowers the life and floors at 0, and a life query folds every filed command up to its time at
least once (with the frame cache, some of them twice). The damage of an entry is at least the smallest damage of its
reachable judgements. Where this bound is 0 the note's life is 0, and `Z_e` is the assist factor times the life-zero
factor. When all allowed cards have this life behavior, the pool-wide coefficients use it too.

Life effects. Recovery (`3001`), guard (`3003`) and damage reduction (`3004`) use the general life
interval when their activation can affect the score. Damage reduction changes the damage commands that a life
query folds, so its member and Snap classes retain the general score envelope. The recovery-specific folds below
use Snap and Gekisou recoveries triggered at their own skill events. A candidate or prefix that can include guard,
damage reduction, recovery at another trigger, or a life effect in a live skill uses the general envelope.

Life with recoveries. The life controller keeps a log of life commands (note damage at the note's chart time,
recovery and guard at their execution times) in 40 ms life frames, and a query at time `t` folds the commands with
times up to `t` in time order: a damage `d` maps the life `x` to `max(0, x - d)`, a recovery `r` maps `x > 0` to
`min(2 * base, x + r)` and keeps 0. Each map is non-decreasing in `x`; a damage never raises the life and a recovery
never lowers it. So removing a damage command or adding a recovery never lowers the result, and a life of 0 stays 0.

The frame cache: a query at life frame `q` stores the fold of the frames before `q` and marks the cache complete up
to `q - 1`; a command filed at a frame `f` up to the mark lowers the mark to `f - 1` but keeps the stored life, so the
next query folds the frames from `f` again, starting from that life: the commands of the frames from `f` up to the old
mark are folded twice. Such a command is filed after a query at a life frame above its own. Queries happen at each
play frame's time and at each judged note's chart time; the commands are the note damage (filed just before the
note's query), and for these candidates the recoveries of snap rows triggered by the performer's own skill event,
filed at the time of the frame where the event fires, after that frame's notes. From the stream the search takes, for
every note and every skill event, the window `[f, q - 1]` of its life frame `f` and the largest life frame `q` queried
before it is filed, when `f < q`. Every twice-folded frame lies in such a window, and each refold folds a command once
more, so a recovery at life frame `F` is applied at most `1 +` (the number of windows containing `F`) times.

The fold is taken over *slots*: a run of overlapping windows is one slot (ending at the end of its last life frame),
and every chart time outside the runs is one slot. A computed life is then a fold, slot by slot, of the filed commands
with some of them repeated inside their slots, and possibly with commands of a slot's later frames folded before its
earlier ones. Within a slot the fold uses `clamp(x + r - d, 0, 2 * base)` when it has both damage `d` and recovery `r`
(each recovery counted as many times as it can be applied): whatever the order and the repetitions, the result is 0
when the life starts at 0, and otherwise at most `x + r - d` (a fold that never reaches 0 adds at most the recoveries
and subtracts at least the damage; one that reaches 0 stays there) and at most `2 * base`. Repeated damage only lowers
the result.

For a candidate whose life-raising rows consist entirely of such recoveries, the search folds, slot by slot,
every entry's smallest damage and the candidate's recoveries at their
events, and takes the end `t0` of the first slot at which this fold is 0. An entry at chart time `t_e` reads life 0
when `t0 <= t_e` and every entry at a chart time up to `t0` is judged no later than it: its query folds all those
damages and only recoveries this fold contains, at most as often, so the life it reads is at most the fold's value at
`t0`, which is 0, and the later commands keep it 0.

Final life. The final life of a play is the life the query at the last play frame reads, a fold of the commands filed
at times up to that frame's time. For such a candidate, the same slot-by-slot fold over the smallest damage of the
entries at chart times up to the last play frame's time and the candidate's recoveries at their events is at least
that life, and it is 0 from the first slot at which it reaches 0. A score and life target pays nothing in an order
where this fold is below its least final life, so the per-order caps of that order are 0. This bound requires the
classes to retain every life-preserving row. When life is rigid for score evaluation, the final-life target is
evaluated by the live simulation.

Windows. A factor started at `exec` holds for the notes with chart times in `[exec, finish)`: its start and end
commands are filed at those times, and a command filed in a score frame that was already executed undoes and
re-executes the frames from there, so every note is last scored with the factors whose commands surround its chart
time. A live skill effect of position `k` starts at the time of each skill event of `k` that some frame reaches. Its
duration is `d = act * 1000 + extension`, in binary32. The first update after its start keeps it running only when
`d > RN32(frame_time.wrapping_sub(exec))`; otherwise it finishes at that update's time. A running execution with
positive activation ends when the native strict predicate `d < RN32(frame_time.wrapping_sub(exec))` holds, and files
its finish at the native `exec + ceil(d)` timestamp, subject to the music-length clamp. A later real-valued frame
time alone does not imply that predicate: the integer-to-binary32 conversion can round the elapsed time back to `d`.
An activation time that is not positive ends in the next frame; with an extension the bound then uses no end.
On the admitted nonwrapping clock domain, `d` is non-decreasing in the
extension (binary32 addition and `ceil` are monotone), so the bound uses the largest extension: the extension rows of
the member's live skill and of its snap class that can start, each once per event of the position. It uses no end at
all when an extension row of the snap class has a trigger other than the performer's own skill event, or when the
position has two events (a restart does not remove the first factor). A Snap score row triggered by its performer's
own skill event, without a release checker, starts at the event frame and uses the same bound without extension.
A release checker skips the first elapsed-time check and can retain an untimed factor indefinitely; such a row
uses the general conservative envelope. Ordinary Snap rows with another trigger also count over the whole live,
five concurrent executions at once. Live skill rows of one
member with the same effect type and duration whose conditions are one condition each, negations of each other, cannot
both start at one event: both are checked in the same phase of the frame, before any effect of that phase applies, and
repeated life queries at one time give one value (probability and count conditions, which change with each check, are
excluded). When the position has at most one event such a pair counts once with the larger factors. Every factor value
is non-negative, so a window that is too wide only raises the bound.

Float margin. `eps` covers `2e-6` for the per-note chain (as without snap skills), `2^-22` for the score-up sum,
`2^-19` for the factor after the floor, and the drift of the running factor state, a binary32 sum. Each float
operation on it rounds by at most `2^-24` of its magnitude. A score frame is executed at most `E(g)` times (its first
run and its re-runs). In each play frame, the two recalculations can re-execute from the earliest score frame covered
by its certified timer floor, the chart times of its judged notes, and its Gekisou override times. Rank confirmations
have their separately counted replays. Commands mapped strictly after the last note's score frame cannot change
that scored prefix.

The timer floor is a separate certificate in
[`command_floor_times`](../crates/ournotes-search/src/search/snaps/score_windows.rs). It uses the previous play-frame
time only when frame times are nondecreasing; frame, chart-note, skill-event and Gekisou range times all lie in
`[0, 2^24]`; extension rows are nonnegative; and positive-duration rows have neither a release checker nor a gated
Gekisou sustained updater. The interval makes every elapsed integer exactly representable in binary32.
An execution that survives the previous update has `d_previous >= previous_time - exec` (strictly greater on its
first update). A nonnegative extension keeps `d` from decreasing, so a later timed finish satisfies
`exec + ceil(d) >= previous_time`. Its successful current strict end test also keeps that timestamp within the
current nonwrapping clock range. A release checker can skip the first elapsed check, and a gated sustained updater
can skip a previous update entirely; neither supplies this premise. Without the compact certificate the timer
floor is score frame zero. Every case also includes the positive music-length finish clamp. This same floor feeds
the execution counts and the network snapshot cancellation bounds. A wider replay interval can increase the
rounding allowance or make the optional cap unavailable; it does not change the scorer.

Frame geometry is a separate obligation. `ScoreFrames` projects a possible factor span `[a,b]` onto the closed
native frame interval `[frame(a),frame(b)]`, using binary32 time conversion and the last-addressable-frame clamp
of the actual scoring clock, including a nonzero `score_music_length_ms` override. Every factor applied, removed
or active in a frame is contained in this projected span. Summing its magnitudes bounds the frame's transient
state; closed endpoints include equal-time start/end pairs and final-frame clamping. A fixed 40 ms neighbourhood
is not equivalent to that map: at large timestamps even more widely separated times can share a native frame.
The compiled raw envelope sums per-slot maxima, which is at least the maximum of the whole team's sum.

A positive music-length clamp needs a frame-level premise too. Raw timestamps `1039 < 1040` can both map to native
score frame 26. If an effect starts at 1100 and its finish is clamped back to 1040, a nominal span beginning at 1100
does not count that earlier frame. Admission therefore requires each note's `ScoreFrames` image to be strictly
before the clamp's image. This also covers a shortened `score_music_length_ms` override. The ordinary
`DataChart::chart` route uses `Chart::from_notes`, which sets `last_timing_note_ms` to the maximum note time, and
`FullSetup` adds 1000 ms to obtain the music length. Direct core callers can supply independent public `Chart`
metadata, so the compiler checks the frame relation instead of assuming this construction. The rule only restricts
the optional certificate; play frames after music length remain allowed and the native evaluator is unchanged.

Let `N` bound lifetime factor commands and `E` their total executions including all frame replays. For a row
touching `h` score fields, let `s` bound starts, `c` concurrency, `f` processing frames and `q` replacements per
activation. The implementation charges

    N_row = 2*h*(s + min(s*q, c*f)).

A fixed factor has no replacements. A finite cumulative replacement count is used only when mill quantization
reconstructs the identical binary32 factor at every reachable requested value; otherwise the bound uses the
processing-frame count. Every start, end and replacement is charged, and its possible filing frames determine
the replay multiplicity used for `E`.

For one candidate, let `F` bound factor magnitudes meeting a native score frame and
`S = 1 + max(F, peak active factor sum)`. With `u = 2^-24` and `W = 3*E + 2*N`, an absolute factor-error cap is

    D = u*(E*(2*S+F) + 2*N*S)/(1-u*W),       u*W < 1.

State addition, frame-difference addition and undo contribute `3*E`; integer cast and division contribute `2*N`.
The rounding magnitude can depend on earlier error: writing `delta = u*(E*(2*S+F) + 2*N*S)` and `alpha = u*W`
gives `error <= delta + alpha*error`. Solving gives the displayed cap; at `alpha >= 1` it is unavailable.
[`float_margin::amplification`](../crates/ournotes-search/src/search/snaps/float_margin.rs) computes that factor
outward and also retains a minimum factor of 1.01. Thus 1.01 is a floor on the allowance, not the proof for
arbitrary command counts. The nonnegative ideal score-up value is at least 1, so an absolute state-error allowance
can also serve as a relative score-up allowance. Above the candidate level the same argument uses the largest
`E`, command weight and factor totals of the admitted performers.

A fine cap adds `D` to its note-plus-selected-judgement factor before the independent chain allowance. For a
linear joint envelope, `B = sum_e k_e*z_e*max_j judgement_percent(j)` bounds sensitivity to absolute factor error,
including budgeted conversions. Thus `P*(A+D*B)` bounds the unfloored contribution before the chain allowance.
An unavailable global certificate rejects preparation; an unavailable candidate certificate keeps the maximal
cap; an unavailable compiled raw cap returns `None`; an inapplicable refinement retains its enclosing envelope.

The compiler's binary64 window sums need their own certificate. With `W` windows there are at most
`2W` endpoint writes, `2W` nonzero prefix additions and one factor multiplication on an input path.
`WindowRoundoff` bounds absolute error by `gamma_(4W+1) * endpoint_L1`, with `u = 2^-53` and outward
evaluation of both terms. The L1 norm makes the bound applicable to cancellation at window ends. An invalid
operation count, nonfinite value or `(4W+1)*u >= 1` makes this optional certificate unavailable.
The implementation combines independent relative allowances multiplicatively, rounding outward, before
subtracting 1 to obtain `eps`. Ordinary real-algebra identities alone do not supply these numerical certificates.
Compiled raw coefficient prefixes retain both lower and upper endpoints. An interval cap uses
`upper[end] - lower[start]`, with outward subtraction: subtracting two upper prefixes has no general upper-bound
guarantee. The reusable representation is specified in [compiled envelopes](search-envelope-programs.md).

Separability. Every window belongs to one position, so the sum over the entries of the per-entry bound without the
floors is `P * (A0 + sum over positions k of G(k, m_k, c_k)) * (1 + eps)`, where `A0` is the value of the entries
with no factor and `G(k, m, c)` the value of the windows of position `k` for member `m` with snap class `c`. The bound
above the leaves is `P_bound * (A0 + sum_k max over allowed members and classes of G(k, m, c)) * (1 + eps)`, compared
with the K-th value where the power search compares its bound. A snap can extend its member's live skill and add
factors of its own, so this bound is higher than without snap skills and prunes less above the leaves.

The depth-first search over members also bounds the gains of the members themselves. With `g(m, k)` the largest
`G(k, m, c)` over the classes of member `m`, a member set `M` adds at most the largest sum of `g` over the assignments
of its members to the positions, which is at most both `sum over positions k of max over M of g(m, k)` and `sum over M
of max over k of g(m, k)`. At a node that has chosen the members `C` (the leader, the fixed members and the picks so
far) and still picks `r` members among the characters from the node's position on, the members to come are cards of
distinct characters there. So the gain is at most the smaller of `sum over k of max(max over C of g(m, k), max over
the cards of those characters of g(m, k))` and `sum over C of max over k of g(m, k)` plus the `r` largest single-card
maxima of distinct characters there. Both fall as the position moves on, like the power bound, so a failed test ends
the loop over characters; a failed test for one card skips that card only. A complete member set gets the largest
assignment sum itself (a recursion over subsets of the five members) before its leader terms are computed, and again
with its exact member-only power. Each test is `P * (A0 + gain) * (1 + eps)` against the K-th value, never above the
test with the largest gain of every position.

**5. Leaf.** For a member set at its canonical leader, with member-only power `F`:

1. every performance order gets `(F + sum_i max_c w_i(c)) * (A0 + sum_k max_c G(k, m_{order[k]}, c)) * (1 + eps)`,
   with `w_i(c)` the largest snap weight of class `c` in slot `i` (distinctness relaxed); orders are visited by this
   bound, and the leaf ends at once when the life test below fails before any order;
2. within an order, a depth-first search over the slots chooses classes; a branch is cut when
   `(F + chosen w + best remaining w) * (A0 + chosen G + best remaining G) * (1 + eps)` is below the cutoff, or when
   the life test fails (with no class chosen yet: for the order);
3. a complete choice gets its exact power from the restricted assignment (or is infeasible), the same product bound
   with that power, the product bound with the life-zero factor from the first entry (chart-time order) after which
   every entry reads life 0 under the candidate's life bound (the sums of `A0` and `G` split at that entry), and then
   the per-entry bound of part 4 with its floors, the candidate's reachable judgements and its own margin;
4. surviving candidates are simulated in descending order of the per-entry bound (ties: power descending, then snap
   ids and order ascending). The first candidate is the best-bound order with a greedy class choice, and pending
   candidates are simulated as the list grows, so the cutoff rises early. Candidates whose performers are the same for
   the simulation (members with the same live skill, band and card type, and character when a member target reads
   characters; snaps with equal class keys; same order and power) are simulated once.

Life test. Call a class *plain* when its only life-raising rows are recoveries at its performer's skill events, or
when it has none. A node whose chosen classes are all plain covers two kinds of completions. Those with a class that
is not plain at some remaining slot `j` are bounded by the product with slot `j`'s best such `w` and `G` and the best
of every other remaining slot. Those with plain classes only recover at most the chosen classes' recoveries and, at
each remaining slot, its largest plain one; the life fold of step 3 with these recoveries gives a start, and they
score at most `(F + chosen w + best remaining plain w) * (A0' + chosen G' + sum over remaining slots of H') *
(1 + eps)`: `'` marks the sums split at that start as in step 3, and `H'` is the smaller of the slot's best plain `G`
and its envelope, the sum over the entries, at `Z_e` before the start and at the life-zero factor from it, of the
largest value of the entry over the member's plain classes at that position. The test fails when all these bounds
are below the cutoff. Before any order it uses the largest plain recovery of any slot at every position, and for the
gains the best assignment of slots to positions. Each slot of the life fold is non-decreasing in its recoveries
(`clamp(x + r - d, 0, 2 * base)` rises with `r` and is at least `max(0, x - d)`), so smaller recoveries, or none,
give no later `t0` and no later start, and a later start leaves at the life-zero factor only entries that read life 0
for every covered candidate (with no recovery at all, the dead entries of the life bound without recoveries). A
class's split gain is the sum over the entries of its value times the entry's factor, which the envelope bounds entry
by entry, and a split sum is at most the plain one: the life-zero factor is at most `Z_e`, and every coefficient and
factor is non-negative.

The cutoff is the larger of the K-th value and the best score simulated in this leaf. A candidate is dropped only when
its bound is strictly below the cutoff, or equal to the best simulated score while its power, snap ids and order cannot
win the tie. Each bound is at least the exact score of every deck it covers (parts 3 and 4), so the leaf returns its
member set's best representative, and a member set whose leaf finds nothing at the K-th value cannot enter the result.
A search that reaches its time limit inside a leaf (checked before each simulation and every 1024 nodes of the
class search) keeps that leaf's best simulated deck and reports `TimedOut`.

Cost. A member-set leaf can be excluded entirely by its bounds. Surviving candidates require exact simulation or
a proven equivalent cached value. Missed or late notes can make combo, conversion and life envelopes looser, leaving
more candidates to evaluate. A time limit stops work cooperatively; one atomic preparation, assignment or evaluation
can overrun it. The [complexity discussion](#search-size-and-complexity) gives the finite-domain bounds.

## Canonical team power and deck-payoff routes

### Canonical team power frontiers

The deterministic team route is implemented by [team power bounds](../crates/ournotes-search/src/search/team_power.rs),
[team traversal](../crates/ournotes-search/src/search/team_power_search.rs) and
[Snap assignment](../crates/ournotes-search/src/search/matching.rs). It applies to native power, or to Skip score,
`scoreAtLeast` and `cappedScore` after compilation certifies a nonnegative, nonwrapping power interval and a
nondecreasing Skip score throughout that interval. If `u(P)` is the selected utility, the order
`(u(P) descending, P descending, canonical IDs ascending)` equals
`(P descending, canonical IDs ascending)`: a greater power never has a smaller utility, and power resolves every
utility plateau. This argument requires the secondary power comparison, including for thresholds and caps.

Members are assigned in slots `[2, 0, 1, 3, 4]`; the four nonleader IDs increase strictly. Every legal choice of leader
and four other cards therefore has exactly one member layout. Distinct leaders and distinct Snap bindings remain
distinct teams. Required cards reserve their characters, already used characters are unavailable, and a required
nonleader below the next allowed ID makes a prefix infeasible. These checks remove exactly the prefixes without a
legal continuation, rather than using card quality to reduce the domain.

For a prefix with `r` unfilled member slots, let `H_R^r(v)` be the sum of the values for the required remaining
characters `R` plus the largest `r - |R|` values of other available characters. Each character's value is a maximum
over its allowed cards. With a fixed leader profile, put `b(m) = A(m) + LEAD_bound(m)` and
`wmax(m) = max(0, max_s W(m,s))`. Two power bounds are:

```text
U_pair = sum_fixed (b(m) + wmax(m)) + H_R^r(max_card (b(m) + wmax(m)))
U_resource = sum_fixed b(m) + H_R^r(max_card b(m))
             + sum_of_five_largest_s max(0, max_fixed_or_available_m W(m,s))
```

For `U_pair`, each actual card contributes at most its character maximum, and independent row maxima relax Snap
uniqueness. For `U_resource`, the member sum is bounded independently and every used Snap contributes at most its
column maximum; a team uses at most five distinct Snaps. Both dominate every completion, so their minimum does too.
At an unassigned leader the maximum over possible leader profiles is also safe. At a complete member layout the
bound uses an exact maximum-weight Snap assignment and the bounded leader constant. Complex leader effects can
make that constant loose; they do not change the Snap weights or the upper-bound direction.

**Fixed-layout assignment lemma.** For a complete member layout `M`, exact native power has the form
`P(M,s) = P(M,None) + sum_i W(M_i,s_i)`. The first term is independent of the Snap binding, including with complex
leader effects. Scan Snap resources once in public-ID order. After a scan position, a partial binding is described
by its occupied-slot mask and assigned IDs. Two partial bindings with the same mask admit exactly the same future
extensions: later resources are unused by both, and precisely the same slots are free. Applying a common extension
adds the same power and fills the same previously empty positions, so it preserves both power order and the first
existing lexicographic difference.

Consequently, if a partial binding is behind `K` distinct partial bindings of its mask, every completion of it has
`K` distinct better completions, obtained by applying the same extension to those witnesses. It cannot belong to
that layout's Top-K. Induction over the resource scan proves that retaining the first `K` partial bindings per mask
preserves the exact final Top-K, including empty slots and ties. Different final masks are merged before the final
truncation. Resource scanning gives each binding one construction path, so the witnesses are distinct identities.

**Local-to-global lemma.** Any binding omitted after keeping the first `K` of one layout has `K` distinct better
teams of that same layout. It therefore cannot enter the global Top-K over all layouts. The evaluator supplies the
reported native values; the leader bound is never substituted for an evaluated result. A member-prefix power cap
below the incumbent K-th power permits pruning. On power equality, pruning additionally requires the prefix's least
possible canonical key to be strictly greater than the K-th key. The key uses the lexicographically least feasible
member completion and `None` in every Snap slot, which is a lower key for all its bindings under `None`-first order.

### Deck payoff curves and bounds

[Deck payoff bounds](../crates/ournotes-search/src/search/deck_payoff.rs) use the additive bonus
`B = sum_members beta(m) + sum_used_Snaps beta(s)` and native power `P`. The compiled function `F(B,P)` is
nondecreasing in both arguments. For deck-determined rewards it is exact; for score-dependent rewards it is an
upper bound. Those two cases have different completion rules.

| Payoff function | Meaning of the compiled value |
| --- | --- |
| `floor(reward * (B + 10000) * rate / 10000)` | Exact Skip event points at the configured result rank. |
| `reward * rate` | Exact Skip challenge-point earnings; no card bonus is applied. |
| `sum_j floor(count_j * (B + 10000) * rate / 10000)` | Exact conditional items for the declared selected rewards of the requested resource. Each reward is rounded separately. |
| Score-step function at a score cap | Upper bound for each played order's score-dependent payoff. |

The compiler resolves the requested result route, active events, reward rows and effect event before selecting a
curve. It checks every contributing card-bonus cast and addition. Nonnegative bonuses make the largest legal
five-member bonus no greater than the five largest per-character maxima; adding the five largest Snap bonuses
also bounds every optional distinct-Snap selection. Compilation checks `B + 10000` and the native intermediate
products over this interval. In particular, a zero final rate does not excuse overflow of the earlier
`reward * (B + 10000)` product. These checks establish monotonicity of the integer arithmetic actually evaluated.

At a member prefix, the fixed bonuses plus required-character maxima and the best remaining character values,
together with the five largest Snap bonuses, give `B_upper`. The team power bound gives `P_upper`. Every completion
has `B <= B_upper` and `P <= P_upper`, so `F(B_upper,P_upper)` bounds its payoff even if those two maxima cannot
occur in one team. This is a relaxation; no assumption of positive correlation between bonus and power is needed.

For played score-step payoffs, [the joint compiler](../crates/ournotes-search/src/search/joint.rs) supplies a
per-outcome score cap `C(P) = ceil(P * global * (1 + eps))`. A reward table may decrease at a higher score: the bound
uses the prefix maximum of the reachable tiers, rather than assuming the actual rewards are monotone. A target
uses the indicator that `C(P)` reaches its threshold; a score-and-life target may relax the life requirement.
Thus each order's payoff, and its lottery expectation when applicable, is at most `F(B,P)`. Multiplying by 120
bounds the uniform-order payoff numerator. This conversion requires an orderwise score cap; a bound on mean score
alone would not justify applying a discontinuous target or reward step in this way.

### Snap frontiers with both bonus and power

The payoff frontier scans Snaps with a state `(occupied mask, bonus sum, power increment, binding)`. Common
extensions remain identical within one mask, as in the fixed-layout assignment lemma. A partial binding `x`
dominates `y` for this purpose when its bonus is at least `y`'s and either its power is greater, or its power is equal
and its Snap key is smaller. For every common extension, monotonicity of `F` gives a payoff at least as large; if
payoffs tie, power and then the preserved canonical key put `x` first.

States are processed by descending bonus. A state is discarded only after `K` distinct earlier states beat its
power/key pair. Each is therefore a valid dominance witness with sufficient bonus. Applying any common extension
gives `K` distinct teams ahead of the discarded state's completion. Induction preserves the first `K` full bindings
under `(F(B,P), P, canonical IDs)`, and final sorting produces that order exactly. For a score-step curve, this is
the exact order of the *bounds*, not a claim that the actual played payoffs equal them.

This frontier can keep many mutually undominated bonus/power tradeoffs in one mask. Its width is not bounded by
`K`, unlike the power-only assignment DP. The compilation checks on `K * max(1, number_of_Snaps)` and table sizes
are algorithm-selection limits; they do not prove a wall-clock bound or a bound of `K` states per payoff mask.

### Evaluation and reranking of payoff bounds

[The payoff traversal](../crates/ournotes-search/src/search/deck_payoff_search.rs) ranks all teams by their compiled
payoff, exact power and canonical IDs, then evaluates the first `K`. Use common payoff mass `D = 1` for Skip and
`D = 120` for played Live. An unevaluated team has optimistic numerator `D * F(B,P)`; an evaluated shortfall is
replaced by its exact numerator over that same mass. Numerator/denominator comparisons are checked explicitly.
Replacing a bound by a smaller exact value only moves that team down. A payoff above its bound or a power mismatch
is an error and cannot certify a ranking.

**Reranking frontier lemma.** Suppose `f` bindings of one member layout have been moved down after evaluation.
Any binding outside that layout's original first `K + f` has at least `K + f` bindings ahead of it; at most `f`
of those can move down. At least `K` unchanged bindings still precede it. Therefore recomputing the original
`K + f` frontier, applying the recorded decreases and merging the layouts preserves the new global Top-K.
The count is per member layout, as are the frontier and recorded shortfalls.

**Termination certificate.** In a round, every current ranking value is an upper bound on its team's true value,
while already replaced values are exact. If all first `K` teams have settled values equal to their current ranking
values and no new shortfall occurs, every other team's optimistic rank is behind those exact ranks. Its true rank
can only move down, so the retained exact Top-K is globally correct. When the domain has fewer than `K` teams, all
of them must be accounted for. This argument includes power and canonical-ID ties, not only primary-payoff values.

A known evaluated team whose exact payoff is no longer retained cannot simply be skipped as an unsettled optimistic
entry. For a bounded payoff that situation hands the domain to the joint traversal. A certified lottery evaluation
that falls below the bound, a value that cannot be settled on the common mass, or continued shortfalls after eight
ranking rounds also hands over. The continuation retains evaluated incumbents and searches the full legal domain
with its own admissible bounds. The round limit changes the algorithm; it never converts an unresolved ranking
into `Complete`. A time or candidate stop retains the root payoff cap for all unresolved work, including ranked
teams not yet evaluated, and reports an incomplete search.

## Uniform member-order search

### Target

A team is a leader in slot 2, four other members and the Snap (or none) paired with each member. In a performance
order the five members act at the five skill positions; the paired Snaps follow their members. The target value of a
team is the mean of its payoff over the 120 performance orders, each equally likely:

    U(team) = (1/120) * sum over the 120 orders of payoff(team, order)

An order's value is defined by a whole-live evaluation with the declared judgement stream. The payoff is the final
score (or capped score), or the client event points of that order's final score, rank and life. Power and the
terminal-payoff law are fixed for a team. Permuting the four non-leader member/Snap pairs preserves power and the
distribution over performer sequences; their slots are a layout, not an additional decision. Changing power can
change the payoff.

To justify this quotient, let `h` be a permutation of the four nonleader slots that fixes slot 2. Mapping each
performance order `pi` to `h composed with pi` is a bijection of all 120 orders. The paired performers occur in the
same sequence under corresponding orders, with the same leader, power and declared terminal mapping. Consequently
their payoff multiset and mean are unchanged. All 24 layouts of the nonleader pairs have the same canonical team.
This argument concerns the declared uniform-order law; it does not require the same seed to generate corresponding
raw shuffle sequences.

A skill probability check draws a random number. With Gekisou off such a draw ends the request with `Unsupported`.
In the supported Gekisou domain without a LUCK range, probability-gated effects cannot affect the score, and each order keeps one exact
score (`"lottery":"noLuckRange"`, below). A LUCK range decides its lottery with random numbers; those decks are
ranked by certified intervals over the native lottery probabilities ([LUCK](#luck)).

### Result identity and shape

`resultIdentity` is `team` (`fixedTeam` for a fixed-deck evaluation). A team is reported in its canonical layout: the
leader in slot 2 and the other (member, Snap) pairs in slots 0, 1, 3 and 4 in ascending member card ID order. Every
layout of the same pairs is the same team, and a fixed deck given in any layout evaluates and reports as that team.
Results are ordered by

1. expected payoff, descending; on the deterministic played branch this compares `expectedPayoff.numerator`
   because each denominator is 120;
2. power, descending;
3. member card IDs of the canonical layout, ascending;
4. Snap IDs of the canonical layout, ascending, with "no Snap" before every ID.

Each deterministic played result carries `expectedScore` and `expectedPayoff` as exact fractions over 120,
`scoreSummary` (minimum,
maximum, lower quantiles and the probability of reaching a score target, over the 120 equally likely orders) and
`bestOrder`: the first performance order, in lexicographic order of slot permutations, with the highest payoff and
then the highest score. `bestOrder.performanceOrder` lists the result's slots in performance order,
`bestOrder.members` the member card IDs in that order, with that order's `score` and `payoff`. It reproduces one play;
it is not a decision of the search. `probabilityLaw` is `{"kind":"uniformMemberOrder","orders":120,"lottery":"none"}`.
In a Gekisou live without a LUCK range, a deck whose skills read a probability reports `"lottery":"noLuckRange"`:
without a LUCK range the controller consumes no lottery, the probability gates only lottery chains and
lottery-dependent score-ups, and each order still has one exact score.

`Complete` certifies the first `min(K, number of legal teams)` teams under this order. `TimedOut` preserves
complete deterministic evaluations, or certified enclosures on the LUCK route; the reason can be a time limit,
candidate limit or another incomplete-search exit and is recorded separately. `telemetry.proof` reports the
available bounds on unexplored work; an absent cap means that the unexplored value is unknown.
An exhausted LUCK domain with unresolved interval ranking is `RefinementRequired` when no time or candidate stop
takes precedence. A deadline reached during refinement can still return `TimedOut` after domain exhaustion.
The [LUCK certificate](#luck-ranking-certificate) specifies its exact-value and rank fields separately.

### Traversals

The joint traversal assigns one `(member, optional Snap)` pair at a time in slot order `[2, 0, 1, 3, 4]`, the leader
first. After the first non-leader slot, each slot takes a pair later in a fixed choice order than the previous slot's
pair, so every team is visited once, in one layout. The composition traversal (Live with Gekisou off) branches on the
leader and an unordered set of four other members, then on the Snap pairings of that composition. Leaves evaluate
the canonical layout. Neither traversal removes a legal pairing: there is no per-card quality cutoff, Snap dominance
deletion or representative-only class matching.

### Node bounds

For an assigned leader, the existing `Tables` decomposition bounds each slot's power by
`a[m] + lead[profile][m] + w[m][s]`. Preparation checks nonnegative slot lower bounds and a nonwrapping
whole-deck power domain. Selected pairs contribute their terms. For remaining slots, take a maximum
per character and then the largest required number of character values. Different remaining slots
may reuse a Snap in the relaxation; this enlarges the feasible domain rather than deleting a resource.

The existing Snap Live bound compiler supplies `A0`, `global`, `eps` and each pair's five position gains. Its
supported-domain checks cover causal nonnegative clocks, forward factor windows, finite normal score chains,
conversion/recovery, frame re-execution drift, Gekisou combo and rank bonuses, and the score overflow ceiling.
Class classification is used only to read these upper
bounds: Snap identities remain in search.

In one performance order the score of a team is at most `P * min(A0 + sum of the slots' position gains, global) *
(1 + eps)`. Power `P` does not depend on the order, and under the uniform order every member is at every position
with probability 1/5, so the mean of the gain sum is the sum of each pair's mean gain over the five positions. The
function `x -> min(x, global)` is concave, so by Jensen's inequality the mean score is at most
`P * min(A0 + sum of mean gains, global) * (1 + eps)`. Every node bound therefore reads position-mean gains (rounded
up) and is multiplied by 120 to bound the payoff numerator. Unassigned members take per-character maxima of power
and mean gain, which may come from different cards; this only enlarges the completion set.

For normal-played PT with exactly one held target event, member and Snap bonuses are additive. Preparation requires
nonnegative resolved per-card bonuses, rate and reachable rank values. It checks `bonus + 10000`, its product with
rate, and the complete point product against `i32::MAX`, with `bonus` the largest event bonus of any team: the five
largest per-character maxima of the member bonuses plus the five largest Snap bonuses (a team's members have distinct
characters and its Snaps are distinct). The reward of one order is a step function of its score,
not necessarily increasing. Its prefix maximum over reachable thresholds is at least the reward, and the concave
majorant of that prefix maximum (the upper concave hull of its corners, flat after the last one) is a concave,
nondecreasing function above it. The mean reward over the orders is then at most the majorant at the mean score
cap, again by Jensen's inequality; the majorant is evaluated with rounding up, then the event bonus applies. At a
complete team the per-order caps take the largest reward among the tiers reachable below each order's score cap.

A numeric branch test removes a branch only when its numerator cap is strictly below the full Top-K threshold,
or the numerators tie and its power cap is strictly lower. With both numeric keys equal, the general test retains
the branch. Routes that additionally construct a valid lower bound on the canonical ID key may prove exclusion
using that key, as in composition search. The completion theorem below includes both rules.

Unsupported bound domains, explicit duration/delta-clock overrides, negative bonuses and possible PT wrapping
select exhaustive fallback and report `telemetry.environment.bounds.fallback`. Preparation errors are not converted
into optimistic bounds. `telemetry.joint` and `telemetry.composition` (checks and prunes of every bound),
`telemetry.environment.bounds.compileMs` and `telemetry.proof` expose the proof work ([telemetry](telemetry.en.md)).

### Bound modules

Further bounds of partial teams are modules behind one interface (`joint::NodeBound`): given the members of the
first `depth` search slots (and, in the joint traversal, their Snaps), a module returns an upper bound of the payoff
numerator of every completion, or nothing. Both traversals consult every module after the built-in bounds, with the
same strict pruning rule, and count checks and prunes per module name (`telemetry.joint.modules`,
`telemetry.composition.modules`).

The member-additive module (`memberAdditive`, score of Lives without Gekisou) keeps each pair's power and gain
together. With `T = A0 + sum of the pairs' mean gains`, the mean score is at most `P * T * (1 + eps)`, and for every
`lambda > 0`, `P * T <= (P + lambda * T)^2 / (4 * lambda)` because `(P - lambda * T)^2 >= 0`. The sum
`P + lambda * T = lambda * A0 + sum over pairs of (power + lambda * mean gain)` is additive: a placed member
contributes the largest term over its Pareto-optimal (mean gain, power) Snap choices, and each slot still to fill
the largest term of one character not yet in the team, a distinct character per slot. Snap uniqueness and required
members are relaxed. A golden-section search over `ln(sqrt(lambda))` picks a finite strictly positive scale; scale
selection changes tightness, and arithmetic rounds outward. Taking the minimum of independently certified scales
intersects valid caps. The analogous [carrier split](../crates/ournotes-search/src/search/joint/carrier_split.rs)
keeps its uncoupled cap when power or coefficient is nonpositive, the coefficient is nonfinite, or a finite
positive scale cannot be obtained. The real inequality alone does not authorize an invalid floating-point scale.

### Leaf evaluation

On the deterministic played branch, a complete team in its canonical layout has one value per performance order
(`search/leaf.rs`). Once the Top-K is full, the
team first gets one cap per performance order: the cheap envelope with that order's position gains, then the raw and
fine per-note caps of that order. If the sum of the caps is below the K-th payoff numerator (or equal with a smaller
power), the team is dropped without simulation. Otherwise the order driver shares equivalent simulation prefixes
and visits each node's child groups in descending total-cap order. The
[simulation cutoff](#simulation-cutoff) combines exact payoffs of completed orders with caps of every remaining
order, including the group currently sharing a prefix. After each completed order, the same total decides whether
the remaining orders are needed. A surviving deterministic team obtains exact values for all 120 orders.
The LUCK branch instead maintains complete per-order enclosures and uses the
[interval ranking certificate](#luck-ranking-certificate).

### Shared simulation prefixes

The five-member `OrderedLive` route preserves a state-equivalence invariant. At a node, all represented orders
agree on the fixed positions, the model is at a complete frame boundary, and no member of an open position has
fired a performance event, returned a condition-effect updater or consumed a skill-random value. Moving those open
members and their private state to any represented order must give the same subsequent behavior as its separate
simulation.

Untouched checkers may update their private counters, previous values and caches. Those states move with their
owner. Their only dynamic position predicate is the owner's performance-event check, including recursive instances
inside compound checkers, and it is remapped. Static formation conditions depend on the unchanged member set and
owner. Within a skill phase, life checks query the same timestamp before any applier writes. Repeating that query
against the same life-command log is idempotent, so these cache reads commute. A random draw makes its owner touched
even when the condition ultimately fails, preventing that draw from being shared across an unresolved position.

Relocation updates owner keys, effect-count keys and idle plans, and sorts the effects by their relocated native
keys. The sort is necessary because a position shift can cross a signed wrapping boundary. Private checker and
updater state stays with the same effect; frame-local scratch state is rebuilt before reuse. Global applier
registrations belong only to fixed members, since an open member has returned no updater. Random stream state
clones by value. Score logs share only immutable prefixes, with separate mutable tails for subsequent work.

An announced open event splits before its frame. An unannounced condition action is detected on a clone, and the
driver replays only to that frame's preceding boundary. Each split fixes another member-position pair and
partitions every represented order into exactly one child. Induction over shared frames and these splits gives
each completed order the same native outcome as its separate simulation. Under this invariant each constituent
order's cutoff table applies to the shared settled prefix; their sum bounds the group's remaining payoff.

This lemma concerns the ordinary model construction used by this search. Scripted lottery-probe models require
their separate probability argument, and the lottery-free construction requires its no-LUCK certificate.
Implementation: [order driver and relocation](../crates/ournotes-sim/src/live/full/orders.rs),
[checker/updater relocation](../crates/ournotes-sim/src/live/full/engine.rs).

### Global upper bound

On deterministic routes that track it, `telemetry.proof.globalUpperBound` bounds the best payoff numerator over the whole domain: the larger of the best
payoff found and the bounds of the branches still open. The composition traversal bounds every leader's subtree
before the search and drops a leader's bound when its subtree is done; the joint traversal in descending root-bound
order reads the bound of its remaining root children. The recorded value only decreases. It equals the best payoff
once the search is complete; after a stop it is at most the larger of the best payoff and `upperBound`. Every point
of the incumbent timeline carries the value at that time (`upper`) where available. With several sequential search
parts (Gekisou conversion parts), a running timeline may lack this scalar. The final result can publish it after
completion or an interrupted unwind that has covered every later part's cap.

This scalar concerns the primary value of the best team. Equality with the best incumbent's value alone does not
certify K results, power ties or canonical-ID ties. Those claims require the remaining ranking obligations to close.
The certified LUCK route uses its interval frontier and rank certificates; this deterministic scalar is not its
expectation or ranking certificate.

### Initial decks

`initialDecks` lists up to 100 legal decks of the domain that the search evaluates or safely excludes before its traversal, for
example the best decks of a fast heuristic. They only fill the Top-K earlier, which lets the bounds prune sooner;
their team identities are retained as handled. When the search completes, the ordered result is independent of
these proposals. Work counts, cutoff evolution and results of an incomplete search can depend on them.

With `search-diagnostics`, `search::diagnostics::prefix_upper`, `module_prefix_uppers` and `audit_order_caps` expose
the node bounds, the module bounds and the per-order caps of a team. The harness independently enumerates every team
and checks every prefix against its exact value and power, then compares the entire ordered Top-K (see
[validation](#validation)).

### Deterministic completion theorem

Fix a validated request and its finite legal team domain `D`. Let `N(t)` be a team's exact payoff numerator on
the common mass, `P(t)` its power, and `I(t)` its canonical member/Snap key. Order teams by
`N descending, P descending, I ascending`. The direct member-set route uses its own representative key instead.
The following obligations establish the complete result:

1. **Coverage.** The traversal initially covers every legal identity. Each branching step partitions its legal
   continuations, or covers them with overlap while preserving identity deduplication. An assignment reduction
   accounts for each omitted result by the distinct dominating witnesses proved for that route.
2. **Values.** Every incumbent is a legal, fully evaluated result under the fixed target. An exact cache or
   score-program substitution supplies the same value as its specified evaluator. A partially simulated team
   supplies no incumbent value.
3. **Bounds.** Every discarded subtree has a valid upper bound on all its completions' primary values. A
   secondary power cap covers all completions that can tie the primary cutoff. Any canonical key bound is a
   lower bound under ascending ID order.
4. **Retention.** The Top-K contains distinct result identities, uses the declared entire key, and keeps the
   first K among the completely evaluated candidates. Representative replacement preserves or improves a key.
5. **Closure.** Every continuation is either handled or certified unable to enter Top-K. Exhaustion, rather
   than a budget or heuristic stopping rule, supplies this last obligation.

Here is the pruning proof with ties included. Suppose the full incumbent set has last key `(T, P_K, I_K)`.
For node `v`, let `U(v)` cap `N(t)` for each completion `t`. If `U(v) < T`, the K distinct incumbents all
precede every completion. If `U(v) = T` and the relevant power cap is below `P_K`, each completion either loses
on the primary value or loses on power. If both caps equal the cutoff and `I_min(v) > I_K`, each completion
also loses on the canonical key. A non-strict key test additionally accounts for the equal-key identity that has
already been handled. Without a key certificate, equality of the numeric caps leaves the branch open.

The K-th full key can only improve. Therefore a rejection supported by K distinct incumbents stays valid even
when those witnesses later leave Top-K: replacements are better witnesses. At closure, every omitted identity
has K better retained-or-replaced witnesses. It cannot belong to the mathematical first K. Every returned value
is correct by the value obligation, and Top-K retention gives the right order. If fewer than K identities
exist, coverage and closure account for all of them.

This proof also explains which shortcuts need separate lemmas. Member-set representative maximization needs
its score/power monotonicity and class-equivalence assumptions. A team search retains distinct leader and Snap
bindings and uses the per-layout K-witness argument. A compiled coefficient, a heuristic proposal or a scalar
best-value certificate supplies only its own local obligation.

### Partial leaves, caches and search parts

For a deterministic team, after a set `E` of orders has completed, its numerator is bounded by

    sum_{pi in E} exact_payoff(pi) + sum_{pi not in E} order_cap(pi).

During the current order, its second term may be replaced by a valid simulation-cutoff cap. Replacing a cap by
that order's exact value preserves coverage. If this total loses against the incumbent key, the whole team is
safely handled; otherwise all 120 orders must complete before offering its exact mean. A cooperative budget
stop can preserve already completed teams, but cannot promote this partial sum to an exact expectation.

The same invariant applies when an identity is skipped because it was already handled. Its previous handling
must be a complete evaluation or a valid exclusion against an earlier, no-better cutoff. A cache eviction may
cause repeated work; it does not change the mathematical domain or justify a new exclusion.

`TeamScores` keys a complete lottery-free law by unordered member/Snap pairs and exact power. It relabels all
120 score/life values bijectively into the requested slots, then recomputes that team's payoff, including event
bonus and final-life conditions. The request-local program cache instead keys full performer values and member
identities; the request fixes chart, play, clocks, scenario and all other simulator parameters except power. Each stored
completed order supplies an exact program evaluated at the candidate's actual power and its final life, followed
by a fresh payoff calculation. Missing orders remain incomplete. See [score programs](score-programs.md) for this
admission and evaluation contract; LUCK equality additionally includes the full payoff mapping.

For disjoint search parts, such as conversion regimes, their union must equal the original domain. They share
one incumbent order, and each part's bound must cover that part under its own constraints. Completion requires
every part to close. An incomplete whole-domain bound must cover the active part, unfinished siblings, remaining
root leaders and all later parts. Where the implementation cannot retain that coverage as one scalar, the
scalar remains unavailable.

Theorem obligations concern the actual arithmetic and domain checks used by each route. The accompanying
finite-oracle tests check instances of these obligations; they do not replace the universal arguments.

## Prefix bounds

Bounds are evaluated from cheap to expensive. Each row names a point where the search relaxes the problem, the
sound envelope it uses there and the condition that keeps the envelope sound.

| Relaxation point | Sound envelope | Necessary boundary |
|---|---|---|
| Skill windows and weighted notes | Compile weighted interval sums per performance position; node bounds read the mean over the five positions; intersect a certified own-event trigger with its mission gate. | The trigger and release behavior must satisfy the timing certificate. |
| Member/Snap power and skill tradeoff | Bound a weighted sum of power and coefficient for the same pair, then apply `P A <= (P + r A)^2 / (4r)`, taking the minimum over positive `r`. | Preserve leader context and distinct-character maxima; optional reuse of remaining Snaps only enlarges the domain. Round outward. |
| Unique Snap capacity | Bound member-only contributions plus at most one positive increment per available Snap; intersect with the character-wise bound. | Snap increments must be bounded over every possible remaining member and position, including negative pair differences. |
| Mission ranges | Charge JUST, COMBO, LUCK and rank bonuses only to the notes/ranges that can receive them. | Confirmation replay and boundary frames follow the scorer; a display Fever ratio is insufficient. |
| COMBO saturation | Bound reachable counts using selected combo windows and take the maximum table value over those counts. | Never sum individually measured aptitude deltas: saturation and conversion interactions matter. |
| PT tiers | Turn a score cap into the maximum reward among reachable tiers, then bound event bonus. | Apply per order at a complete team and through the concave majorant at nodes; allow nonmonotone reward tables; retain multiplayer total-score rules separately. |
| Exclusive final judgement | Take the maximum of each reachable judgement times its own active judgement-specific bonus, not a sum of Perfect and JUST bonuses on the same note. | Keep conversion reach and budgets conservative; one note can realize only one final judgement. |
| Judgement conversion and life | Candidate conversion masks/budgets and provable life ceilings refine per-note bounds. | Do not infer temporal judgement allocation from a whole-chart Great rate. |
| Conditional dominance | Compare complete effect/context vectors within the same legality class, preserving canonical ties and Top-K alternatives. | Song-level efficiency dominance is not universal card or song dominance. |

Measured chart statistics, such as a fitted base score or per-position weights at one power, are not upper
bounds and never prune the integer scorer.

The bounds in use:

- Certified own-event Gekisou factor windows use the performer's event frames and mission gate; rows with release
  checkers retain the general timing envelope.
- Cheap prefix bounds intersect distinct-character maxima with unique-Snap capacity bounds for power, gain and
  event bonus.
- Optional correlated envelopes retain the power/skill tradeoff of each pair.
- At surviving leaves, per-note envelopes use the candidate's conversion reach, budgets and combo windows, and
  take the maximum over exclusive judgements.
- PT transforms the per-order cheap and fine score caps of a complete team through all reachable reward tiers,
  and node bounds through the concave majorant of the tiers. It does not assume that a higher grade pays more.
  A declared multiplayer score policy with nonnegative rank confirmations permits local-score preimages of room
  thresholds; without that certificate, the route keeps the unrestricted reward ceiling.

**Network rank snapshots.** Network rank bonuses read retained controller score snapshots. A timed factor can
still affect notes in its finish score frame before the end command is filed, so the snapshot factor envelope
includes that whole frame, clamped to the declared score clock. The cheap and per-order fine caps use these
windows, and cutoff tables keep the full cap while ranking uses the retained snapshots.

Correlation preparation probes at most 16 leader/pair prefixes, independently of traversal order. Unless the
correlated bound tightens the cheap expected upper bound there by more than 3%, the solver skips that optional
bound and keeps the ordinary complete traversal. The probe is a cost policy, not a proof, and never removes
candidates; diagnostics still audit the correlated bound when the policy skips it.

The coefficient, correlated and node bound checks read position-mean gains; the per-order caps of a complete team
read each order's own positions and are summed with checked integer arithmetic. All are compared to the canonical
K-th incumbent with strict payoff/power rejection. Equality of both keys retains possible ID ties.

**Judgement-frame triggers.** Judgement-match and count conditions (1000/1010/1020/1030/1040) require a
judgement in the current frame. Possible triggers are therefore restricted to frames with a raw judgement,
whatever the targets, conversions or count thresholds; range-completion and finish frames without a judgement
cannot trigger them. Targets, conversions and thresholds themselves stay optimistic.

**Counted-judgement trigger times.** Positive counted-judgement triggers also require enough eligible
judgements to have occurred while their mission gate could be open. A whole-pool transitive conversion closure
supplies optimistic eligibility; duplicate targets retain their possible multiple increments. Dropping resets,
consecutiveness and prior firings only increases the count. This gives a necessary first-trigger time and can
prove a row unreachable when the entire gated stream is below its threshold. It is a state-reachability bound,
not an estimate from an average Great/Perfect rate.

**Compound trigger times.** The simulation drops the conditions of type 0 from a trigger group, combines the rest
of each set with an AND and two or more non-empty sets with an OR. Neither combination reports a trigger time, so
such a row triggers at the time of the frame that checks it, and its Gekisou factor windows start there; only a
group of one set with one remaining condition can carry an earlier override time.

**Sustained windows.** A sustained Gekisou effect runs one execution at a time, each inside the span of its start
frame (its earliest trigger time to the first later frame whose trigger fails), so its factor windows are the
components of the union of those spans, each with the executions of its own start frames.
For timed Gekisou and Rush rows with a release checker, nominal activation time alone supplies no upper end:
the checker can bypass the first elapsed-time test. Their general windows retain the effect unless a separately
proved release, such as completion of the matching range, bounds its end. Conversion registration windows use the
same lifecycle restriction.

**Candidate suffixes.** Candidate-suffix envelopes precompute component maxima after every position in the
ordered member/Snap choice list. For a surviving prefix, the next slot is bounded by its suffix and the other
remaining slots by a character/resource relaxation. Their overlaps only enlarge the legal completion set. A
suffix whose expected payoff is strictly below the K-th incumbent (or equal with lower power) ends that node's
entire remaining loop. The `tailChoicesSkipped` counter counts pair-loop entries, including illegal pairs, not
distinct complete decks.

**Next pair.** Each surviving prefix also prepares the residual once, excluding the next slot. A constant-cost
per-pair check combines that residual with the proposed pair's actual power/gain/bonus components before
recursing. It allows the residual to reuse that pair's character/Snap, so it remains an upper bound. Only
survivors pay for a fresh, stronger resource-aware prefix scan.

**Table form.** The cheap prefix relaxation is compiled into tables. A prefix fixes at most four characters and
four Snaps, so keeping the five best entries with distinct keys (Snap for per-character maxima, character for
per-Snap increments) returns the exact maximum over the free keys. The values and floating operations equal the
member/Snap scan, so the relaxation returns identical numbers.

## PT bounds

### Membership regimes

PT under joint traversal first evaluates legal teams from the maximum-event-bonus regime exactly, stopping after
K results. This is only an incumbent search: if that regime contains fewer
than K decks, the original domain still receives a full search and no membership exclusion is made.

For each member, a per-order cap combines that member's event bonus, the four largest other-character bonus
maxima, the five largest unique Snap bonuses, and the largest reachable reward multiplier. Pairing, score-tier
feasibility, required-card and leader restrictions are relaxed, so the cap can only be too high. A member is
removed from a private search view only when `cap * 120` is strictly below the already evaluated K-th payoff
numerator. Equality is retained regardless of power or IDs. Every excluded completion is
thereby certified below the incumbent in the original domain. Recompiling score/resource bounds for that view
also removes inactive programs from the pool-wide envelopes. The original built problem and reported proof
domain are unchanged. If compilation of the optional view fails, the original complete traversal remains
available.

### Residual power assignment

When exactly `5-depth` characters remain available, the residual power bound can be solved as a bipartite
assignment. A character/Snap edge takes the largest power over that character's allowed members, retaining the
correlation between member base power and Snap scaling. Subtract each row's best no-Snap power, add it outside
the matching, and pad to five rows with zero-weight dummies. Optional empty Snap choices remain legal; used Snaps
are removed. The Hungarian matcher yields an upper bound on every completion's power. Its tie convention is
immaterial because this stage consumes only the numeric optimum, never a representative deck. The bound is
checked only after payoff equality with the K-th incumbent; power equality retains all canonical alternatives.

### Bonus-conditioned score envelopes

For a fixed leader-first prefix, group each remaining character's member/Snap choices by their exact additive
event bonus. Within a bonus group retain maxima of power, skill coefficient, and each of the three weighted sums
`P + r*G`. A cardinality DP chooses the required number of distinct remaining characters and convolves their
bonuses. Every DP cell therefore refers to one exact bonus sum. Its component maxima may come from different
completions within that cell, which only enlarges the upper bound; a high-bonus cell cannot borrow the power or
skill of a lower-bonus completion.

Remaining Snap reuse and required-card obligations are relaxed. The assigned prefix retains its exact pairs; gains
are position means. In each final cell intersect the product envelope with the three weighted-sum envelopes, then
apply the concave majorant of the reachable reward tiers to that cell's event bonus. A team has one event bonus in
every performance order, so the mean cap is taken within each bonus cell before maximizing over cells.

The same cells also cap power for ties. Every completion with an exact event bonus has at most that cell's
expected payoff and residual power, so a completion whose payoff reaches the overall cap lies in a cell whose
total is the cap. A branch whose payoff cap equals the K-th payoff numerator survives only when the largest power
among those cells reaches the K-th result's power.

Residual tables depend on leader profile and excluded characters. They are
reused only within one compiled bound/domain; used Snaps need not enter the key because residual Snap reuse is
explicit. Limits on states, convolution work and retained cells disable this optional bound rather than dropping
DP states. All numeric equality cases retain the original canonical ranking rules. This layer supplements the
unique-resource and suffix bounds; it does not replace the exact leaf evaluator.

## Member compositions, Snap pairings and power frontiers

For Live with Gekisou off, branch on the leader and an unordered set of four other members. Increasing indexes in a
fixed heuristic order enumerate each set exactly once; that order never removes a member. The composition envelope
allows every member any Snap and reads position-mean gains, so it does not depend on the layout. It combines
distinct-character power, gain, bonus and weighted-sum maxima. These statistics are compiled by leader profile and
leader position, with
a direct calculation fallback when the optional table exceeds its capacity.

Every surviving composition is searched once, in one layout: the value of a team does not depend on the layout of
its non-leader pairs, and the leaf evaluates the canonical layout. A Snap search over that layout enforces resource
uniqueness and bounds remaining power with bipartite assignment. Numeric ties also use a canonical lower key: the
canonical layout of the composition with the Snaps assigned so far and None in each unassigned Snap slot. This key
is no greater than any legal completion because None is always allowed and sorts before every Snap ID.

When at most two Snap slots remain, the numeric assignment is solved directly. At most one real Snap can conflict
with the other slot, so each row's two best choices, including optional None, contain an optimal assignment. Zero
remaining slots need no assignment call. The general matcher remains the value reference; the fast residual tie
convention is not used as a Top-K certificate.

For PT and the other bounded terminal objectives admitted by this path, when the best-power proposal attains the
composition's primary-payoff cap, a small resource DP separately
solves the **power-only** Top-K bindings of the composition's layout. After scanning a Snap, future resources depend
only on the occupied-slot mask. Keeping K partial bindings per mask is therefore exact for power and None-first
identity ordering. The Snap increments in the validated power table are exact; all member/leader-only terms are
constant across bindings, including a complex leader's fixed-member contribution. The actual power of the last
proposal is read through the ordinary evaluator's power path before using it as the remaining-power cap.

Every proposed binding is completely evaluated, resolved from existing evaluation state, or safely excluded by
an admissible leaf cap. All unexamined bindings rank after the
last power proposal. The composition may close only when its primary-payoff cap, the remaining-power cap and canonical
lower key cannot beat the updated global K-th result. An equal cutoff key is already handled. If fewer than K legal
bindings exist, the DP returned all of them and the composition is exhausted. Otherwise a failed closure
proof resumes the complete Snap traversal, excluding only the identities already safely handled in this layout. Higher
power is **not** assumed to imply higher PT, and a nonmonotone reward table does not justify representative-only
pruning. The cap-attainment condition is a cost policy only: skipping the frontier leaves complete Snap traversal
intact. Compositions without Snaps have one binding and close immediately after its exact evaluation.

Live obtains its incumbents from this integrated assignment stage. Gekisou keeps joint member/Snap traversal with
the PT membership regime above. Both schedules optimize over the same teams and share the fixed evaluator.

## Class and resource envelopes as explicit harness schedules

For Gekisou score objectives, two diagnostic schedules separate two decisions: an upper-bound effect class in each slot, then the
unique Snap assignment inside those classes. A class is an identity in `JointFineBounds` metadata only. Every
allowed completion remains represented. A constrained assignment proves infeasibility or maximum power; at a
complete class vector the per-note fine bound, summed over the 120 performance orders, applies to every binding
because its inputs are exactly members, classes, positions and an upper bound on power (`JointFineBounds::upper`
projects each choice through `class_of`). Surviving bindings reach the ordinary leaf path, which either evaluates
them or proves their exclusion. The [class-envelope proof](#class-envelopes) uses direct per-entry admissibility
at the supplied power cap; it requires neither representative-only score reuse nor monotonicity of actual score
or of the entire relaxed fine-envelope function.

The resource-correlated cap addresses a different relaxation. For a fixed member composition, write the mean score
envelope as `P*A`, where `A = a0 + sum(mean gain)`. For every positive `r`, `P*A <= (P+r*A)^2/(4*r)`. Each pair's
`P+r*gain` is rounded up to an integer before subtracting a row shift. An exact constrained assignment then
maximizes the sum with each Snap used at most once; None remains independently available only in allowed slots.
Restoring the row shifts and constant `r*a0` gives an upper bound on `P+r*A`. Floating operations round outward
and retain the scorer margin. The minimum over three positive scales intersects the other caps; unavailable
numeric/resource limits skip this optional cap entirely.

Incumbent preparation evaluates maximum-power and weighted power/gain assignments of a completed member
composition before deep traversal. These rounded surrogate objectives only propose legal
candidates. Their real values come from the fixed evaluator and never justify exclusion. Local identity sets prevent
repeat evaluations even when the global cache is disabled.

The harness exposes `production`, `classes` and `classesWithResource` schedules, audits every class-prefix and
within-class binding-prefix against each oracle team, and compares the full ordered Top-K. Player
requests always use the production schedule.

Bound invariance across a class does not prove that native score or PT is monotone in power, and a power
coefficient fit is not an exact evaluator: score rank conditions, signed undo/replay, fixed range bonuses,
integer wrapping and float factor drift all intervene. Exact per-deck programs and their checked monotonicity
certificates are described in [score programs](score-programs.md).

## Proofs for prefix and assignment bounds

These arguments apply to one compiled bound and its declared candidate domain. Let `N(D) = 120 U(D)` be a team's
scaled expected payoff, `P(D)` its power, and `C(N)` the legal completions of a node. A payoff cap `U(N)`
must satisfy `N(D) <= U(N)` for every `D` in `C(N)`. For LUCK, an orderwise cap also covers every admitted lottery outcome before averaging. A power cap used after payoff equality need only cover the
completions that can reach that payoff cap; a general power cap covers all completions.

### Position means and component maxima

For a team with member/Snap pairs `i = 0..4`, write `g_i(k)` for its compiled gain at performance position `k`.
The underlying score-envelope obligation is, for every order `pi`,

```text
S(D, pi) <= (1 + eps) P(D) min(G, a0 + sum_i g_i(position_pi(i))).
```

Here power, `a0`, `G`, gains and the margin are nonnegative; the compiled domain must also establish that the
envelope covers the scorer's integer and floating operations. Every slot occupies every performance position in
exactly 24 of the 120 permutations. Therefore, with `mean_i = sum_k g_i(k) / 5`,

```text
E_pi[S(D, pi)] <= (1 + eps) P(D) min(G, a0 + sum_i mean_i).
```

This follows from linearity of expectation and Jensen's inequality for the concave, nondecreasing function
`x -> min(G, x)`. The five positions need not be independent. The stored means round upward, so replacing a real
mean with its stored value preserves the inequality. A mean cap multiplied by 120 bounds the score numerator;
the single position-mean pseudo-order is a representation of that cap, not one simulated performance order.

For any additive component, choosing the `r` largest remaining distinct-character maxima bounds every choice of
`r` remaining characters. Component maxima for power, gain and event bonus may come from different completions:
the resulting rectangle contains every completion. Products require nonnegative components; the joint compiler
proves nonnegative slot power, bounds total power below `i32::MAX`, and rejects negative or nonfinite gains.

Implementation: [means and order enumeration](../crates/ournotes-search/src/search/uniform.rs),
[joint compilation and components](../crates/ournotes-search/src/search/joint.rs).

### Unique-resource relaxation

For one component `x`, decompose a member/Snap choice at position `k` as

```text
x(m, s, k) = x(m, None, k) + delta(m, s, k),
Delta(s) = max(0, max over eligible remaining members and positions of delta(m, s, k)).
```

Every completion with `r` free slots uses at most `r` distinct real Snaps. Its component sum is consequently at
most the sum of the `r` largest free-character no-Snap maxima plus the `r` largest available `Delta(s)` values.
Taking the minimum of this bound and the distinct-character bound remains sound. The two maxima may relax
different constraints; both inequalities must hold for the same completion before they are intersected.

The increment is formed from the same member's paired and unpaired values. A negative increment is covered by
zero because Snap use is optional. This reasoning does not permit subtracting independently maximized paired
and unpaired values: their maximizing members may differ.

Candidate-suffix bounds apply the same inclusion argument. Every remaining child uses a next pair from its
suffix; the other slots belong to the residual relaxation. Permitting that residual to reuse the proposed pair's
character or Snap enlarges the completion set. A bound below the cutoff therefore closes the whole suffix.

Implementation: `scan_free`, `tail_upper` and `pair_upper` in
[joint bounds](../crates/ournotes-search/src/search/joint.rs).

### Weighted sums and quantized assignment

Put `A(D) = a0 + sum_i mean_i`. For every positive scale `r`,

```text
4 r P(D) A(D) <= (P(D) + r A(D))^2,
P(D) + r A(D) = r a0 + sum_i (p_i + r mean_i).
```

The first inequality is equivalent to `(P(D) - r A(D))^2 >= 0`. If an additive relaxation gives
`W >= P(D) + r A(D) >= 0`, squaring preserves the inequality, and
`(1 + eps) W^2 / (4r)` is a score cap. Each pair contributes its own power and gain before maximization.
Distinct-character maxima may relax Snap uniqueness; resource assignment may instead retain unique Snaps.
Every positive scale gives an independent valid cap. A grid or finite optimization procedure may choose any
evaluated scales and take their minimum; proving that it finds the best scale is unnecessary for admissibility.

The resource version first rounds each absolute edge upward:

```text
q(i, s) = ceil(up(p(i, s) + r g(i, s))),
shift(i) = q(i, None),
weight(i, s) = q(i, s) - shift(i).
```

The subtraction is exact integer arithmetic. For every allowed binding, adding all row shifts back to its
assignment weight recovers the sum of its quantized absolute edges. This remains true when None is forbidden
in some rows: the shift is an algebraic constant, not a choice the binding must make. Maximizing with unique
real Snap columns and independently available allowed None columns therefore bounds every legal weighted sum.

For this optional quantized matcher, each absolute edge lies in `[0, 10^15]` and there are at most 4096 real
Snap columns. Five absolute edges sum to at most `5 * 10^15 < 2^53`, so that integer sum converts exactly to
binary64. Row shifts and assignment arithmetic also stay within their integer ranges. The weighted square,
division and margin operations round outward. These statements concern the quantized weighted expression;
the ordinary score-product caps additionally rely on the scorer-envelope margin theorem.

An unavailable numeric or storage range disables the optional cap. It does not discard an edge and then use
the reduced assignment as an upper bound. Surrogate assignments used to obtain incumbents have a separate role:
they propose legal teams whose actual payoff still goes through the leaf evaluator.

Implementation: [resource product](../crates/ournotes-search/src/search/joint/resource.rs),
[prefix resource tables](../crates/ournotes-search/src/search/joint/prefix_resource.rs),
[member-additive scales](../crates/ournotes-search/src/search/joint/lambda.rs).

### Exact maxima from bounded tables

An unfinished prefix occupies at most four character keys and four real Snap keys. A table retaining its five
largest values with distinct excluded keys therefore retains a maximum among all free keys: at least one of
those five survives, and every omitted value is no larger. If fewer than five keys exist, the table retains all
of them. None is a choice that is never excluded by another slot's None. The stored table and the direct scan
use the same values and accumulation order, which is required for their stated numeric equality.

A related exchange argument proves the small residual character solver. With `k <= 4` mandatory rows, retain
each row's `k` largest distinct-character edges. If an assignment uses an omitted edge, at most `k - 1` retained
characters are occupied by other rows. Replacing that edge with a free retained edge preserves feasibility and
cannot lower the value. Repeating this replacement produces an optimum using retained edges only.

With at most two free Snap slots, the same argument retains two choices per row, treating None as a shareable
choice. At most one real Snap is blocked by the other row. These arguments prove numeric optimum values; their
tie choices do not certify a physical Top-K representative.

Implementation: [relaxation tables](../crates/ournotes-search/src/search/joint/relax_tables.rs),
[residual character solver](../crates/ournotes-search/src/search/joint/prefix_character.rs),
[small Snap assignment](../crates/ournotes-search/src/search/matching.rs).

### Residual assignment lemma

Fix the leader-first prefix. When every remaining character must be used, let `M_c` be one character's allowed
members, and let `l_m` be the compiled leader contribution, which may be an upper bound for a complex leader.
Define `h(c, None) = max_{m in M_c}(a_m + l_m)` and
`h(c, s) = max_{m in M_c}(a_m + l_m + w(m, s))`.

Every completion maps to a character/Snap assignment whose edges dominate its actual member contributions.
Subtract `h(c, None)` from its real-Snap edges and restore these row constants after matching. Used Snaps are
excluded; zero-weight dummy rows can choose None, so padding to five rows leaves the residual optimum unchanged.
Required-member restrictions are relaxed. A complex leader can make the resulting number optimistic, which is
sufficient for a cap. The matcher tie convention does not matter because only its numeric optimum is consumed.

The current optimization runs at prefix depths 1 through 3 and is consulted after payoff equality. A strictly
smaller power cap then proves inferiority; equal power retains the canonical ID alternatives.
Implementation: `assignment_power_upper` in [joint bounds](../crates/ournotes-search/src/search/joint.rs).

### PT from a score cap

Let `h(s)` be the actual nonnegative reward multiplier at score `s`, possibly decreasing at higher ranks, and
let `m(s) = max_{t <= s} h(t)`. For a valid per-order score cap `s_max`, take a concave, nondecreasing majorant
`H` of the prefix-maximum reward steps reachable at scores at most `s_max`. For one team's constant event bonus
`b >= 0` and a valid mean-score cap `s_mean`,

```text
Y_pi = floor((10000 + b) h(S_pi) / 10000),
E[Y_pi] <= (10000 + b) E[H(S_pi)] / 10000
        <= (10000 + b) H(E[S_pi]) / 10000
        <= (10000 + b) H(s_mean) / 10000.
```

The final bound rounds upward and intersects the direct maximum reachable per-order payoff. Applying the raw
reward function to mean score would be unsound: 60 orders at score 0 and 60 at score 2 have mean score 1; a
reward of 100 only at score 2 or above gives mean reward 50 while the reward at the mean is zero.

The nonnegative-score hull requires that score domain. With a declared multiplayer score policy and nonnegative
rank confirmations, the compiler uses local-score preimages of the room-score thresholds. A multiplayer route
without that policy, or one admitting negative network rank bonuses, keeps the unrestricted reward ceiling.
Integer reward products must remain in the validated nonwrapping domain.

Implementation: `PointBound` in [joint bounds](../crates/ournotes-search/src/search/joint.rs),
[concave majorant and integer interpolation](../crates/ournotes-search/src/search/uniform.rs).

### Exact-bonus cells

After processing `j` character groups, the cardinality DP cell `(k, b)` covers every choice of `k` distinct
processed characters with exact additive event bonus `b`. Initialization at `(0, 0)`, descending-cardinality
convolution with one new character, and component-wise maximization establish this invariant by induction.
Power, mean gain and each weighted sum may maximize at different choices within the cell; they never cross
between bonus values. Residual Snap reuse, including a Snap held by the prefix, and required-card obligations
are relaxed. The assigned prefix contributes its fixed pairs.

The team's event bonus is identical in every performance order. Thus any order-mass contributions to a bonus
cell's cap are summed before maximizing over cells. Let `U_b` and `P_b` be a cell's payoff and power caps and
`U = max_b U_b`. If a team reaches `U`, its bonus cell must have `U_b = U`; cells strictly below `U` cannot tie.
Consequently `max_{b: U_b = U} P_b` is a valid power cap for the teams reaching `U`, and can reject a node after
`U` equals the K-th payoff. A general power cap may be intersected with it.

The row cache belongs to one compiled bound/domain. Its leader profile, excluded-character set and remaining
positions determine the residual rows; used Snaps need no key because their reuse is relaxed. State, convolution
or storage limits abandon the optional bound as a whole. Implementation:
[bonus-conditioned DP](../crates/ournotes-search/src/search/joint/bonus.rs).

### Canonical power frontiers and closure

For a fixed leader and complete member composition, true power is one member/leader constant plus the sum of
exact Snap increments. This is still true for a complex leader: its exact contribution depends on the fixed
members and song, not on the Snap binding. The composition's nonleader pairs are first put in public member-ID
order, and Snap columns are public-ID sorted; None precedes every real Snap in the team identity order.

Process Snaps one at a time. At a fixed processed-Snap index and occupied-slot mask, all partial bindings have
the same available future resources and free slots. Giving two such bindings the same future extension preserves
their power difference and lexicographic order: the extension fills the same previously empty coordinates.
For any partial binding ranked below K, each extension has K distinct extensions of better retained bindings.
It therefore cannot enter the final power Top-K. Keeping K per mask is exact; after the final Snap, merge all
masks and retain the first K. This also covers unused Snaps, empty slots, equal powers and K exceeding the
number of legal bindings.

Every proposed binding is submitted to the leaf path: it is exactly evaluated, resolved from existing evaluation
state, or certified unable to enter Top-K by a valid cap. A candidate stopped by a budget is not marked handled.
The incumbent threshold never weakens, so an identity already excluded by a cap remains safely excluded.

Let `L` be the last power-ranked proposal and `U` the composition's primary-payoff cap. Every unexamined binding
has lower power than `L`, or equal power with a larger canonical key. Read `P(L)` through the regular power
evaluator. The unexamined bindings cannot enter Top-K when `U` is below the cutoff, or when `U` equals it and
`P(L)` is below the cutoff power. If both numeric keys are equal, `key(L) >= key(cutoff)` also closes them;
the equal-key proposal itself was already handled. Fewer than K proposals means every binding was handled.

Otherwise complete Snap traversal resumes, omitting only safely handled identities. Attaining the composition
cap controls whether the frontier is attempted; it is not a monotonicity assumption. Higher power need not give
higher PT. The same frontier proof applies to the other bounded terminal objectives admitted by this path.
Implementation: [Top-K assignment DP](../crates/ournotes-search/src/search/matching.rs),
[layout and exact increments](../crates/ournotes-search/src/search/joint/composition.rs),
[frontier closure and traversal](../crates/ournotes-search/src/search/composition.rs).

### Class envelopes

The diagnostic class schedules currently require a Gekisou score objective. Grouping Snap choices partitions
the choices; all individual legal bindings remain represented. At a complete class vector, the fine-bound
inputs are fixed members, classes and positions together with an upper bound on power. `JointFineBounds::upper`
projects every choice through `class_of`, so those metadata inputs are identical across the class bindings.

Admissibility at the supplied power cap follows from the per-entry envelope construction: for actual power
`p <= P`, each possible final judgement's bound at `P` covers that entry at `p`. Every actual budgeted conversion
is charged to an eligible budget row; maximizing each row's allocation independently only enlarges its options.
Summing these bounds covers every binding and order. This argument requires neither native-score monotonicity
nor monotonicity of the complete fine-envelope function in power. Independent conversion-budget overcounts
alone would not prove such monotonicity.

Constrained assignment supplies infeasibility or a power cap; the fine cap supplies a payoff cap. Every surviving
binding still reaches the ordinary leaf path. A bound-metadata class therefore supplies no representative-only
evaluation certificate. Implementation: [class bounds](../crates/ournotes-search/src/search/joint/classes.rs),
[class and binding traversal](../crates/ournotes-search/src/search/composition/classes.rs),
[fine-bound projection](../crates/ournotes-search/src/search/snaps/fine_view.rs).

## Gekisou envelopes

### Conversion reach and conversion regimes

Cumulative score-ups that count JUST judgements (effect 2001) depend on which entries can become JUST. The
whole-pool judgement closure contains every converting Snap, so it would count every entry as a possible JUST,
while in a deck without a conversion long-note heads and other non-JUST entries never count.

Count conditions therefore read a per-entry reach: each stream entry's raw judgement, closed transitively under
every allowed conversion whose registration window contains that entry's processing frame. Outside every window
an entry keeps its raw judgement. The windows come from the conversion analysis (which itself uses the
whole-pool closure), so they only over-approximate. Until the windows are known, the whole-pool closure remains
the fallback.

The reach depends on which converting Snaps a deck may hold, so Gekisou score partitions the physical domain:

- no converting Snap: an envelope compiled for that sub-domain;
- exactly one, `c` in physical slot `s`: the sub-domain keeps `c` as its only converting Snap, slot `s` must take
  `c` and every other slot excludes it;
- two or more: split by the first two slots in search order that hold converting Snaps; both must take converting
  Snaps and the other earlier slots exclude them.

The parts are disjoint and cover the domain (a team's converting Snaps do not depend on its layout); they share one
Top-K, so canonical order and tie handling are unchanged. A forced slot is charged in the cheap relaxation only with its allowed choices; excluded masks filter
enumeration only. All other bounds remain valid unforced relaxations. A failed part compile falls back to one
pool-wide search.

### COMBO

**Integer-stack domain.** Combo-count and combo-table refinements require an exact-integer certificate for
effect-12000 bonuses. The compiler bounds a member's own rows plus at most one allowed Snap row list, and then
the sum `B` for any admitted five-slot deck and requires `B < 2^24`. Each stack prefix, including the initial
increment of 1 and paired removals, stays in `[1, 1+B]` with `1+B <= 2^24`; those integer operations are exact in
binary32. This is a prerequisite for using integer window sums as
native combo counts in threshold triggers, ramps and fine score tables. When that stack certificate is unavailable
on a COMBO chart, the shared bound compiler declines the bound and the recommendation route retains exhaustive
evaluation. The score evaluator continues to use its native binary32 operations.

This guard is needed at every count-dependent bound, not only at trigger-time gates: rounding a bonus upward by
one can cross a combo-table threshold. A multiplicative score margin cannot justify the wrong side of such a
discontinuous threshold.

**Combo-count ramps.** A cumulative note score-up that counts the playing range's combo (7001) updates its count
in the skill phase of every frame it runs in, from the playing range's combo after the judgements of the earlier
frames, and files a changed factor at the frame's time; its first factor can be backdated to its trigger time. An
execution ends no later than the time of the frame that processes its end, so an entry reading the factor of an
execution started in frame `f` reads the count of frame `max(f, F)`, `F` the last frame with a time up to the
entry's chart time. Per factor window and entry, the bounds read the largest such count over the window's start
frames whose earliest trigger time is at most the entry's chart time. A frame's count is bounded by the prefix sum
of its playing range's largest per-entry increments over the shortest chart-order prefix holding the entries
judged before the frame; within a run of frames with one playing range it does not decrease. A frame whose playing
range is not an ordered combo range keeps the flat factor. The candidate cap reads the increments of the
candidate's own combo bonus windows, the gain tables those of their carrier level or keyed envelope.

**Carrier split.** A completion of a prefix whose placed carriers bring the window lists `S` has exactly the carriers
of `S` and of some multiset `T` of lists, one per carrier among its slots to fill. Its cheap bound is at most the one
of a complete deck under the envelope keyed by `S` and `T` with no carrier to come, every slot reading its gains under
that envelope. For every `T` of at most the slots to fill, each list of `T` adds its best powers and best gains over
distinct characters outside the prefix with no Snap or a Snap outside the prefix, and the slots without a carrier the
table relaxation of the pairs that are no carrier, with gains under the same envelope; the node bound is the largest
over `T` (characters and Snaps may repeat between the lists and the other slots, which only enlarges the completion
set). Every gain is the mean over the five positions, so each table holds one value per pair. A `T` whose bound is
above the K-th payoff is also bounded with power and gain coupled: for any weight `λ > 0`,
`P·(A0 + G) <= (λ·P + (A0 + G)/λ)^2 / 4`, and the slots to fill relax `λ·power + gain/λ` as one value per pair (per
list for the carriers, by the same table relaxation for the other slots), so a pair's power and gain come from the
same pair. `λ` is the step of a geometric grid (ratio 1.08) nearest `sqrt((A0 + G)/P)` at the uncoupled terms; the
bound of `T` is the smaller of the two. The check stops at the first `T` still above the K-th payoff. Below the leader
the slots to fill take candidates in ascending choice order, so a node's completions use the choices from its start
on, and in a node's choice loop every child from an offset on, with its completions, uses the choices from that offset
on. The tables are compiled for the pairs from each of a few suffix starts (0, 8, then about half again each time); a
node reads the latest start at most its own, and every 16 offsets the loop bounds all the children left by the latest
start at most the offset and stops when that bound is below the K-th payoff.

**Gated combo bonuses.** A combo bonus window whose trigger is a sole positive combo-count condition on its own
range counts at an entry only once the range's combo can reach the threshold by the entry's chart time. Its start
needs the threshold counted from judgements processed before that frame; judged in chart order, those are at
chart times up to the trigger time, and the bonus reaches the judgements from the trigger time on. The bound
counts every earlier entry of the range with its largest possible increment and lets each entry at the same
chart time add at most one step, the increment read with every other running window included; a gate opened this
way only adds bonus. The window's own bonus stays out of its step: an entry counts the bonus only once the first
of the window's executions has started, at a trigger time no later than the entry's, and at the entry's own chart
time that first start reads judgements none of those executions reached. A window without such a gate, or whose
gate names another range, always counts.

**Threshold trigger times.** A sole positive COMBO threshold trigger (7005) gains a necessary trigger time from
prior-frame Good-through-Just processing and the largest exact integer effect-12000 increment of any admitted
five-member deck (conditions, character uniqueness and Snap uniqueness relaxed; a member has its own rows plus at
most one allowed Snap row list). Same-frame skill phases cannot use increments before the controller recount.
An unsafe controller or integer-stack domain declines the shared bound on a COMBO chart. Within the certified
domain, an unavailable optional trigger-time table keeps the broader timing envelope; compound trigger groups
trigger at their frame's time.

### LUCK

A LUCK mission decides its lottery with random numbers, so a LUCK chart's outcome is a distribution over draws
rather than one integer payoff per performance order. Its declared value is

    U(t) = (1/120) * sum_{pi in S5} E_{h ~ Q(t,pi)}[payoff(t, pi, h)].

`Q` is the independent nominal draw law implemented by
[the lottery DP](../crates/ournotes-sim/src/live/full/luck_dp.rs) and
[exact replay refinement](../crates/ournotes-sim/src/live/full/luck_exact.rs).
For a history `h`, its mass is the product of the conditional probabilities along its path. Lottery choices use
the current native integer weight divided by the current total weight; supported probability conditions use the
exact dyadic rational represented by their binary32 chance. State-dependent choices can depend on earlier history,
while successive source draws follow this declared independent law. The stream, clocks and rank context are fixed.
This law is distinct from averaging a finite set of seeded `System.Random` executions.

The reported probability-law field is `"lottery":"certifiedNativeLotteryIntervals"`. Exact refinement completes
all positive-mass paths of an admitted order, or retains the earlier enclosure. Conditional uncertainty over a
lottery remains separate from the uniform distribution over member orders.

#### Order and payoff enclosures

For each performance order `sigma`, the admitted scorer supplies score support and an outward interval containing
its nominal expected score. The lottery-state DP must retain every possible state, and the score recorder must
enclose each supported command/query schedule and its final integer score. These scheduling and effect checks are
premises of the enclosure. If `L_sigma <= E[Q_sigma] <= H_sigma`, then

    sum_sigma L_sigma / 120 <= U(t) <= sum_sigma H_sigma / 120.

Aggregation checks that the input contains 120 distinct permutations and rounds outward. An unrepresentable exact
rational aggregate can omit its exact metadata while keeping that interval certificate.

For a step payoff starting at value `v0`, with thresholds `t_i` and signed value changes `delta_i`,

    E[f(S)] = v0 + sum_i delta_i * Pr(S >= t_i).

The identity follows by writing the step function as a sum of threshold indicators and taking expectations; it
does not require reward tiers to be monotone. Score targets consume tail probabilities, capped scores consume
truncated expectations, and score/life conjunctions require a joint-event certificate. Support and first-moment
inequalities retain conservative bounds where refined tails are unavailable. These transforms are in
[`certified_search.rs`](../crates/ournotes-search/src/search/certified_search.rs).

Exact refinement replays each prefix in a fresh model and branches over every admitted positive-weight outcome.
A complete law is installed only when every branch terminates and exact rational terminal masses sum to 1.
Unsupported draws, resource limits or an interrupted branch retain the previous sound enclosure. A partial tree
is not a probability law of total mass 1.

#### LUCK ranking certificate

For every retained team `t`, maintain a certified payoff interval

    L(t) <= U(t) <= H(t).

The [interval frontier](../crates/ournotes-search/src/search/interval_topk.rs) proves that `t` precedes `s` when:

- `L(t) > H(s)`; or
- `L(t) = H(s)` and `t` has the better power/canonical-ID key; or
- exact expectations, or an equal-program certificate, prove the primary comparison, followed by that same key.

The endpoint-equality rule is sound even when one interval has positive width: every possible value of `t` is
at least every possible value of `s`; a strict actual inequality wins on payoff, and actual equality wins on
the secondary key. Equal overlapping intervals alone prove neither value equality nor rank.
An equal-program certificate covers the full evaluation program, power, declared context and payoff mapping.
Sharing a score coefficient, class metadata or an approximate scalar does not supply that certificate.

A candidate leaves the live frontier only after K distinct candidates are proved ahead of it. Extending a
certified ordered prefix requires its next team to precede every remaining live competitor and every unseen
completion covered by the remaining-domain cap. The same K-witness argument as in the deterministic proof then
preserves true Top-K membership.

`rankCertified` reports this ranking fact. Exact `expectedScore` and `expectedPayoff` fractions are optional
and can have any admitted positive `u128` denominator; averaging 120 rational order values need not produce a
fraction with denominator 120. Certified ranks can therefore be complete while exact expectations remain
unavailable. Exact order statistics and a deterministic `bestOrder` are reported only when the required exact
order values exist. An incomplete lower-level result may retain more than K unresolved frontier candidates;
the account facade presents its requested number without declaring their unresolved ranks proved.

#### The integer grid used by bounds

Node caps are often integer upper bounds on `120*U(t)`. This scaling is a bound grid, not a restriction on the
denominator of the true expectation. For an incumbent with lower endpoint `L_i`, define
`a_i = ceil(120*L_i)`, and let `T` be the K-th largest `a_i`. If an integer node cap `B < T`, at least K
incumbents satisfy

    B < ceil(120*L_i)  implies  B < 120*L_i <= 120*U_i.

The implication uses the integrality of `B`; it is valid whether or not `120*L_i` is an integer. Thus strict
grid pruning is sound. At `B = T`, let `r` incumbents have `a_i > T`; they already give strict witnesses. Among the
remaining incumbents with lower endpoint proved exactly equal to `T/120`, let `Q` be the `(K-r)`-th largest power.
Then a node power cap below `Q` supplies the missing tie witnesses, giving K distinct witnesses in total. If there
are too few exact-grid ties, the power-tie cut is disabled. A rounded-up endpoint alone cannot supply such a tie
certificate. This is the distinction made by `IntervalTopK::grid_cutoff`.

#### Exhaustion, refinement and displayed bounds

After candidate-domain exhaustion, an unresolved frontier returns `RefinementRequired` unless a time or candidate
stop takes precedence. A deadline during refinement returns `TimedOut` even when the candidate domain is exhausted.
An unlimited wall-clock setting does not remove the exact refinement provider's own admission and work limits.
Its current limits include 32 notes, 512 frames, branch depth 32 and 32,768 replay runs per order, with a shared
240,000-run/8,000,000-frame work budget. A declined or interrupted refinement preserves its previous interval.
Consequently, domain exhaustion, availability of an exact expectation, and certified ranking are separate
observable states.

When an exact fraction is available, integer display bounds are its mathematical floor and ceiling with the
entire unsigned denominator preserved. Probability display bounds use outward rational-to-binary64 conversion.
The exact fraction remains the source of value comparisons; a decimal display approximation is not a ranking
certificate. See [account recommendation](recommendation.md) for the result schema.

## Simulation work

### Simulation cutoff

A team whose value cannot reach the Top-K cutoff stops simulating as soon as the part of its score that is already
final shows it.

**Settled frames.** Under a validated stream of nonnegative, nondecreasing i32 frame times, after a play frame a later command lands
at a time no earlier than the least of: the frame's time (skill execution and re-application times; live skill events
not yet fired); the pending native timed finishes of every active live and condition execution; the positive music
length used to clamp finish times; the earliest chart time
of a note judged in a later frame (note commands, and checker override times taken from later notes); and the
start of every Gekisou range not yet finished (range-start and range-state override times, override times read
from the playing range's judged notes and the solo ranking rewind to the range start); and the end of every
externally ranked range whose fixed rank bonus has not yet been applied, even when the range is already finished.
The pending finishes use the same binary32 duration and integer timestamp as the updater. In particular, rounding
the elapsed time can postpone the strict end test past that timestamp; the current frame time alone is
insufficient. A future nonnegative extension can only move the finish later on the admitted clock domain. A
duration-reducing extension keeps the settled prefix empty, because it could move a future finish earlier.

Let `G` be the native `get_frame` map and `L` the last addressable score frame. Future commands can be clamped to
`L`, so they only satisfy `frame >= min(G(horizon),L)`. The calculator keeps `L` open and settles only already
executed frames strictly below that minimum and before any pending fixed-score frame. A rewind retains the
frames up to its own, hence those settled note and fixed scores are final. The native frame map uses binary32
conversion and ceiling; integer floor division is not interchangeable with it. The two parts of the construction
are [`LiveModel::settle`](../crates/ournotes-sim/src/live/full/mod.rs) and
[`IncrementalCalculator::settle`](../crates/ournotes-sim/src/live/full/scorecalc.rs).

**Cutoff tables.** For one team and performance order the fine cap is split by score frame. Each entry's term
bounds its note's final score whatever happens later, plus a conversion gain when a budget row converts it; an
entry in a settled frame already counts with its final score. A rank bonus is a percentage of its range's entries,
filed at the range end: once that frame is settled it is part of the settled total; before, an unsettled entry
carries its share in its rank factor and a settled one adds its term times the percentage (and that share of its
conversion gain). Each budget row converts at most its count among the unsettled entries and as many among the
settled ones, a bound on its conversions among all of them. The cap after a frame is the settled total plus this
remainder, rounded outward; PT applies the candidate's bonus and reachable reward tiers to it.

The [remainder arithmetic](../crates/ournotes-search/src/search/joint/cutoff.rs) preserves a separate invariant:
each suffix and retained conversion-edge sum is an upper endpoint of its exact stored sum. Adding edge `v` uses
`U <- round_up(U+v)`; removing a retained edge uses `U <- round_up(U-v)`. Both preserve that endpoint inequality.
The largest-k sum is monotone in each nonnegative edge, so heaps of bounded conversion gains preserve the cap.
Rank-share multiplication and division round upward separately with nonnegative numerators and positive
denominators. Exact zero stays zero; nonfinite prepared sums disable the optional table. A final reserve factor
widens an already certified bound and cannot substitute for these operation-level certificates.

**Test.** Every few frames the search adds the running order's cap to the exact payoffs of the orders already
simulated and the per-order caps of the orders still to run. The simulation stops only when that total is strictly
below the K-th payoff numerator, or equal with a lower power than the K-th result; otherwise it runs to the end and
the order is scored exactly. A stopped team cannot enter the Top-K. Orders without a finite table simulate normally.

### Deterministic idle trigger plan

A condition skill whose rows are all one-shot, without reset, and whose triggers are only the member's own
live-skill event (4010) or a fixed false trigger compiles an idle plan. Compound triggers never qualify, because
short-circuit checks can reset counters or consume skill random numbers. In a frame with no matching event and no
active execution the skill's triggers are pure false: the plan fills the trigger cache as the checks would and
skips condition, release, cumulative and applier work. Active and end-frame-pending instances keep the original
path. Trigger cache, phase order, errors and random-number consumption are unchanged, and effect metadata is
immutable after construction, so a compiled plan cannot go stale. A thread-local switch builds reference models
without the plan for the idle audit below.

## Search size and complexity

Let `n_c` be the number of eligible member cards for character `c`, and let `n = sum_c n_c`.
Before further constraints, the number of five-character member sets is the fifth elementary symmetric sum

    M5 = sum_{c1 < c2 < c3 < c4 < c5} n_c1*n_c2*n_c3*n_c4*n_c5 <= choose(n,5).

With `s` universally allowed Snaps, the number of optional injective bindings to five fixed members is

    B5(s) = sum_{j=0}^{min(5,s)} choose(5,j) * (s)_j,
    (s)_j = s*(s-1)*...*(s-j+1),  (s)_0 = 1.

Choose the `j` occupied slots, then assign an ordered set of `j` distinct Snaps. This counts each binding once,
including the empty binding. With every leader allowed, the canonical team count is `5*M5*B5(s)`;
leader/include/exclude constraints can only reduce it. Fixed physical-slot enumeration has another factor of 24
for the nonleader layouts. Performance orders are evaluations of a team, rather than additional result identities.

Even one fixed leader and five fixed members with five allowed Snaps gives `B5(5) = 1546` teams and 185,520
logical team/order values under deterministic exhaustive Live scoring. A member-set Top-K may return one
representative from this domain; a team Top-K retains the distinct bindings.

For a fixed five-member deck size, the general unpruned domain grows as `O(n^5 * (s+1)^5)`; evaluating each deterministic
order separately costs 120 whole-live evaluations per team. The implementation can share equivalent simulation
prefixes, so 120 is a count of logical order values rather than necessarily separate full simulations. Their cost depends on
the chart, frames, effects and replay work. LUCK refinement adds branching over positive-mass random histories
within its explicit work limits. When deck size itself is variable, the combination, injection and permutation
counts must retain that size parameter.

The full runtime also includes table compilation, per-node bounds, assignments, Top-K maintenance and result
materialization. A fixed five-row numeric Hungarian assignment takes `O(s)` arithmetic steps up to the constant
row factor; K-assignment and payoff-frontier DPs additionally depend on K and retained states. In particular,
bonus/power Pareto frontiers can contain many incomparable states. A depth-five traversal does not imply constant
total memory: tables, caches, retained DP states and results consume memory as well.

Branch-and-bound retains exactness even when its bounds reject nothing. K limits the result count; it does not
bound the number of candidates that must be excluded to prove those results. Heuristic ordering, warm starts,
cap-attainment policies and optional-table limits can improve work on an instance, while the coverage and
K-witness arguments preserve the answer.

These combinatorial bounds do not imply a uniform wall-clock completion guarantee. Deadlines are cooperative,
including preparation and final verification where the route provides those checks. An atomic unit can overrun
the deadline. `TimedOut` records incomplete proof work; `Complete` follows the route's closure certificate.

## Validation

The experiments below compare search results or bounds with independently enumerated candidates under the shared
scorer. Agreement establishes the checked equality or inequality on those enumerated finite inputs. The universal
search argument additionally needs the coverage, arithmetic and bound lemmas above. Scorer compatibility with the
game is a separate source of evidence ([native validation](native-validation.md)).
Counts of audited prefixes include repeated prefixes; they count checks, not independent samples.

There are three different validation obligations:

| Obligation | What the check establishes | Shared assumptions |
|---|---|---|
| Returned-value replay | A returned candidate agrees with the regular evaluator. | The candidate may still be globally suboptimal. |
| Full-domain ordered oracle | The whole returned Top-K equals independently enumerated and sorted candidates on that finite domain. | Resolved inputs, target and scorer must be the same. |
| Prefix/leaf cap audit | Every audited cap dominates each covered oracle completion. | Every bound family and its numeric admission boundary must be exercised. |

Returned-value replay alone cannot detect an optimum removed before evaluation. The separate oracle and cap checks
exercise that exclusion logic. These source-level mathematical arguments and executable checks are not a
machine-checked refinement proof of the entire Rust implementation.

### Tests without game data

`cargo test --release --locked --features search-diagnostics` runs these on synthetic deck data:

- Exhaustive enumeration (`search::oracle::brute_force`) evaluates every legal deck (member set, leader, snap
  assignment) with the regular deck-power path and its own ordering, and is compared with the search on complete
  ordered results for several K, objectives and constraint sets (`crates/ournotes-search/tests/search_oracle.rs`). It shares no bound,
  decomposition or Top-K code with the search. It shares constraint resolution, objective construction, permutation
  generation and the regular scorer; its independence concerns enumeration, representative selection and ordering.
- Each returned deck is re-evaluated with the regular path (see Outcomes); skip and live scores are recomputed with
  the general score calculator.
- The prepared live and skip evaluators are compared with the general calculator on random plays and charts.
- The objective with snap skills is compared with an exhaustive enumeration that simulates every member set, leader,
  snap placement and performance order (`crates/ournotes-search/tests/search_snaps.rs`; the `OURNOTES_DECK_SNAPS_*` variables enlarge it), on
  synthetic pools whose snaps extend live skills, add score factors, recover life, guard, convert judgements, count
  judgements and draw random numbers, under the default stream and random streams with late frames, lost life and
  life that runs out.
- The recommendation facade compares `branchAndBound` with `exhaustive` on synthetic team domains: full ordered
  Top-K with resource constraints, nonmonotone PT tiers, composition frontiers with K above the binding count, the
  PT membership regime with strict ties, supplied initial decks, and the fallback for wrapping PT and explicit
  clocks. Fixed decks in all 24 layouts of their non-leader pairs give the same canonical result; node bounds, bound
  modules and per-order caps cover the exact value of every team of an exhaustive ranking; LUCK intervals keep the
  leaders and prove equal-program ties, and probability decks without a LUCK range match the exhaustive ranking
  (`crates/ournotes-search/tests/adapter_fixture_export.rs`).
- Unit tests check the uniform order enumeration, the concave majorant of reward steps, and the member-additive
  bound against brute-force enumeration of completions.
- Numeric-domain regressions exercise forward nonnegative factor windows, causal clock boundaries, native negative
  recovery, nonfinite or subnormal chains, overflowing positive note sums, the binary32 integer boundary of COMBO
  stacks, held release windows, delayed timed finishes and finish-time clamping. They distinguish legitimate late
  playback frames from early note judgement and verify complete fallback identities for declined domains.
  Shared-order tests compare complete outcomes across wrapped effect-key ordering, and
  timer-floor tests cover both execution counts and network snapshot cancellation. Rank-key tests distinguish
  every real `i64` Snap ID from None. Effect-identity regressions exercise key wrapping, duplicate state identities,
  conversion-cache aliases and safe reuse of the same source row by different candidates. Rational output tests
  cover unsigned-denominator limits and verify probability endpoints against exact fractions.

### Team-domain oracle

The search harness ([tools/search-harness](../tools/search-harness/README.md)) enumerates every team of a bounded
domain independently of the production pool resolution, enumeration, assignment, bounds, cache and Top-K, and
evaluates every team with the fixed evaluator over its 120 performance orders. A case passes when the ordered Top-K
of every experiment equals the oracle's, every returned deck agrees with the fixed evaluator, and every audited cap
is at least the exact value (and power) of every oracle team it covers: joint prefixes, per-order leaf caps,
character and resource prefixes, classes and class bindings, compositions and teams, bound modules, bonus and
expected-bonus prefixes, per-member PT caps, suffixes and next pairs. Any
inadmissible cap fails the run. The synthetic suite needs no game data:

```sh
BDON_HARNESS_OUT=work/corpus cargo test --release --locked --test adapter_fixture_export export_search_harness_inputs -- --ignored
cargo build --release --locked --manifest-path tools/search-harness/Cargo.toml
python3 tools/search-harness/run.py run --binary tools/search-harness/target/release/ournotes-search-harness \
  --suite work/corpus/suite.json --out work/reports
```

Cases on real charts use a deck data file written by `nnnotes deck-data`, rosters from
`tools/search-harness/mock_rosters.py` and recommendation requests with an explicit `oracleMaxCandidates`; the
harness refuses a domain above that cap rather than emitting a truncated oracle.

### Audits of fixed decks

These binaries take `DATA ROSTER REQUEST DECKS OUTPUT`, where `DECKS` is a JSON array of
`{"members":[...],"snaps":[...]}`, and exit nonzero on any violation after saving the report. Without an
exhaustive oracle they apply to full-size rosters; the decks are typically the returned results of a search.

- `cutoff_audit` settles after every frame of a complete simulation for each deck at ten audited performance orders
  (the five cyclic shifts of the slot order and their reverses), and checks that no settled score frame is undone
  later, that every settled total equals the final scores of its frames, and that every cutoff cap is at least the
  final score. Orders without a finite table are reported as unavailable.
- `slack_profile` reports, per audited order, the team's fine cap beside its actual simulated score and per-note
  scores; the cap must not be lower than the score. The per-entry terms are attribution, not bounds.
- `idle_audit` evaluates each deck with and without the idle trigger plan and requires identical outcomes,
  including the score distribution, best order and non-timing counters; `--search` repeats this for a whole
  deterministic search.
- `score_program` compares recorded score programs with fresh complete simulations at a list of powers
  ([score programs](score-programs.md)).

### Independent PT plateau certificate

When the global PT ceiling is already attained by K incumbents, `pt_power_certificate` exports model bonus values
and complete per-slot power matrices, and `pt_certificate.py` independently resolves the PT ceiling and runs a
Top-K DP over the five-slot subset mask while scanning each Snap exactly once. It enumerates every allowed member
layout and preserves None-first ties, without production pruning or Hungarian assignment; the power-leading
assignments are evaluated over the performance orders and their serialized results compared with the production
Top-K. The certificate applies only when exactly five members remain eligible under the
independent member caps and those assignments attain the ceiling; other inputs are reported as inapplicable.

### Native and WebAssembly outcomes

`tools/search-harness/browser.cjs` runs the recommendation package in a Chromium Worker on original UTF-8 inputs
and compares every semantic outcome field with a native reference outcome, keeping JSON number tokens as strings so
integers beyond JavaScript's safe range cannot compare equal by rounding.
