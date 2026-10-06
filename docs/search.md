# Exact deck search

The recommendation facade searches leader/member-Snap teams under the declared deterministic or played-live
objective. Played-live values average all 120 member performance orders. Each API has an explicit result identity,
probability law and completion contract, specified below. The member-set solvers and the team solvers share power
and score components, while retaining their own result spaces. Live with Gekisou off uses the
[composition/Snap decomposition](#member-compositions-snap-pairings-and-power-frontiers);
Gekisou keeps joint member/Snap traversal. [Compiled envelopes](search-envelope-programs.md) and
[score programs](score-programs.md) describe reusable bound and evaluation components.
[Validation](#validation) gives reproducible correctness experiments.

## Search contracts and proof scope

Exactness is a statement about a declared finite optimization problem. Its inputs are the resolved card pool,
cultivation, song, scenario, play and clock inputs, constraints, payoff function, probability law, result identity,
canonical ordering and positive result limit `K`. These inputs stay fixed throughout one search.
Recommendation and session requests accept `1 <= K <= 100`; the member-set API uses its `usize` result limit.
The evaluator's integer and floating-point operations are part of the objective being optimized.
Legality means membership in the resolved `CandidateDomain`: eligible cards at their resolved cultivation,
five distinct characters, allowed leader, required/excluded cards and unique paired Snaps. The account envelope
records the input coverage used to build that domain.

The following interfaces use distinct result spaces:

| Interface | Result identity | Objective and representative |
|---|---|---|
| `search::search` (Power/Skip) | Set of five member card IDs | The best leader and Snap assignment of each member set, under the member-set ordering below. |
| `search::search_best_order_diagnostic` (Live) | Set of five member card IDs | The best leader, Snap assignment and performance order of each member set under its declared diagnostic objective. |
| Recommendation of a played Live | `team` | One leader and five member/Snap pairs, with the four nonleader pairs unordered; payoff averaged over the declared uniform member-order law. |
| Recommendation of Power or monotone Skip targets | `team` | One leader and five member/Snap pairs; deterministic payoff, then power, then the canonical team key. |
| Deterministic Skip client-point and conditional-item recommendation | `team` | The same team identity, with the declared client payoff evaluated for each team. |
| Fixed evaluation | `fixedTeam` or `fixedPhysicalDeck` | The requested deck under the selected evaluator's identity; `optimality` is `notApplicable`. |
| `SearchSession` v1 | `physicalDeck` | Deterministic Power/Skip with physical slot arrays, exact payoff, power and physical member/Snap ID ties. |

`search::topk::identity` defines the member-set identity. The recommendation engine's `team_identity_in`,
`uniform::canonical` and `physical::compare` define its identity and ordering. The returned `resultIdentity` and,
for sessions, `goalSpec` state the contract of the actual route.

### Common mathematical problem

Let `D` be the finite set of legal candidate configurations and let `I` be the route's result identities.
A configuration includes every choice optimized within a result: leader, members, Snap pairing and, for a
member-set Live objective, performance order. A uniform-order team's performance orders belong to its evaluation.
For each identity `i`, let `D_i` contain all its configurations. Its result is the best representative of `D_i`
under the route's ordering; layout-equivalent teams share one canonical layout. Let `V(i)` be that result's
objective and `P(i)` its power. All remaining tie keys are exact discrete values.
The requested answer is the first `min(K, |I|)` identities in the specified total order.

For a lottery-free played team `t`, write `q(t,o)` for its exact terminal payoff in performance order `o`:

    N(t) = sum over o in S_5 of q(t,o),       V(t) = N(t) / 120.

Every one of the 120 slot permutations has mass one, including permutations whose performers execute equivalent
skills. The common denominator permits comparisons of `N` alone. Deterministic Power and Skip use mass one.
For LUCK, the mathematical value is instead

    V(t) = (1/120) * sum over o in S_5 of E[q(t,o,R) | o],

where `R` follows the nominal conditional draw law defined in [LUCK](#luck). Certified intervals enclose this value; exact fractions are
provided when the certificate also establishes their numerator and denominator.

### Exactness theorem

The completed-search theorem uses these hypotheses:

1. **Finite legal domain.** The candidate construction and traversal cover every legal configuration, directly or
   through a proved equivalent canonical layout. Every domain partition covers its parent.
2. **Faithful evaluation.** A completed scalar candidate carries its exact objective and tie keys. Reused results
   satisfy the same evaluator contract. An interrupted scalar evaluation remains outside the scalar Top-K.
3. **Admissible bounds.** Each numeric cap bounds every completion represented by its node, in the units used by
   the cutoff. A power cap used for a payoff tie bounds all completions that can attain that payoff cap.
4. **Certified arithmetic.** The operations used to establish each bound satisfy its sign, range and rounding
   assumptions. Checked exact sums establish values; outward-rounded relaxations establish upper bounds.
5. **Order-preserving exclusions.** A discarded configuration transfers its obligation to an at-least-as-good
   evaluated or open representative of its own identity, or ranks after evaluated or certified representatives
   of `K` distinct identities. Comparisons use the full order; identical configurations share one evaluation.
6. **Closed completion.** `Complete` follows only after every legal configuration has a completed evaluation or
   such a certificate, and any retained interval frontier has a proved ordered answer.

Under these hypotheses, a completed search returns exactly the first `min(K, |I|)` identities and their declared
values or certified value intervals. The theorem is conditional on the declared evaluator and inputs.
The [model validation](native-validation.md) contract supplies the separate relation to game behavior.

**Proof.** For each raw configuration `d`, maintain one of three witnesses: an evaluated at-least-as-good
configuration of the same identity; `K` distinct evaluated or certified identities whose representatives rank
before `d`; or an open evaluation/proof obligation for `d` or an at-least-as-good representative of its identity.
Branching replaces an open region by covering children. Evaluation closes its obligation. Representative
substitution transfers the obligation; global pruning supplies the `K` witnesses. Evaluating or discarding one
representative leaves stronger open representatives covered. Domain partitions preserve this invariant and
share one Top-K.

Among the evaluated identities, insertion maintains the first `K` in the canonical order. For the member-set
route, an insertion first improves the representative of its own identity, then competes with other identities.
An identity removed from this Top-K has `K` distinct better evaluated identities. Their true order remains valid
as further evaluations improve the incumbents. Consequently the full K-th key improves monotonically; its primary
value and then its power at a primary tie provide monotone pruning thresholds.

Suppose a true Top-K identity is missing or has an inferior representative at completion, and take its best
configuration `d`. Every open proof obligation is closed, so its witness is an evaluated at-least-as-good representative of its
own identity or `K` distinct better representatives. The first case gives the insertion invariant its true best
representative. The second implies `K` other identities rank before it: a strictly better
representative of its own identity would contradict the choice of `d`. Both conclusions contradict the supposed
error. Thus the maintained Top-K is the first `min(K, |I|)` of the domain, including every identity when fewer
than `K` exist. The empty domain yields an empty completed answer.

### Numeric and canonical cutoffs

For scalar team ranking, let `(T,Q)` be the K-th incumbent's payoff numerator and power. A node with payoff cap
`B` and a corresponding power cap `H` may close when

    B < T,       or       B = T and H < Q.

At `B = T` and `H = Q`, a canonical lower key `L` can additionally prove closure when every completion's key
is at least `L` and `L` ranks after the K-th key. Equality of complete keys denotes the same already considered
identity. A bound that supplies only a primary cap retains equality. Before the Top-K contains `K` distinct
identities, its K-th threshold is absent.

Independent upper bounds of the same completion set combine by taking their minimum. If completion sets
`C_i` cover a parent and `B_i` bounds `C_i`, the parent is bounded by `B = max_i B_i`. For power ties,
`H = max_{i: B_i = B} H_i` suffices when each `H_i` bounds the power of its cell: every completion attaining
`B` lies in a cell attaining `B`. This is the conditional power certificate used by exact-bonus cells.

A carrier-multiset check can stop as soon as one cell exceeds the cutoff; that outcome keeps the prefix.
A certificate closing the prefix covers every remaining multiset. A recorded frontier upper bound likewise
covers the entire region it represents. Relaxations preserve a map from every real completion into their
feasible sets; this coverage justifies dropping a resource or required-member constraint inside a bound.
See [`NodeBound`](../crates/ournotes-search/src/search/joint.rs),
[bonus cells](../crates/ournotes-search/src/search/joint/bonus.rs) and
[carrier splits](../crates/ournotes-search/src/search/joint/carrier_split.rs).

These conditions handle a flat objective correctly: a score plateau still ranks by power, and equal power still
ranks by member and Snap IDs. Member-set search places an empty Snap after real IDs; team search places it before
real IDs. Each assignment solver and lower-key construction uses the convention of its consumer.

For a scalar leaf with evaluated order set `E`, the unresolved numerator is bounded by

    C = sum over o in E of q(t,o) + sum over o outside E of b(t,o),

where each `b(t,o)` bounds that order's complete payoff. The same cutoff applies to `C` and the team's exact power.
During shared-prefix simulation, each open order-tree node represents a set of orders. Its cap is the smaller
of its direct cap and the sum of its completed children's payoffs, open child's cap and unstarted children's caps.
Induction up the tree gives a cap of the same complete numerator. A cutoff closes the team only through this bound.

### Outcomes and partial results

The recommendation outcome separates completion, rank proof and stopping cause:

| Outcome fields | Meaning |
|---|---|
| `Complete`, `proven`, `exhausted` | The full canonical Top-K is certified for the declared search domain. |
| `TimedOut`, `unproven`, `timeLimit` | Cooperative time checks stopped a search; completed scalar values or certified intervals are retained. |
| `TimedOut`, `unproven`, `candidateLimit` | The configured candidate limit stopped a complete-domain strategy. |
| `TimedOut`, `heuristic`, a limit reason | The candidate strategy returned its evaluated proposals. |
| `RefinementRequired`, `unproven`, `refinementRequired` | Domain traversal is closed; retained payoff intervals still require refinement to establish the requested ranking. |
| Fixed evaluation, `notApplicable` | The result evaluates the requested deck; its completion concerns that evaluation. |

On a feasible domain, the candidate strategy ends with a limit reason and `heuristic` optimality. An infeasible
declared domain has an empty completed answer independently of candidate proposals.

For scalar played-live results, `expectedScore` and `expectedPayoff` are exact fractions over 120, and the complete
order vector supplies `scoreSummary` and `bestOrder`. LUCK results carry `scoreInterval`, `payoffInterval` and
`rankCertified`; their exact fraction fields are optional, and the scalar order-summary fields are absent.
Validation and evaluation errors return an error through the API's error channel.

A deadline is cooperative. An indivisible preparation or evaluation operation can finish after its expiry.
The completion field records whether the proof closed; elapsed time records the work's duration.
Within a partial scalar leaf, completed individual orders may be reusable, while only a completed aggregate
enters the result Top-K. Interval leaves retain their certified value contract.

`telemetry.proof.upperBound`, when supplied, bounds the best numerator in regions left open by the stop.
For a nonempty scalar domain, taking the maximum of that bound and the best evaluated numerator bounds the
whole-domain optimum. `globalUpperBound` records a valid whole-domain bound and can tighten as regions close.
The equality of a primary upper bound and an incumbent establishes the optimal primary value; canonical Top-K
certification additionally closes the power and identity ties for all requested ranks.

`engine::recommend_with_progress` and the account facade expose completed candidates and the current proof state
through progress callbacks. Their telemetry is closed on a copy;
the running frontier stays available to the search. The stop's final unexplored-domain bound is computed when
the traversal unwinds. Callback work contributes to elapsed time under the cooperative budget.

## Problem

A deck has five member cards in slots 0..4 and up to five snaps, one per slot. The member in slot 2 is the leader.
The members belong to five different characters; a snap appears at most once. Snaps are optional.

Deck power is computed per slot and summed. For a slot holding member `m` and snap `s` under leader skill profile
`L` (the leader card's leader skill at its level), the slot total in points is

    S(m, s, L) = A(m) + LEAD_L(m) + W(m, s)

- `A(m)`: every term that depends on the member alone (its own power, character rank and total rank bonuses, band
  items, song type and tag bonuses, memory, VIP, and the member's event bonus when event parameters are used);
- `LEAD_L(m)`: `floor(B_m x leaderPercent_L(m))`, where `B_m` is the member's base (its power plus the flat
  character rank, total rank and memory points) and `leaderPercent_L(m)` sums the leader skill effects that target
  `m`;
- `W(m, s)`: `floor(B_m x (snapPercent_s + snapEvent_s)) + floor(B_m x typeLink(m, s))`, and `W(m, none) = 0`.

Here each displayed percentage term means the model's component-wise operation, including its binary32
conversion and finite saturation. With base stat `B` and percentage `p` in BP units, define

    mulBP(B,p) = trunc0(B*p / 10000),
    F(B,p) = floor_to_i32(RN32(RN32(mulBP(B,p)) / 10000)).

`F` is the stat contribution in points. `RN32` denotes rounding to binary32; `floor_to_i32` includes the
finite saturating conversion implemented in [`num.rs`](../crates/ournotes-sim/src/num.rs). On the checked product
domain, multiplication and truncation are monotone in `p` for `B >= 0`; both binary32 operations and finite
saturating floor preserve that order. The table bounds keep the integer products and converted operands finite.
The model's [`CardPower`](../crates/ournotes-sim/src/power.rs) operations supply these exact terms.

Each prepared slot term has a whole number of points per stat, represented by a multiple of 10,000 BP.
The additive power identity also requires the final stat conversions and total sum to stay in their proved
range. Let `x_j` be a completed deck's integer point sum for stat `j`, before the final conversion, and let
`b_j >= x_j` be its sum of nonnegative component upper contributions. The prepared leader bound and the
required-member/distinct-character lower proof establish

    sum_j b_j <= Pmax <= i32::MAX,       sum_j x_j >= 0.

For every stat,

    x_j <= b_j <= Pmax,
    x_j >= -sum_{k != j} x_k >= -sum_{k != j} b_k >= -Pmax.

Each final stat is therefore representable. Its whole-point BP value converts exactly through the final
binary64 division, and the total lies in `[0,Pmax]`. Thus the model's power is exactly the sum of the slot
terms, with representable final conversions and a nonwrapping total. This is a whole-domain certificate,
including branches that can subsequently be pruned. Individual percentage terms still use the finite
saturating binary32 conversion above, whose monotonicity is sufficient for their bounds.

Table construction checks fixed-point integrality, signs and product limits; the member-set search establishes
the total-power interval before traversal and returns `Error::Domain` when its certificate is unavailable.
Team bound compilation establishes the corresponding interval for its prepared domain; the recommendation
facade retains its exhaustive evaluation route when a bound is unavailable. The ordinary evaluator supplies
the declared integer semantics in either route. Source: [tables](../crates/ournotes-search/src/search/tables.rs),
[member-set preparation](../crates/ournotes-search/src/search/mod.rs) and
[team power bounds](../crates/ournotes-search/src/search/team_power.rs).

The four nonleader positions preserve power under pair relabelling; leader choice and Snap pairing can change it.

A leader profile is *simple* when none of its effects has a condition group or a cumulative count: then
`leaderPercent_L(m)` depends on `m` alone. Otherwise (conditions on the other members or on the song, counts over the
deck) it depends on the whole member set; the search evaluates it exactly for every complete member set and bounds
it before.

## Member-set result identity and order

One result per set of five member cards. Its representative is the best deck with those members under this order,
better first:

1. objective, descending;
2. deck power, descending (for score objectives);
3. the five member card ids, ascending, compared as a sorted list;
4. the leader card id, ascending;
5. the snap ids in slot order, ascending, with "no snap" after every snap;
6. the performance order, ascending.

Canonical Snap keys distinguish every `i64` card ID from an empty slot.

The non-leader members sit in slots 0, 1, 3, 4 in ascending card-id order. The function `identity` in
`crates/ournotes-search/src/search/topk.rs` is the only place that defines what makes two results the same.

## Member-set outcomes

- `Complete`: the results are exactly the first `min(K, number of feasible member sets)` representatives under the
  order above.
- `TimedOut`: every result is a legal deck whose values are computed exactly, but some better deck may be missing.
  Preparation, traversal and final verification share one cooperative deadline. Only completely verified
  candidates are returned. An atomic verification can finish after expiry, but another candidate is not started.
- An exhausted domain with no legal deck (fewer than five characters, or constraints that exclude every deck):
  `Complete` with no results.
- `Error::Domain`, `Error::Input`, `Error::Game`, `Error::Capacity`: no results.

Every returned deck is evaluated again with the regular deck-power path (and, for the live objective with snap
skills, simulated again); a difference from the search's value is an error, not a result.

`K = 0` is an input error, including with a zero budget or infeasible pool. A zero budget returns `TimedOut`
without preparing tables or visiting decks. K is a result limit, so requesting more results than exist does not
allocate K entries up front. A cooperative deadline is checked between atomic operations.
The separate [search session](search-session.md) API provides resumable deterministic Power/Skip physical-deck
search with its own identity and step contract.

For Skip search, ordering by power additionally requires a proof over the entire feasible power domain. The
solver combines target-aware lower bounds for signed leader effects with required members and distinct-character
constraints, and takes the largest prepared leader bound as its power upper bound. It rejects a domain unless
power is nonnegative and within i32 and every intermediate Skip f32 operation at that upper bound is finite and
nonnegative, with a nonwrapping note sum. This is conservative: a rejected domain need not contain an actual
overflow. The unrestricted evaluator retains the native conversion and wrapping behavior, including infinity
converting to MIN. The search proof is conditional on the declared model and resolved inputs; the validation
section specifies the scope of its independent comparisons.

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


### Exact assignment and empty-slot ties

The five-row maximum-weight assignment uses one column per real Snap and five dummy columns. Distinct real
columns enforce unique Snaps; the dummies supply independent empty slots. For the member-set route, sort the
`n` real Snap IDs and give them ranks `0..n-1`, with None at rank `n`. Set `b=n+1` and `B=b^5`. The transformed
objective for assignment weight `W` and row ranks `j_i` is

    W*B - sum from i=0 to 4 of j_i*b^(4-i).

The second term lies in `[0,B-1]`. An integer weight improvement of one therefore wins over every tie penalty;
at equal weight, the penalty orders the five row ranks lexicographically. The numeric optimum and the
None-last representative are both exact. Restricted class assignments retain the same encoding on allowed
edges. With `M` bounding absolute edge weights, the forbidden-edge cost `16*(M+2)*B` exceeds the possible cost
advantage from the other four rows, so a feasible assignment wins over one using a forbidden edge. The returned
binding is checked for allowed edges. See [matching](../crates/ournotes-search/src/search/matching.rs).

The encoding also fits its integer implementation. `Pool` admits at most 65,535 Snaps, so `B<=2^80`.
Prepared Snap weights sum six nonnegative finite `i32` component terms and are less than `2^34`; the
forbidden cost `F=16*(M+2)*B` is therefore below `2^118`. Quantized resource and proposal callers admit at
most 4,096 Snaps and absolute weights at most `10^15`, giving `F<2^115`.

For the five-row Hungarian solve, all matrix costs lie in `[-F,F]`. Real-column potentials remain nonpositive,
and free columns have potential zero. Dual feasibility, matched-edge equality and an available free column
give row potentials in `[-F,F]` and real-column potentials in `[-2F,0]`. Reduced costs and finite slacks are
bounded by `4F`; at most five nonzero real-column potentials bound the auxiliary objective potential by
`15F`. The corresponding updates fit below `16F<2^122`, within `i128`. Finite slacks lie below the
`i128::MAX/4` sentinel; every unused slack is initialized by its first scan. The exact-integer optimization
therefore implements the radix proof on these admitted domains.

Sources: [pool capacity](../crates/ournotes-sim/src/pool.rs),
[Snap weight construction](../crates/ournotes-search/src/search/tables.rs),
[resource limits](../crates/ournotes-search/src/search/joint/resource.rs),
[class-assignment limits](../crates/ournotes-search/src/search/joint/classes.rs) and
[prefix-resource limits](../crates/ournotes-search/src/search/joint/prefix_resource.rs).

## Why no result is lost

Let `T` be the K-th value currently held (minus infinity while fewer than K results are held). `T` never decreases.
A branch is discarded only when its bound is **strictly** below `T`; equal values are always explored, because an
equal-valued deck can still win on the later keys of the order.

1. *The bound is at least the value of every deck below the node.* For a member `m` in any deck: `A(m)` is exact;
   `W(m, s) <= Wmax(m)` for every snap and for an empty slot, since `Wmax` is a maximum over all allowed snaps and 0
   (relaxing the rule that snaps are distinct only enlarges the maximum); `LEAD_L(m)` is exact for simple profiles. For
   other profiles the search uses `floor(B_m x P)` with `P` the component-wise sum over the effects that could target
   `m` of `max(0, value x c)`, where `c` is the largest count the effect can take (at most 5, or its cap), counting every
   conditional effect as active. The exact percentage is a sum over a subset of those effects with counts at most `c`,
   so it is at most `P` component-wise. The slot multiplication `(B x p) / 10000` with truncation and the floor through
   binary32 are both non-decreasing in `p` for `B >= 0` (IEEE rounding is monotone, and so is `floor`), so the exact
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

Under these assumptions the leader and the snaps change the live score only through the deck power `P`. Preparation
checks the common power interval and the score chain over that entire interval: fixed multipliers are finite and
nonnegative, positive intermediate values remain normal, the maximum intermediates remain finite, and the native
sum of note scores does not wrap. Every supported factor command has a nonnegative start and an equal opposite end
at the same or a later time. The ideal active factor is therefore at least one, and its rounding certificate keeps
the native factor positive. For a fixed performance order every operation is monotone in `P`, so a member set's
best representative is its highest-power leader and Snap choice with its best performance order.

For `N` commands and total absolute command magnitude at most `F`, use `u=2^-24`, `alpha=(3N+1)u` and
`D=alpha/(1-alpha)*(1+F)`, with all proof arithmetic rounded outward. The count charges two representation
roundings and one state addition per command, plus the final field addition. Preparation requires `alpha<1` and
`D<1/2`, and encloses every score-up factor in `[1-D,1+F+D]`. Per-event maxima cover every performer assignment.
The score audit checks positive paths at power one and the lower factor endpoint, and maximum paths at the
prepared power cap and the upper endpoint. Zero multiplier paths stay zero. These checks establish the premises
for monotonicity and the relative score-chain error bound; an unavailable certificate reports a domain error.

Sources: [`LiveCtx::new`](../crates/ournotes-search/src/search/live.rs),
[`LiveModel::prove_search_domain`](../crates/ournotes-sim/src/live/model.rs).

Bound: for note `n`, the score is at most `P * k_n * U_n * (1 + e)`, where `k_n` is the product of the note's fixed
constants (assist, life, judgement and note percentages, score adjustment, level factor, combo factor, divided by
the converted note count), `U_n` bounds the score-up factor and `e = 2e-6` covers the float roundings of the chain
(at most 16 roundings of relative size 2^-24). `U_n = 1 + S_n + d`, where `S_n` sums, over the skill events whose
effect window can contain the note, the largest factor any available performer gives at that event (for the note's
judgement), and `d=D` bounds the accumulated representation and state error above. The native converted-note
count is first rounded to binary32, and the coefficient calculation uses that same denominator. Coefficients,
gains and their sums round outward. The floors only lower the score. The search compares `P_bound * A` with
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
from `i = 0` while that time is at most `T + 2000`, where `T` is the maximum of zero, every chart-note time and
every skill-event time, including unjudged chart notes.
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
search checks that (score adjustment and level factor positive, converted note count positive, assist and life-zero
factors, note and judgement percentages, combo factors and score factor values non-negative) and that its score bound
stays below half of the 32-bit range, so the sum does not wrap. For fixed performers and stream, the score is
therefore a non-decreasing function of the power.

**2. The leader.** Performers follow the members, whatever their slots, and the snap terms of the power `W(m, s)` do
not depend on the leader; so the leader changes only the member-only part of the power. The best representative's
leader is the allowed member with the largest exact member-only power (ties: smallest id); a leaf reached with another
leader returns at once. If the canonical leader's leaf is pruned, its bound is below the K-th value, and every other
leader gives the same performers at a power no larger, so the member set cannot enter the result.

**3. Snap classes.** For each allowed member `m` and each allowed snap, every effect row of the snap's support skills
is classified. Two facts come first, from the stream and every allowed card:

- the *reachable judgements* of each stream entry: its raw judgement, and the target judgement of every conversion an
  allowed card has that lists the raw judgement and can see the entry. Conversion functions only see raw judgements. A
  conversion of a live skill, or of a snap row with any other trigger, can see every entry; a snap conversion
  triggered by its performer's own skill event is registered in the frame where the event fires and sees the entries
  judged in the next frames up to the first frame later than its start plus its activation time (with a margin of
  `2^-22` relative and 1 ms);
- the *life interval* `[lo, hi]`: `lo` is the base life when no reachable judgement of any entry costs life, else 0;
  `hi` is twice the base when some allowed card recovers life, else the base. Every life value the simulation computes
  lies in it, whatever the order of the life commands and the state of the life frame cache: recovery caps at twice
  the base, damage floors at 0, and without damage nothing lowers the life (the search checks that damage is
  non-negative and recovery below `2^30`, so no 32-bit sum wraps). Life is *rigid* when every life condition of any
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
  is impure and its cumulative condition is valid: life recovery and guard when life is rigid; a judgement conversion
  when no raw judgement of the stream is one of its targets other than the one it converts to (a conversion that
  never converts changes neither the judgement nor the order of the other conversions);
- *active*: anything else (score factors, extensions of the member's live skill, and every row with an impure
  checker).

Removing never and inert rows, and skills left without rows, from a performer leaves the simulation's score
unchanged: their factor commands do not exist, the life values they change are read by nothing that can change the
score, and the other rows keep their order. A snap's *class key* for `m` is the list, per skill with active rows (in
the performer's order), of its active rows by row id, each written with every field the simulation reads except its
id; a condition that is decided true and pure is written as none. The simulation reads a row id only as a sort key
within its skill (the list keeps that order) and as the identity of the row's own state, so snaps with equal keys are
interchangeable at `m`. The empty key (class 0) holds the snaps that cannot change anything at `m`, together with "no
snap".

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
- the coarse separable envelope uses `V_e = max_j jp(j) * (1 + N_e) + sum_j jp(j) * J_e(j)`, over the reachable judgements `j` of the entry, `jp(j)` the
  judgement percentage, `N_e` and `J_e(j)` the note and judgement factors whose windows contain `t_e`. The note ends
  with one reachable judgement `x`, and its score-up value `jp(x) * (1 + N + J(x))` is at most `V_e`.

Above the candidates these coefficients use the reachable judgements of every allowed card. For one candidate (a
member set, a performance order and a class for each slot) only its own performers register conversions, and one
triggered by its performer's skill event only at that position's events; the per-entry bound of a candidate uses the
reachable judgements of its own conversions, and the combo breaks that follow from them. They are a subset of the
pool-wide ones, so the candidate's combo counts and percentages are never larger. The fine cap also preserves
the exclusivity of the final judgement: it uses `max_j jp(j)*(1+max(0,N_e)+max(0,J_e(j))+D)`, with the
candidate's factor error `D`, before the separate score-chain allowance and floors.

Life of a candidate. When no performer of a candidate has a life recovery or guard row that can start, every life the
simulation computes when a note reads its life is at most `max(0, base - filed minimum damage)`. The filed damage is from the
entries judged in earlier frames, or earlier in the same frame, the note itself included, with chart times up to the
note's: damage only lowers the life and floors at 0, and a life query folds every filed command up to its time at
least once (with the frame cache, some of them twice). The damage of an entry is at least the smallest damage of its
reachable judgements. Where this bound is 0 the note's life is 0, and `Z_e` is the assist factor times the life-zero
factor. When no allowed card has a life recovery or guard row, the coefficients above the candidates use it too.

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
when the life starts at 0. With positive starting life, a fold that reaches 0 ends at 0; an always-positive fold
adds at most the counted recoveries and subtracts at least the required damage, so it is at most `x + r - d` and
at most `2 * base`. Both cases are bounded by `min(2 * base, max(0, x + r - d))`. Repeated damage only lowers the result.

For a candidate whose only life-raising rows are such recoveries (no guard, no recovery with another trigger, none in
the live skills), the search folds, slot by slot, every entry's smallest damage and the candidate's recoveries at their
events, and takes the end `t0` of the first slot at which this fold is 0. An entry at chart time `t_e` reads life 0
when `t0 <= t_e` and every entry at a chart time up to `t0` is judged no later than it: its query folds all those
damages and only recoveries this fold contains, at most as often, so the life it reads is at most the fold's value at
`t0`, which is 0, and the later commands keep it 0.

Final life. The final life of a play is the life the query at the last play frame reads, a fold of the commands filed
at times up to that frame's time. For such a candidate, the same slot-by-slot fold over the smallest damage of the
entries at chart times up to the last play frame's time and the candidate's recoveries at their events is at least
that life, and it is 0 from the first slot at which it reaches 0. A score and life target pays nothing in an order
where this fold is below its least final life, so the per-order caps of that order are 0. The bound is not used when
life is rigid (recovery and guard rows are then left out of the snap classes).

Windows. A factor started at `exec` holds for the notes with chart times in `[exec, finish)`: its start and end
commands are filed at those times, and a command filed in a score frame that was already executed undoes and
re-executes the frames from there, so every note is last scored with the factors whose commands surround its chart
time. A live skill effect of position `k` starts at the time of each skill event of `k` that some frame reaches. Its
duration is `d = act * 1000 + extension`, in binary32; it ends at `exec + ceil(d)`, or at the next frame after its
start frame when that is later, if a frame later than `exec + d` exists, and never otherwise (an activation time that
is not positive ends it in the next frame; with an extension the bound then uses no end). `d` is non-decreasing in the
extension (binary32 addition and `ceil` are monotone), so the bound uses the largest extension: the extension rows of
the member's live skill and of its snap class that can start, each once per event of the position. It uses no end at
all when an extension row of the snap class has a trigger other than the performer's own skill event, or when the
position has two events (a restart does not remove the first factor). A snap score row triggered by its performer's
own skill event starts at the frame where the event fires and ends by the same rule without extension; a snap score
row with any other trigger counts over the whole live, five concurrent executions at once. Live skill rows of one
member with the same effect type and duration whose conditions are one condition each, negations of each other, cannot
both start at one event: both are checked in the same phase of the frame, before any effect of that phase applies, and
repeated life queries at one time give one value (probability and count conditions, which change with each check, are
excluded). When the position has at most one event such a pair counts once with the larger factors. Every factor value
is non-negative, so a window that is too wide only raises the bound.

**Float margin and command-count certificate.**

Let `N` bound lifetime factor commands and `E` their total executions including
score-frame replays. For a row touching `h` score fields, let `s` bound starts,
`c` concurrency, `f` processing frames and `q` replacements per activation:

`N_row = 2 h [s + min(s q, c f)]`.

A fixed factor has zero replacements. The finite cumulative replacement count
requires every reachable requested factor to reconstruct to the identical
binary32 value after mill quantization; other cases use the processing-frame
count. Each start, end and replacement is charged, and its possible filing
frames supply an upper replay multiplicity for `E`.

Map every possible factor span `[a,b]` to the closed interval `[frame(a),frame(b)]` using the scorer's
binary32 time-to-frame calculation and its final-frame clamping. Every factor that can be applied, removed
or remain active in a score frame has a projected interval containing that frame. The maximum sum of interval
magnitudes therefore bounds its ideal transient factor state above the base value. Closed endpoints include equal-time start/end
pairs and the final clamped frame. A frame difference is the signed difference between two states in this
nonnegative envelope. The compiled raw bound sums the per-slot maxima, which bounds the maximum of their sum.

Set `u=2^-24`, `W=3E+2N`, `F` to that frame-magnitude bound,
and `S=1+max(F, peak active factor sum)`. State addition, frame-difference
addition and undo account for `3E`; integer-to-binary32 conversion and division
account for `2N`. An absolute score-up error certificate is

`D = u [E (2S+F) + 2N S] / (1-uW)`, where `uW<1`.

The feedback inequality is `error<=u M+u W error`, with
`M=E(2S+F)+2N S`. Solving gives the displayed bound. The implementation rounds
outward and uses `max(1.01,1/(1-uW))` for amplification. This argument requires
the chosen start, replacement, replay and frame-magnitude envelopes to cover
every reachable command state.

A candidate's fine cap adds `D` to the note-plus-selected-judgement factor before
applying the separate multiplicative allowances for the score chain. For a
linear joint envelope, `B=sum_e k_e z_e max_j judgement_percent(j)` bounds the
sensitivity to absolute factor error, including budgeted conversions. Therefore
`P(A+D B)` bounds the unfloored contribution before the chain allowance. The
joint refinement uses a larger prepared allowance and its positivity and finite
normal-range checks. An unavailable global certificate rejects preparation;
an unavailable candidate certificate supplies the maximal cap; an unavailable
compiled raw cap returns `None`; an inapplicable refinement retains its enclosing
envelope.

Source: [`command_count`](../crates/ournotes-search/src/search/snaps/score_windows.rs), [`amplification` and `WindowRoundoff`](../crates/ournotes-search/src/search/snaps/float_margin.rs),
[`FineView::cand_drift`](../crates/ournotes-search/src/search/snaps/fine_view.rs),
[`additive_joint_envelope`](../crates/ournotes-search/src/search/snaps/snap_live.rs).

When a fine cap uses refined Rush windows, `WindowRoundoff` additionally certifies their binary64 endpoint
construction. With `w` windows, at most
`2w` endpoint writes, `2w` nonzero prefix additions and one factor multiplication affect an input path.
For `v=2^-53` and `(4w+1)v<1`, the absolute construction error is at most

    gamma_(4w+1) * sum of absolute endpoint inputs,
    gamma_n = n*v/(1-n*v).

`WindowRoundoff` encloses the endpoint norm and this factor with outward rounding. The compiled raw envelope
independently stores lower and upper coefficient-prefix endpoints and uses `upper[end] - lower[start]` for
interval caps. The separate chain allowance and the operation-counted state drift together
justify the coarse relative `eps`; candidate fine caps retain the tighter additive drift before their floors.

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
   candidates are simulated as the list grows, so the cutoff rises early. Candidates with the same member identities
   and Snap class IDs in performance order, at the same power, are simulated once.

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

### Finite domain and exhaustive work

With `n` eligible member cards and `s` eligible Snaps, the number of member sets is at most `binom(n,5)`;
distinct-character and required-card constraints reduce that count. A fixed member set has at most five leader
choices. Its number of optional injective Snap bindings is

    B(s) = sum from j=0 to min(5,s) of binom(5,j) * s!/(s-j)!.

Choose the `j` occupied slots and inject `j` distinct Snaps into them; the other slots are independently empty.
Consequently the team domain has at most `5 * binom(n,5) * B(s)` identities. A physical-slot contract additionally
retains the `4!` nonleader layouts. Lottery-free played evaluation aggregates 120 order outcomes per team.
LUCK evaluation also depends on its admitted lottery state space and refinement limits.

These are finite exhaustive bounds on configurations. Pruning, assignments and exact evaluation reuse reduce
the work by the certificates described here. A declared resource limit returns a partial outcome when proof
obligations remain open; closure certifies the requested ordering.

## Uniform member-order search

### Target

A team is a leader in slot 2, four other members and the Snap (or none) paired with each member. In a performance
order the five members act at the five skill positions; the paired Snaps follow their members. The target value of a
team is the mean of its payoff over the 120 performance orders, each equally likely:

    U(team) = (1/120) * sum over the 120 orders of payoff(team, order)

Each order has the outcome of a complete simulation with the declared judgement stream. Exact score laws,
validated score programs and shared simulation prefixes can supply that outcome as described in
[leaf evaluation](#leaf-evaluation). The payoff is the selected metric applied to that order's final score,
rank and life, with the team's declared event context.

Permuting the four non-leader `(member, Snap)` pairs preserves both power and the uniform mean payoff. To see
this, let `sigma` be the slot permutation fixing slot 2. Relabeling every performance order by `sigma` gives a
bijection on the 120 orders and preserves the actual sequence of paired performers. Corresponding simulations
therefore have the same input and payoff. Summing over that bijection proves layout invariance. Leader choice
and Snap pairing remain team decisions; power is an input to the score calculation.

A skill probability check draws a random number. With Gekisou off such a draw ends the request with `Unsupported`.
For Gekisou without a LUCK range, the recorder's effect and condition checks can certify that every draw history
has the same score, life and declared rank arrivals. Such an admitted model keeps one exact score per order
(`"lottery":"noLuckRange"`, below). LUCK models use certified intervals under the nominal conditional draw law
defined in [LUCK](#luck).

### Result identity and shape

`resultIdentity` is `team` (`fixedTeam` for a fixed-deck evaluation). A team is reported in its canonical layout: the
leader in slot 2 and the other (member, Snap) pairs in slots 0, 1, 3 and 4 in ascending member card ID order. Every
layout of the same pairs is the same team, and a fixed deck given in any layout evaluates and reports as that team.
Lottery-free results are ordered by

1. `expectedPayoff.numerator`, descending (each such played-live result has denominator 120);
2. power, descending;
3. member card IDs of the canonical layout, ascending;
4. Snap IDs of the canonical layout, ascending, with "no Snap" before every ID.

Each lottery-free result carries `expectedScore` and `expectedPayoff` as exact fractions over 120, `scoreSummary` (minimum,
maximum, lower quantiles and the probability of reaching a score target, over the 120 equally likely orders) and
`bestOrder`: the first performance order, in lexicographic order of slot permutations, with the highest payoff and
then the highest score. `bestOrder.performanceOrder` lists the result's slots in performance order,
`bestOrder.members` the member card IDs in that order, with that order's `score` and `payoff`. It reproduces one play;
it is not a decision of the search. `probabilityLaw` is `{"kind":"uniformMemberOrder","orders":120,"lottery":"none"}`.
An admitted Gekisou model without a LUCK range whose skills read a probability reports `"lottery":"noLuckRange"`.
Its recorder certificate establishes one exact score per order for every draw history; the absence of a LUCK
range and the supported effect/condition checks are both premises of this certificate.

`Complete` certifies the first `min(K, number of legal teams)` under this order. LUCK teams use the
[interval ranking certificate](#luck-ranking-certificates), with optional exact fractions and a separately
certified order. [Outcomes and partial results](#outcomes-and-partial-results) specifies the stopping fields
and the availability of whole-domain bounds.

### Traversals

The joint traversal assigns one `(member, optional Snap)` pair at a time in slot order `[2, 0, 1, 3, 4]`, the leader
first. After the first non-leader slot, each slot takes a pair later in a fixed choice order than the previous slot's
pair, so every team is visited once, in one layout. The composition traversal (Live with Gekisou off) branches on the
leader and an unordered set of four other members, then on the Snap pairings of that composition. Leaves evaluate
the canonical layout. Fix the traversal's total choice order. Each team's four nonleader pairs have exactly
one increasing sequence, and the leader is chosen separately, giving one path per legal team. Composition search
applies the same argument to member choices, then enumerates each legal Snap binding. A prefix may fail legality
when it repeats a character or a real Snap, violates the leader restriction, conflicts with a required card
of the same character, or leaves fewer slots than outstanding required members. Each condition rules out every
completion of that prefix. Source: [joint traversal](../crates/ournotes-search/src/search/physical.rs),
[composition traversal](../crates/ournotes-search/src/search/composition.rs) and
[candidate domain](../crates/ournotes-search/src/domain.rs).

### Node bounds

For an assigned leader, the existing `Tables` decomposition bounds each slot's power by
`a[m] + lead[profile][m] + w[m][s]`. Preparation checks nonnegative slot lower bounds and a nonwrapping
whole-deck power domain. Selected pairs contribute their terms. For remaining slots, take a maximum
per character and then the largest required number of character values. Different remaining slots
may reuse a Snap in the relaxation; this enlarges the feasible domain rather than deleting a resource.

The existing Snap Live bound compiler supplies `A0`, `global`, `eps` and each pair's five position gains. Its
supported-domain checks cover nonnegative score factors, conversion/recovery, frame re-execution drift, Gekisou
combo and rank bonuses, and the score overflow ceiling. Class classification is used only to read these upper
bounds: Snap identities remain in search.

### From position gains to a mean score

Write `g_i(k)` for the prepared nonnegative gain of physical pair `i` at skill
position `k`. The per-order envelope premise is

`S(d,o) <= P(d) min(A0 + sum_i g_i(pos_o(i)), G) (1 + eps)`.

The admitted domain supplies `P(d)>=0`, finite nonnegative coefficients and the scorer's nonwrapping score conditions. Every pair occupies each of the five
positions in exactly 24 of the 120 permutations. Thus, with
`gbar_i=(sum_k g_i(k))/5`, linearity gives
`E_o[sum_i g_i(pos_o(i))]=sum_i gbar_i`.
The function `x -> min(x,G)` is concave and nondecreasing. Applying Jensen and
then multiplying by the order-independent nonnegative power proves

`E_o[S(d,o)] <= P(d) min(A0 + sum_i gbar_i, G) (1 + eps)`.

The implemented `mean_up` rounds every addition and division toward an upper
endpoint. Each stored mean therefore dominates its exact real mean. Prefix
power and gain relaxations may attain their maxima on different completions:
if `0<=P(d)<=Pplus` and `0<=A(d)<=Aplus`, then
`P(d) A(d)<=Pplus Aplus`. Multiplying an integer mean cap by 120 gives a cap of
the payoff numerator in the score objective.

Source: [`uniform::mean_up` and `MEAN_ORDERS`](../crates/ournotes-search/src/search/uniform.rs), [`JointBounds::payoff_cap_from`](../crates/ournotes-search/src/search/joint.rs).

### Reward steps, concave majorants and bonus cells

Let `r(s)` be the resolved reward multiplier, including its nonnegative rate,
and let `b` be a team's additive event bonus. In the admitted arithmetic domain,
one order pays `floor((b+10000) r(s)/10000)`. The integer-domain checks establish
the products used by this expression before the bound invokes its monotonicity.
Define `rplus(x)=max_{0<=s<=x} r(s)` using the reachable rank thresholds. This
function bounds each permitted reward even when successive tiers pay less.

For a per-order score cap `C`, retain the thresholds at or below `C` and form the
smallest nondecreasing concave majorant `H_C` of their prefix-maximum steps on
`[0,infinity)`, flat after the final retained corner. The upper hull keeps
nonincreasing chord slopes. Its value at each retained corner is at least that
corner's reward; monotonicity then covers the interval until the next corner.
All outcomes with score in `[0,C]` satisfy `r(s)<=H_C(s)`.

If `E[S]<=M` and every order satisfies `0<=S<=C`, then

`E[payoff] <= ((b+10000)/10000) E[H_C(S)]`
`           <= ((b+10000)/10000) H_C(E[S])`
`           <= ((b+10000)/10000) H_C(M)`.

The first inequality drops only a downward floor, the second uses concavity,
and the third uses monotonicity. Exact integer interpolation and a final ceiling
produce the implemented cap. Its intersection with the per-order maximum
`floor((b+10000) rplus(C)/10000)` is also valid. The choice of the truncated hull
uses the per-order cap `C`, which is stronger information than a mean score cap.
For example, outcomes at scores 0 and 100 with rewards 0 and 100 have mean
reward 50; the score-step reward at mean score 50 may be 0. The majorant
supplies the valid mean inequality.

The per-order cap comes from both the spread above pair means and a placement
bound preserving one pair per performance position. The placement DP has 32
position masks. Each actual ordering appears in that DP or its free-column
relaxation. Their minimum remains a cap of every order's gain sum.

For a prefix, a distinct-character cardinality DP groups completions by their
exact bonus sum. Each cell separately retains upper endpoints for power, gain,
spread and weighted sums. Component maxima within the cell may be independent;
all represent that same bonus. Apply the reward majorant within each cell, then
take the maximum over cells. A physical team's bonus is shared by all orders,
so any order-weighted sum is formed within one bonus cell before this maximum.
The conditional power rule above applies to the cells attaining that maximum.

Multiplayer contexts with signed final scores use a reward cap over the defined
rank domain. A declared monotone room-score policy and the nonnegative-score
checks permit local-score threshold preimages and the corresponding hull.
For capped score, `min(S,t)` is concave and nondecreasing on the signed domain.
For a score-target indicator, its per-order score cap establishes reachability.

Source: [`PointBound::compile`, `mean_payoff`, `node_order_gain`](../crates/ournotes-search/src/search/joint.rs), [`concave_majorant` and `concave_value_ceil`](../crates/ournotes-search/src/search/uniform.rs),
[`bonus_rows` and `bonus_caps`](../crates/ournotes-search/src/search/joint/bonus.rs).

A numeric cap check removes a branch when its numerator cap is strictly below the full Top-K threshold, or the
numerators tie and its power cap is strictly lower. Equality of both retains the branch unless a separate
canonical-key certificate establishes closure, preserving member-ID/Snap-ID tie order.
Exhausting the traversal therefore certifies the same Top-K as exhaustive enumeration of every team.

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
members are relaxed. A golden-section search over `ln(sqrt(lambda))` picks a finite positive scale. Every such scale supplies a
valid inequality, so approximation of the best scale affects only bound tightness. Taking the minimum over
valid scales intersects their certificates. The carrier split likewise retains its uncoupled cap whenever
a positive finite scale is unavailable. Arithmetic rounds outward. See
[correlated caps](../crates/ournotes-search/src/search/joint/lambda.rs) and
[carrier splits](../crates/ournotes-search/src/search/joint/carrier_split.rs).

### Leaf evaluation

A complete team uses [`search/leaf.rs`](../crates/ournotes-search/src/search/leaf.rs). Complete cached
score/life laws and valid recorded order programs supply exact outcomes in their proved context. For remaining
orders, cheap, raw and fine caps provide per-order bounds. The team closes when their sum with the already exact
payoffs satisfies the canonical cutoff.

The remaining simulations form a tree of performance-order prefixes. Before the next unassigned performer acts,
every order below the same node has the same live state. Cloning that state and assigning the next performer
therefore produces the same child state as a complete run of each descendant order. Induction on the five
positions establishes all 120 terminal outcomes; completed orders are stored by their exact permutation.

At an open tree node, the direct cap bounds all its descendant orders. Completed child totals plus bounds of
open and unstarted children provide another cap. Their minimum is valid, and propagating it to the root bounds
the team's complete numerator, including reused orders. The [simulation cutoff](#simulation-cutoff) refines
a node's cap using only settled score frames. A stopped tree retains completed reusable orders; a scalar team
enters Top-K only after its complete aggregate is available. Source:
[order-tree evaluation](../crates/ournotes-sim/src/live/full/orders.rs).

### Global upper bound

`telemetry.proof.globalUpperBound` bounds the best payoff numerator over the whole domain: the larger of the best
payoff found and the bounds of the branches still open. The composition traversal bounds every leader's subtree
before the search and drops a leader's bound when its subtree is done; the joint traversal in descending root-bound
order reads the bound of its remaining root children. The recorded value only decreases. It equals the best payoff
once the search is complete; after a stop it is at most the larger of the best payoff and `upperBound`. Every point
of the incumbent timeline carries the value at that time (`upper`). With several sequential search parts (Gekisou
conversion parts) it is reported only once the search completes.

### Initial decks

`initialDecks` lists up to 100 legal decks of the domain that the search considers before its traversal, for
example the best decks of a heuristic. They use the same leaf contract: a complete evaluation can enter Top-K,
and a pruning certificate can close a team already below the current cutoff. An interrupted evaluation remains
open. Preselected teams preserve the legal domain, objective and canonical order, so every completed search has
the same result. Their effect on the incumbent cutoff can change traversal work and a stopped search's results
and bounds.

With `search-diagnostics`, `search::diagnostics::prefix_upper`, `module_prefix_uppers` and `audit_order_caps` expose
the node bounds, the module bounds and the per-order caps of a team. The harness independently enumerates every team
and checks every prefix against its exact value and power, then compares the entire ordered Top-K (see
[validation](#validation)).

### Evaluation reuse and supplied candidates

Candidate identity caching records a completed evaluation or a sound exclusion. The monotonically improving
cutoff preserves that exclusion on later encounters. An interrupted evaluation stays eligible for future work.
Cache eviction changes the amount of repeated work while preserving domain coverage.

The request-local `TeamScores` cache keys a complete lottery-free score law by the unordered member/Snap pairs
and exact power. Its 120 score/life values are relabelled bijectively into the requested team's slots.
The payoff is recomputed for that team, preserving deck-dependent event bonuses and final-life targets.

The request-local program cache compares the complete performer values and member identities. The request fixes
the chart, play, clocks, scenario and simulator parameters other than power. A stored completed order supplies
an exact score program and its final life; the program is evaluated at the candidate's actual power, and its
payoff is recomputed. Missing orders remain explicit. The program's admission contract is described in
[score programs](score-programs.md). Equality caching of LUCK additionally includes the full payoff mapping.

`initialDecks` and assignment proposals pass through the same legality, leaf-evaluation and cutoff contracts.
They establish useful incumbents before the complete traversal. Their ordering can change the work and partial
results; every completed run optimizes the same legal domain with the same objective and canonical order.

## Prefix bounds

Bounds are evaluated from cheap to expensive. Each row names a point where the search relaxes the problem, the
sound envelope it uses there and the condition that keeps the envelope sound.

| Relaxation point | Sound envelope | Necessary boundary |
|---|---|---|
| Skill windows and weighted notes | Compile weighted interval sums per performance position; node bounds read the mean over the five positions; intersect an own-event trigger with its mission gate. | A trigger cannot invent extra trigger events or move its window. |
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

- Own-event Gekisou factor windows use the performer's event frames and mission gate.
- Cheap prefix bounds intersect distinct-character maxima with unique-Snap capacity bounds for power, gain and
  event bonus.
- Optional correlated envelopes retain the power/skill tradeoff of each pair.
- At surviving leaves, per-note envelopes use the candidate's conversion reach, budgets and combo windows, and
  take the maximum over exclusive judgements.
- PT transforms the per-order cheap and fine score caps of a complete team through all reachable solo reward tiers,
  and node bounds through the concave majorant of the tiers. It does not assume that a higher grade pays more.
  Declared monotone multiplayer room-score policies with nonnegative score bounds admit local threshold
  preimages; other multiplayer contexts use the full defined-rank reward cap.

**Network rank snapshots.** Network rank bonuses read retained controller score snapshots. A timed factor can
still affect notes in its finish score frame before the end command is filed, so the snapshot factor envelope
includes that whole frame, clamped to the declared score clock. The cheap and per-order fine caps use these
windows, and cutoff tables keep the full cap while ranking uses the retained snapshots.

Correlation preparation probes at most 16 leader/pair prefixes, independently of traversal order. Unless the
correlated bound tightens the cheap expected upper bound there by more than 3%, the solver skips that optional
bound and keeps the ordinary complete traversal. The probe is a cost policy, not a proof, and never removes
candidates; diagnostics still audit the correlated bound when the policy skips it.

The coefficient, correlated and node bound checks read position-mean gains; the per-order caps of a complete team
read each order's own positions and are summed with checked integer arithmetic. Numeric cap checks compare these
to the canonical K-th incumbent with strict payoff/power rejection. Equality of both keys retains possible ID
ties unless a separate canonical-key certificate establishes closure.

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

**Candidate suffixes.** Candidate-suffix envelopes precompute component maxima after every position in the
ordered member/Snap choice list. For a surviving prefix, the next slot is bounded by its suffix and the other
remaining slots by a character/resource relaxation. Their overlaps only enlarge the legal completion set. A
suffix whose expected payoff is strictly below the K-th incumbent (or equal with lower power) ends that node's
entire remaining loop. The `tailChoicesSkipped` counter counts pair-loop entries, including illegal pairs, not
distinct complete decks.

**Next pair.** Each surviving prefix also prepares the residual once, excluding the next slot. A constant-cost
per-pair check combines that residual with the proposed pair's prepared power and gain bounds and exact event bonus before
recursing. It allows the residual to reuse that pair's character/Snap, so it remains an upper bound. Only
survivors pay for a fresh, stronger resource-aware prefix scan.

**Table form.** The cheap prefix relaxation is compiled into tables. A prefix fixes at most four characters and
four Snaps. An entry omitted from the five best distinct keys has five retained competitors, at least one
of which remains free after four exclusions; that retained value is at least the omitted value. Thus the
five-entry table preserves the exact free-key maximum. Keys are Snaps for per-character maxima and characters
for per-Snap increments; reusable None retains its explicit availability. The values and floating operations equal the
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

Residual tables depend on leader profile, excluded characters and remaining positions. They are
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

When at most two Snap slots remain, the numeric assignment is solved directly. Fix the other row of an optimal
assignment. It occupies at most one real Snap, so one of a row's two best choices is still available; optional
None stays reusable. Replacing an omitted choice by that available choice preserves feasibility and cannot
reduce weight. Applying the replacement to both rows proves that their two-choice sets contain an optimum. Zero
remaining slots need no assignment call. The general matcher remains the value reference; the fast residual tie
convention is not used as a Top-K certificate.

For PT, when the best-power proposal attains the composition's primary-payoff cap, a small resource DP separately
solves the **power-only** Top-K bindings of the composition's layout. After scanning a Snap, future resources depend
only on the occupied-slot mask. Keeping K partial bindings per mask is therefore exact for power and None-first
identity ordering. To prove the K-entry retention rule, fix a scanned-resource prefix and occupied-slot mask. Every partial
binding in that state has the same possible continuations: the remaining resources and unoccupied slots agree.
Appending one continuation adds the same power and preserves the first differing occupied-slot Snap key.
If a partial binding has K better partial bindings in the state, the same continuation gives K distinct better
complete bindings. Thus discarding it preserves the power-only Top-K, including signed edge increments and
None-first ties. When the domain has fewer than K bindings, all of them survive.

The Snap increments in the validated power table are exact; all member/leader-only terms are
constant across bindings, including a complex leader's fixed-member contribution. The actual power of the last
proposal is read through the ordinary evaluator's power path before using it as the remaining-power cap.

Every proposed binding is evaluated exactly over the 120 performance orders. All unexamined bindings rank after the
last power proposal. The composition may close only when its primary-payoff cap, the remaining-power cap and canonical
lower key cannot beat the updated global K-th result. An equal cutoff key is already evaluated. If fewer than K legal
bindings exist, the DP returned all of them and the composition is exhausted. Otherwise a failed closure
proof resumes the complete Snap traversal, excluding only the identities already evaluated in this layout. Higher
power is **not** assumed to imply higher PT, and a nonmonotone reward table does not justify representative-only
pruning. The cap-attainment condition is a cost policy only: skipping the frontier leaves complete Snap traversal
intact. Compositions without Snaps have one binding and close immediately after its exact evaluation.

Live obtains its incumbents from this integrated assignment stage. Gekisou keeps joint member/Snap traversal with
the PT membership regime above. Both schedules optimize over the same teams and share the fixed evaluator.

## Class and resource envelopes as explicit harness schedules

Two diagnostic schedules separate two independent decisions: an upper-bound effect class in each slot, then the
unique Snap assignment inside those classes. A class is an identity in `JointFineBounds` metadata only. Every
allowed completion remains represented. A constrained assignment proves infeasibility or maximum power; at a
complete class vector the per-note fine bound, summed over the 120 performance orders, applies to every binding
because its inputs are exactly members, classes, positions and an upper bound on power (`JointFineBounds::upper`
projects each choice through `class_of`). Surviving bindings still receive the full evaluation over the orders. There is no representative-only score reuse and no assumption that actual score is
monotone in power.

The resource-correlated cap addresses a different relaxation. For a fixed member composition, write the mean score
envelope as `P*A`, where `A = a0 + sum(mean gain)`. For every positive `r`, `P*A <= (P+r*A)^2/(4*r)`. Each pair's
`power+r*gain` is rounded up to an integer before subtracting a row shift. An exact constrained assignment then
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

Let $C$ be the set of allowed Snaps whose ordinary or Gekisou support rows contain a recognized judgement
conversion (effect 12006 or 13005). A legal team belongs to exactly one of these cases:

1. It contains no Snap in $C$.
2. It contains exactly one Snap $c\in C$. In the total choice order of the domain admitting $c$ and the
   nonconverting Snaps, its unique traversal layout places $c$ in exactly one slot $s$.
3. It contains at least two Snaps in $C$. In the full domain's unique traversal layout, let $i<j$ be the
   first two occupied converting positions in search-slot order.

Case 1 uses the domain excluding $C$. Case 2 retains only $c$ from $C$, forces $c$ in $s$, and excludes
it elsewhere. Case 3 forces converting choices at $i,j$, excludes converting choices at earlier positions
other than $i$, and leaves positions after $j$ free. These cases are disjoint and exhaustive.
The conversion count and unique converting identity are layout invariant; the slot predicates use the
particular traversal's deterministic layout. Each part retains all compatible member/Snap pairings.

All parts share the result collector. Every part is compiled before traversal; when a specialized compilation
is unavailable the original full-domain envelope supplies the traversal. A timeout includes a full-domain cap
for all later parts. This establishes coverage independently of the tightness of conversion reach estimates.

Sources: [conversion classification](../crates/ournotes-search/src/search/snaps/conversion.rs),
[partition construction and execution](../crates/ournotes-search/src/search/physical.rs),
[slot predicates](../crates/ournotes-search/src/search/joint.rs).

### COMBO

**Combo-count ramps.** For an admitted one-shot Gekisou row, a cumulative note score-up that counts the playing range's combo (7001) updates its count
in the skill phase of every frame it runs in, from the playing range's combo after the judgements of the earlier
frames, and files a changed factor at the frame's time; its first factor can be backdated to its trigger time. An
execution ends no later than the time of the frame that processes its end, so an entry reading the factor of an
execution started in frame `f` reads the count of frame `max(f, F)`, `F` the last frame with a time up to the
entry's chart time. Per factor window and entry, the bounds read the largest such count over the window's start
frames whose earliest trigger time is at most the entry's chart time. A frame's count is bounded by the prefix sum
of its playing range's largest per-entry increments over the shortest chart-order prefix holding the entries
judged before the frame; within a run of frames with one playing range it does not decrease. A frame whose playing
range is not an ordered combo range keeps the flat factor. The candidate cap reads the increments of the
candidate's own combo bonus windows, the gain tables those of their carrier level or keyed envelope. Other
lifecycle or numerical cases retain their flat-window envelope.

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
same pair. `λ` is the step of a geometric grid (ratio 1.08) nearest `sqrt((A0 + G)/P)` at the uncoupled terms.
Every finite positive scale gives the displayed inequality; scale selection affects tightness. Zero power, a
nonpositive or nonfinite coefficient, or an unavailable finite positive scale retains the uncoupled cap.
Otherwise the bound of `T` is the smaller of the two. The check stops at the first `T` still above the K-th payoff. Below the leader
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
Unsafe controller effects keep the broad bound; compound trigger groups trigger at their frame's time.

### LUCK

#### Probability law

For a team $D$, let $S_{D,\sigma}(\omega)$ be the final score and $Q_{D,\sigma}(\omega)$ its requested
payoff at order $\sigma$. The certified target is

$$
U(D)=\frac{1}{120}\sum_{\sigma\in S_5}\mathbb E_{\mathrm{nominal}}
       [Q_{D,\sigma}(\omega)].
$$

The nominal model uses independent random inputs for successive lottery/skill draws, with each transition
conditioned on the complete preceding live state. For a lottery history $h$, its possible next results have
probabilities $w_j(h)/\sum_k w_k(h)$, where the weights include the model's integer tables and binary32 buff
conversion. Supported skill probability checks use their binary32 chances. Thus the probability of a complete
history is the product of its conditional transition probabilities.

The uniform-order law and nominal draw law are declared mathematical inputs. The finite-seed APIs define a
separate target by averaging complete post-shuffle random states from a supplied root-seed law. That target
retains the correlations created by the seeded random streams.

Sources: [nominal probability model](../crates/ournotes-sim/src/live/full/luck_dp.rs),
[finite-seed shuffle](../crates/ournotes-search/src/search/expectation.rs),
[finite-seed order assumption](../crates/ournotes-search/src/search/expectation.rs).

#### Enclosing all orders and payoffs

The scorer produces, for every order, a score support interval and an outward-rounded interval containing
the nominal expected score. Its supported lottery-state DP preserves every possible state, and the score
recorder encloses the binary32 command/query schedule and final integer score for all admitted paths.
The supported effect and scheduling checks are premises of this enclosure.

If order $\sigma$ supplies $L_\sigma\leq\mathbb E[Q_\sigma]\leq H_\sigma$, then

$$
\frac{1}{120}\sum_\sigma L_\sigma
\;\leq U(D)\leq\;
\frac{1}{120}\sum_\sigma H_\sigma .
$$

Aggregation validates 120 distinct permutations and uses outward arithmetic. Exact rational metadata is
available when all contributing exact values fit its integer representation; the enclosure remains the
certificate when exact rational metadata is unavailable.

For a step payoff with value $v_0$ followed by thresholds $t_i$ and value changes $\Delta v_i$,

$$
\mathbb E[f(S)]=v_0+\sum_i\Delta v_i\,\Pr(S\geq t_i).
$$

This identity permits signed value changes and therefore covers nonmonotone reward tables. Every reachable
score belongs to one declared payoff interval. Score-target payoffs consume tail probabilities, capped-score
payoffs consume a truncated expectation, and joint score/life targets additionally require a joint event
certificate. Support and first-moment inequalities provide conservative bounds where refined tails are unavailable.

Sources: [LUCK score enclosure](../crates/ournotes-sim/src/live/full/luck_score_bounds.rs),
[order evaluation](../crates/ournotes-search/src/search/certified_search.rs),
[payoff transforms and aggregation](../crates/ournotes-search/src/search/certified_search.rs).

#### Bounded exact refinement

Ranking certification and exact expectation evaluation are separate properties. A team can have a certified
rank while its expected payoff remains an interval. A ranking certificate compares every relevant candidate
and remaining-domain bound; equal-program certificates establish equality before the canonical tie keys apply.
An unresolved overlap is reported as **RefinementRequired**.

After physical-domain exhaustion, the refinement provider considers the overlapping frontier. Its exact tree
replays each prefix in a fresh model, branches over all positive-weight base-point and bonus LUCK outcomes, and
combines terminal (score, life) atoms with exact rational masses. A complete law is installed only after every
branch terminates and its masses sum to exactly one. The provider admits only random draws covered by this tree.

The current admission limits are 32 notes and 512 play frames. One order admits at most 32 branch levels and
32,768 replay runs. All attempted orders share 240,000 replay runs, 8,000,000 replayed frames and the request's
cooperative deadline. Exhaustion retains the previous valid intervals; completed orders can refine the frontier.
The final rank certificate is issued by the interval frontier using the resulting bounds and equality proofs.

For Gekisou with no LUCK range, the lottery-free reduction additionally requires the recorder's effect/condition
checks. They establish that probability-dependent writes affect only unused lottery state and that lottery
score probes remain false; the resulting score, life and declared rank arrivals are identical for every seed.

Sources: [refinement schedule](../crates/ournotes-search/src/search/certified_engine.rs),
[exact-tree limits](../crates/ournotes-sim/src/live/full/luck_exact.rs),
[complete-law construction](../crates/ournotes-sim/src/live/full/luck_exact.rs),
[lottery-free reduction](../crates/ournotes-sim/src/live/full/luck_score_bounds.rs).

### LUCK ranking certificates

For candidate `a`, write `[l_a,u_a]` for its proved payoff enclosure. Candidate `a` ranks before `b` when
`l_a > u_b`, or when `l_a = u_b` and `a` wins the canonical secondary order. A verified equality of complete
program, probability law, power and payoff mapping establishes equal payoffs and permits the same secondary order.
Known exact fractions replace their interval endpoints in these comparisons using exact rational comparison.

An interval candidate is discarded only after `K` distinct candidates have a proved better relation to it.
To certify the next displayed rank, the frontier proves that candidate ahead of every other retained candidate
and of the unseen domain. Repeating this argument establishes the ordered prefix. The unexplored-domain scalar
cap, when available, must be strictly below a candidate's lower bound because it carries no complete tie key.

The integer cutoff uses `t_j = ceil(120*l_j)` for each lower certificate, with exact comparisons correcting the
initial arithmetic approximation. Let `T` be the K-th largest `t_j`. For `U < T`, at least `K` candidates satisfy
`U/120 < l_j <= V(j)`. At `U = T`, candidates with `t_j > T` remain strict witnesses; candidates with `t_j = T`
supply power-tie witnesses only when `l_j = T/120` is proved exactly. If `a` candidates are strictly above, `Q` is
the `(K-a)`-th largest power among those certified ties. If fewer such ties exist, `Q = i32::MIN` disables the
power-tie test. Consequently the rule `U < T` or `U = T and H < Q` always has `K` distinct witnesses.

Refinement intersects existing enclosures with sound tighter enclosures and preserves established exact values.
Its completion criterion is a proved ranking. A proved ranking can coexist with positive-width value intervals.
The returned `rankCertified` distinguishes the proved ordered prefix from the unresolved candidates retained
after it. An unresolved frontier can contain more than `K` candidates.

## Simulation work

### Simulation cutoff

A team whose value cannot reach the Top-K cutoff stops simulating as soon as the part of its score that is already
final shows it.

**Settled frames.** After a processed play frame, define the command horizon `H` as the minimum of the
current frame time, the positive music length, the earliest chart time of any future judged note, the start of
every Gekisou range still before FINISH, and the end of every externally ranked range whose fixed bonus awaits
application. The music-length term covers clamped effect finishes; the range terms cover range-start rewinds
and delayed rank bonuses.

This shrinking prefix is available when every duration-adjustment row (effect 15000) instantiated in the model
has a nonnegative value. The check covers all stored model rows, including those inactive in a particular play.
Other models retain the empty settled prefix `(frame, total, fixed) = (0, 0, 0)`, so their cutoff continues to use
the whole-score remainder bound. This preserves signed duration-adjustment semantics without assuming that a
future adjustment cannot move an effect finish backward.

Let `G` be the scorer's binary32 time-to-frame map and `L` its last addressable score frame. Every future filing
addresses a frame at or above `min(G(H),L)`. The calculator keeps frame `L` open and settles only executed frames
strictly before that minimum and any pending fixed-score frame. Recalculation undoes frames beginning at the
first filed command frame; a direct rewind preserves its own frame. Every later score operation therefore
preserves the settled prefix, and its note and fixed-score sums equal the final sums of those frames. Sources:
[`LiveModel::settle`](../crates/ournotes-sim/src/live/full/mod.rs) and
[`IncrementalCalculator::settle`](../crates/ournotes-sim/src/live/full/scorecalc.rs).

**Cutoff tables.** For one team and performance order the fine cap is split by score frame. Each entry's term
bounds its note's final score whatever happens later, plus a conversion gain when a budget row converts it; an
entry in a settled frame already counts with its final score. A rank bonus is a percentage of its range's entries,
filed at the range end: once that frame is settled it is part of the settled total; before, an unsettled entry
carries its share in its rank factor and a settled one adds its term times the percentage (and that share of its
conversion gain). Each budget row converts at most its count among the unsettled entries and as many among the
settled ones, a bound on its conversions among all of them. The cap after a frame is the settled total plus this
remainder, rounded outward; PT applies the candidate's bonus and reachable reward tiers to it.

**Remainder arithmetic.** The suffix array and conversion heap store upper endpoints of sums of their stored
floating edge values. Adding an edge `v` uses `round_up(U+v)`; removing that exact retained edge uses
`round_up(U-v)`. Both preserve the sum invariant. Each stored edge bounds its candidate contribution, and the
sum of the largest `k` values is nondecreasing in every edge, so heap selection preserves the conversion-budget
cap. Rank-share products and divisions use upward endpoints with nonnegative numerators and positive
denominators. Exact zero remains zero; nonfinite prepared sums make the optional table unavailable. The final
reserve is a multiplier at least one, enlarging an already certified remainder. Source:
[cutoff tables](../crates/ournotes-search/src/search/joint/cutoff.rs).

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
path. The plan preserves trigger cache, phase order, errors and random-number consumption. Controlled performer
relabelling updates both the checker metadata and the plan's member positions (`move_position`), preserving
its applicability predicate. A thread-local switch builds reference models for the idle audit below.

## Validation

Every experiment below compares the search, or one of its bounds, with an independent computation under the same
shared scorer. Agreement establishes search correctness for the declared model, inputs and target; agreement of the
model with the game is established separately ([native validation](native-validation.md)). Counts of audited
prefixes include repeated prefixes; they are checks, not independent samples.

### Proof obligations and implementation evidence

The arguments above establish implications from explicit domain and arithmetic premises. Exhaustive tests
check every candidate of their declared finite domains; focused regression tests exercise individual boundary
contracts. Agreement on those inputs supplies reproducible evidence for the implementation. The global theorem
applies through the stated coverage, evaluation and bound invariants.

| Obligation | Implementation | Reproducible evidence |
|---|---|---|
| Legal configurations and identity-preserving traversal | `domain.rs`, `uniform.rs`, `physical.rs`, `composition.rs`, `team_power_search.rs` | `search_oracle`, `power_team_identity`, `adapter_fixture_export`, session oracle tests |
| Additive, representable power throughout the domain | `tables.rs`, `power.rs`, `mod.rs`, `team_power.rs` | `search::gate_tests`, `search_oracle`, numeric-domain fixtures |
| Complete scalar evaluation and order aggregation | `leaf.rs`, `expectation.rs`, `full/orders.rs` | `expectation`, `search_snaps`, `orders`, fixed-team facade comparisons |
| Strict total ordering, distinct witnesses and interval rank proofs | `topk.rs`, `physical.rs`, `interval_topk.rs` | full ordered oracle comparisons, interval-frontier tests, canonical Snap-ID tests |
| Character/Snap relaxations and exact assignment frontiers | `matching.rs`, `joint/relax_tables.rs`, `joint/bonus.rs` | assignment unit tests and all-prefix harness audits |
| Floating command and coefficient envelopes | `snaps/float_margin.rs`, `snaps/score_windows.rs`, `snaps/fine_view.rs`, `snaps/raw.rs` | operation-count, actual-frame and exact-arithmetic cap tests |
| Irreversible settled prefixes and safe remaining-payoff caps | `full/mod.rs`, `full/scorecalc.rs`, `joint/cutoff.rs` | `orders`, `cutoff_audit`, exact dyadic suffix/conversion tests |
| Reuse preserves evaluation context and every result identity | `team_scores.rs`, `program_cache.rs`, `full/score_program.rs` | relabelling, eviction, partial-order and fresh-evaluation comparisons |
| Declared LUCK law, enclosure and bounded refinement | `full/luck_dp.rs`, `full/luck_exact.rs`, `certified_search.rs`, `certified_engine.rs` | lottery interval and complete-law fixtures, interval ranking tests |
| Closed completion and explicit partial states | `budget.rs`, `physical.rs`, `session.rs` | request-budget, proof-telemetry and session state tests |

`domain.rs` is in `crates/ournotes-search/src`; the other search module paths are relative to
`crates/ournotes-search/src/search`. Simulation paths beginning with `full/` are relative to
`crates/ournotes-sim/src/live`. Integration tests are in the corresponding crate's `tests`
directory. The command below runs the synthetic tests, and the harness commands exercise independently
constructed bounded team domains.

### Tests without game data

`cargo test --release --locked --features search-diagnostics` runs these on synthetic deck data:

- Exhaustive enumeration (`search::oracle::brute_force`) evaluates every legal deck (member set, leader, snap
  assignment) with the regular deck-power path and its own ordering, and is compared with the search on complete
  ordered results for several K, objectives and constraint sets (`crates/ournotes-search/tests/search_oracle.rs`). It shares no bound,
  decomposition or Top-K code with the search.
- Each returned member-set deck is re-evaluated with the regular path (see [member-set outcomes](#member-set-outcomes)); skip and live scores are recomputed with
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
