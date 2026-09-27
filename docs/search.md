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

Every returned deck is evaluated again with the regular deck-power path (and, for the live objective with snap
skills, simulated again); a difference from the search's value is an error, not a result.

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

## Live score without snap skills

The live objective with `exclude_snap_skills` scores a stated play (every judged note with its judgement, life and
combo, and the life in the frame where each skill event fires) with live skills only: Gekisou off and snap skills
excluded. The member at performance position k fires the chart's skill event k; its live skill's effects add factor
commands for a fixed duration. The default play (`Play::theoretical_best`) judges every judged note Perfect, which is
a full combo: the combo a note reads is the number of judged notes at earlier chart times and the life stays at its
base value (a Perfect costs no life in the data; a table where it does is reported as unsupported).

Under these assumptions the leader and the snaps change the live score only through the deck power `P`, and for a
fixed performance order the score is non-decreasing in `P` (each step of the per-note chain multiplies by a fixed
non-negative value, rounds and floors). So a member set's best representative is its highest-power leader and snap
choice with its best performance order. This precondition is checked by the request (`exclude_snap_skills`); the
objective with snap skills is the next section.

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
- `V_e = max_j jp(j) * (1 + N_e) + sum_j jp(j) * J_e(j)`, over the reachable judgements `j` of the entry, `jp(j)` the
  judgement percentage, `N_e` and `J_e(j)` the note and judgement factors whose windows contain `t_e`. The note ends
  with one reachable judgement `x`, and its score-up value `jp(x) * (1 + N + J(x))` is at most `V_e`.

Above the candidates these coefficients use the reachable judgements of every allowed card. For one candidate (a
member set, a performance order and a class for each slot) only its own performers register conversions, and one
triggered by its performer's skill event only at that position's events; the per-entry bound of a candidate uses the
reachable judgements of its own conversions, and the combo breaks that follow from them. They are a subset of the
pool-wide ones, so the candidate's combo counts and percentages are never larger.

Life of a candidate. When no performer of a candidate has a life recovery or guard row that can start, every life the
simulation computes when a note reads its life is at most the base minus the damage already filed at that point (the
entries judged in earlier frames, or earlier in the same frame, the note itself included) with chart times up to the
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
when the life starts at 0, and otherwise at most `x + r - d` (a fold that never reaches 0 adds at most the recoveries
and subtracts at least the damage; one that reaches 0 stays there) and at most `2 * base`. Repeated damage only lowers
the result.

For a candidate whose only life-raising rows are such recoveries (no guard, no recovery with another trigger, none in
the live skills), the search folds, slot by slot, every entry's smallest damage and the candidate's recoveries at their
events, and takes the end `t0` of the first slot at which this fold is 0. An entry at chart time `t_e` reads life 0
when `t0 <= t_e` and every entry at a chart time up to `t0` is judged no later than it: its query folds all those
damages and only recoveries this fold contains, at most as often, so the life it reads is at most the fold's value at
`t0`, which is 0, and the later commands keep it 0.

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

Float margin. `eps` covers `2e-6` for the per-note chain (as without snap skills), `2^-22` for the score-up sum,
`2^-19` for the factor after the floor, and the drift of the running factor state, a binary32 sum. Each float
operation on it rounds by at most `2^-24` of its magnitude. A score frame is executed at most `E(g)` times (its first
run and its re-runs): in each play frame the two recalculations re-execute frames only from the earliest score frame
a command of that play frame can land in, the previous play frame's score frame or the chart time of a note judged in
it; commands after the last note cannot change a note's score. For one candidate deck, every factor command lands in
some score frame `g` of its window; each execution applying it rounds the state (below `1 + F_peak`, the largest total
of the candidate's factors active at one time) and the frame's difference (below the total of the factors whose
windows meet that 40 ms frame), each undo rounds the state once more, and each factor's binary32 value adds one
rounding. The sum of these terms (times 1.01) bounds the state's error; the score-up value it enters is at least 1, so
it is also a relative bound. Above the candidate level the same argument runs with the largest `E` over the live, the
largest command count and the largest factor total of any member and class at each position.

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

Cost. The search simulates at least one deck per member set that reaches the leaves, and every candidate whose
per-entry bound reaches the cutoff. With the default stream the bound is close to the score and the number of
simulations grows with K. Streams with many missed or late notes make the bound looser (combo breaks, conversions and
life are bounded, not simulated), so many more candidates are simulated; a time limit (`TimedOut`) keeps such requests
bounded.

## Validation

- Exhaustive enumeration (`search::oracle::brute_force`) evaluates every legal deck (member set, leader, snap
  assignment) with the regular deck-power path and its own ordering, and is compared with the search on complete
  ordered results for several K, objectives and constraint sets (`tests/search_oracle.rs`). It shares no bound,
  decomposition or Top-K code with the search.
- Each returned deck is re-evaluated with the regular path (see Outcomes); skip and live scores are recomputed with
  the general score calculator.
- The prepared live and skip evaluators are compared with the general calculator on random plays and charts.
- The objective with snap skills is compared with an exhaustive enumeration that simulates every member set, leader,
  snap placement and performance order (`tests/search_snaps.rs`; the `OURNOTES_DECK_SNAPS_*` variables enlarge it), on
  synthetic pools whose snaps extend live skills, add score factors, recover life, guard, convert judgements, count
  judgements and draw random numbers, under the default stream and random streams with late frames, lost life and
  life that runs out.
