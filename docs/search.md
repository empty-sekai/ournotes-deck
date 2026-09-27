# Exact deck search

This document states what the search returns and why its pruning never removes a deck that belongs in the result.

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

Each term of a slot is a whole number of points on each stat (a multiple of 10 000 in the fixed-point unit), so the
deck power, `sum over stats of floor(sum over slots / 10000)`, equals the sum of the slot totals exactly. The search
checks the ranges that keep this identity (non-negative stats and percentages, no 64-bit or 32-bit wrap) when it
builds its tables and reports `Error::Domain` otherwise. The positions of the four non-leader members do not change
the power; the leader choice and the snap pairing do.

A leader profile is *simple* when none of its effects has a condition group or a cumulative count: then
`leaderPercent_L(m)` depends on `m` alone. Otherwise (conditions on the other members or on the song, counts over the
deck) it depends on the whole member set; the search evaluates it exactly for every complete member set and bounds
it before.

## Result identity and order

One result per set of five member cards. Its representative is the best deck with those members under this order,
better first:

1. objective, descending;
2. deck power, descending (for score objectives);
3. the five member card ids, ascending, compared as a sorted list;
4. the leader card id, ascending;
5. the snap ids in slot order, ascending, with "no snap" after every snap;
6. the performance order, ascending.

The non-leader members sit in slots 0, 1, 3, 4 in ascending card-id order. The function `identity` in
`src/search/topk.rs` is the only place that defines what makes two results the same.

## Outcomes

- `Complete`: the results are exactly the first K entries of the order above over every legal deck.
- `TimedOut`: every result is a legal deck whose values are computed exactly, but some better deck may be missing.
  The flag is set by the search when it stops at the deadline; it is never inferred afterwards.
- No legal deck (fewer than five characters, or constraints that exclude every deck): `Complete` with no results.
- `Error::Domain`, `Error::Input`, `Error::Game`, `Error::Capacity`: no results.

Every returned deck is evaluated again with the regular deck-power path; a difference from the search's value is an
error, not a result.

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

## Live score

The live objective scores a stated play (every judged note with its judgement, life and combo, and the life in the
frame where each skill event fires) with live skills only: Gekisou off and snap skills excluded. The member at
performance position k fires the chart's skill event k; its live skill's effects add factor commands for a fixed
duration. The default play (`Play::theoretical_best`) judges every judged note Perfect, which is a full combo: the
combo a note reads is the number of judged notes at earlier chart times and the life stays at its base value (a
Perfect costs no life in the data; a table where it does is reported as unsupported).

Under these assumptions the leader and the snaps change the live score only through the deck power `P`, and for a
fixed performance order the score is non-decreasing in `P` (each step of the per-note chain multiplies by a fixed
non-negative value, rounds and floors). So a member set's best representative is its highest-power leader and snap
choice with its best performance order. This precondition is checked by the request (`exclude_snap_skills`) and has
to be revisited when snap skills or Gekisou are modelled.

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

## Validation

- Exhaustive enumeration (`search::oracle::brute_force`) evaluates every legal deck (member set, leader, snap
  assignment) with the regular deck-power path and its own ordering, and is compared with the search on complete
  ordered results for several K, objectives and constraint sets (`tests/search_oracle.rs`). It shares no bound,
  decomposition or Top-K code with the search.
- Each returned deck is re-evaluated with the regular path (see Outcomes); skip and live scores are recomputed with
  the general score calculator.
- The prepared live and skip evaluators are compared with the general calculator on random plays and charts.
