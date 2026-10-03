# Exact deck search

The played Live/PT recommendation facade searches physical member/Snap assignments under a declared finite
native-root law. Its result order and proof contract are specified in
[joint physical search](#joint-physical-deck-search-under-a-finite-native-root-law); the bounds it uses are
described after it, and [validation](#validation) lists the reproducible correctness experiments.
The opening sections describe the canonical member-set solvers and the power/score components that the physical
solver reuses. Each route declares its result identity explicitly. Live with Gekisou off uses the
[composition/layout/resource decomposition](#member-compositions-physical-layouts-and-power-frontiers);
Gekisou keeps joint member/Snap traversal. Compiled envelopes are derived in
[compiled search envelopes](search-envelope-programs.md).

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
`crates/ournotes-search/src/search/topk.rs` is the only place that defines what makes two results the same.

## Outcomes

- `Complete`: the results are exactly the first K entries of the order above over every legal deck.
- `TimedOut`: every result is a legal deck whose values are computed exactly, but some better deck may be missing.
  Preparation, traversal and final verification share one cooperative deadline. Only completely verified
  candidates are returned. An atomic verification can finish after expiry, but another candidate is not started.
- No legal deck (fewer than five characters, or constraints that exclude every deck): `Complete` with no results.
- `Error::Domain`, `Error::Input`, `Error::Game`, `Error::Capacity`: no results.

Every returned deck is evaluated again with the regular deck-power path (and, for the live objective with snap
skills, simulated again); a difference from the search's value is an error, not a result.

`K = 0` is an input error, including with a zero budget or infeasible pool. A zero budget returns `TimedOut`
without preparing tables or visiting decks. K is a result limit, so requesting more results than exist does not
allocate K entries up front. Cooperative deadlines do not provide preemption, resumability or a browser Worker
step API; these remain separate requirements.

Physical-deck searches can report progress (`engine::recommend_with_progress`, `Progress`). A report is the
result the search would return if its deadline expired at that point: `TimedOut`, with the exactly evaluated
Top-K so far and the telemetry so far. The report's telemetry is closed on a copy, so its proof is not complete and
has no upper bound of the unexplored part, which only a stop computes. Reports are made at the deadline checks and
after Top-K insertions, at most once per interval. Building a report only reads the search state, so a complete
search visits the same nodes and returns the same result with or without reports.

For Skip search, ordering by power additionally requires a proof over the entire feasible power domain. The
solver combines target-aware lower bounds for signed leader effects with required members and distinct-character
constraints, and takes the largest prepared leader bound as its power upper bound. It rejects a domain unless
power is nonnegative and within i32 and every intermediate Skip f32 operation at that upper bound is finite and
nonnegative, with a nonwrapping note sum. This is conservative: a rejected domain need not contain an actual
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

## Joint physical-deck search under a finite native-root law

The `branchAndBound` strategy in the recommendation facade retains physical-deck identity. It assigns
one `(member, optional Snap)` pair at a time in physical slot order `[2, 0, 1, 3, 4]`, fixing the leader
first. Every legal member/Snap pairing remains present. There is no per-card quality cutoff, Snap
dominance deletion, representative-only class matching, or selectable performance order in this solver.

For an assigned leader, the existing `Tables` decomposition bounds each slot's power by
`a[m] + lead[profile][m] + w[m][s]`. Preparation checks nonnegative slot lower bounds and a nonwrapping
whole-deck power domain. Selected pairs contribute their terms. For remaining slots, take a maximum
per character and then the largest required number of character values. Different remaining slots
may reuse a Snap in the relaxation; this enlarges the feasible domain rather than deleting a resource.

The existing Snap Live bound compiler supplies `A0`, `global`, `eps` and each physical pair's five
position gains. Its supported-domain checks cover nonnegative score factors, conversion/recovery,
frame re-execution drift, Gekisou combo/luck/rank bonuses, and the score overflow ceiling. Class
classification is used only to read these upper bounds: physical Snap identities remain in search.

For each declared root, `native_member_order(root)` fixes the map from physical slot to skill position.
It is computed before any deck-dependent Skill/Luck execution. Equal permutations can share an upper
bound; their integer masses are added without changing the root law used by the leaf evaluator.
Assigned pairs use their actual position gain. An unassigned member's relaxed gain takes the maximum
over still unfilled positions and available Snap choices; per-character maxima may use different
cards for gain and power. Both operations enlarge the completion set. Gain sums round upward.
The per-root cap is `ceil(P_upper * min(A0 + G_upper, global) * (1 + eps))`. The search sums integer
caps times exact integer masses, with checked arithmetic. It never ranks by a rounded mean or by
the maximum of sampled scores. The bound does not assume independent skill/luck draws.

For normal-played PT with exactly one held target event, member and Snap bonuses are additive.
Preparation requires nonnegative resolved per-card bonuses, rate and reachable rank values. It
checks `bonus + 10000`, its product with rate, and the complete point product against `i32::MAX`.
The maximum point value is taken over all reachable rank thresholds, without assuming rank values
increase with score. Thus `(bonus_upper + 10000) * rate * max_rank_value / 10000` bounds every atom.
The inherited Live domain checks ensure nonnegative, nonwrapping terminal scores for these thresholds.
The leaf still computes each atom's actual score rank and points before aggregation.

A branch is removed only when its numerator cap is strictly below the full Top-K threshold, or the
numerators tie and its power cap is strictly lower. Equality of both retains the branch, preserving
member-ID/Snap-ID tie order. Traversal visits each legal physical assignment once. Exhausting the
frontier therefore certifies the same conditional physical Top-K as exhaustive enumeration.

Unsupported bound domains, explicit duration/delta-clock overrides, negative bonuses and possible
PT wrapping select exhaustive fallback and report `telemetry.environment.bounds.fallback`. Preparation errors are not
converted into optimistic bounds. Regular request/model validation and exact leaf errors still apply.
`telemetry.joint` (checks and prunes of every bound by depth) and `telemetry.environment.bounds.compileMs` expose
the actual proof work; `telemetry.proof` bounds what a stopped search leaves ([telemetry](telemetry.en.md)).

With `search-diagnostics`, `search::diagnostics::prefix_upper` exposes a per-root prefix cap. The
harness independently enumerates complete physical decks and checks every prefix against its actual
per-atom payoff and power, then compares the entire ordered Top-K (see [validation](#validation)).

## Prefix bounds

Bounds are evaluated from cheap to expensive. Each row names a point where the search relaxes the problem, the
sound envelope it uses there and the condition that keeps the envelope sound.

| Relaxation point | Sound envelope | Necessary boundary |
|---|---|---|
| Skill windows and weighted notes | Compile weighted interval sums per actual native-root position; intersect an own-event trigger with its mission gate. | A probability condition may be assumed successful, but cannot invent extra trigger events or move its window. |
| Member/Snap power and skill tradeoff | Bound a weighted sum of power and coefficient for the same pair, then apply `P A <= (P + r A)^2 / (4r)`, taking the minimum over positive `r`. | Preserve leader context and distinct-character maxima; optional reuse of remaining Snaps only enlarges the domain. Round outward. |
| Unique Snap capacity | Bound member-only contributions plus at most one positive increment per available Snap; intersect with the character-wise bound. | Snap increments must be bounded over every possible remaining member and position, including negative pair differences. |
| Mission ranges | Charge JUST, COMBO, LUCK and rank bonuses only to the notes/ranges that can receive them. | Confirmation replay and boundary frames follow the scorer; a display Fever ratio is insufficient. |
| COMBO saturation | Bound reachable counts using selected combo windows and take the maximum table value over those counts. | Never sum individually measured aptitude deltas: saturation and conversion interactions matter. |
| LUCK/Rush reachability | Restrict the Rush envelope by mission and reachable state, using a root-conditioned replay where dependencies permit. | RNG consumption changes with conversions/skills. Independent marginal expectations are not the native joint law. |
| PT tiers | Turn a score cap into the maximum reward among reachable tiers, then bound event bonus. | Apply per atom; allow nonmonotone reward tables; retain multiplayer total-score rules separately. |
| Exclusive final judgement | Take the maximum of each reachable judgement times its own active judgement-specific bonus, not a sum of Perfect and JUST bonuses on the same note. | Keep conversion reach and budgets conservative; one note can realize only one final judgement. |
| Judgement conversion and life | Candidate conversion masks/budgets and provable life ceilings refine per-note bounds. | Do not infer temporal judgement allocation from a whole-chart Great rate. |
| Conditional dominance | Compare complete effect/context vectors within the same legality class, preserving canonical ties and Top-K alternatives. | Song-level efficiency dominance is not universal card or song dominance. |

Measured chart statistics, such as a fitted base score or per-position weights at one power, are not upper
bounds and never prune the integer scorer.

The bounds in use:

- Own-event Gekisou factor windows use the actual performer's event frames and mission gate; random conditions
  are relaxed to success.
- Cheap prefix bounds intersect distinct-character maxima with unique-Snap capacity bounds for power, gain and
  event bonus.
- Optional correlated envelopes retain the power/skill tradeoff of each pair.
- At surviving leaves, per-note envelopes use the candidate's conversion reach, budgets and combo windows, and
  take the maximum over exclusive judgements.
- PT transforms both cheap and fine per-atom score caps through all reachable solo reward tiers. It does not
  assume that a higher grade pays more. Multiplayer keeps the unrestricted tier cap.

Correlation preparation probes at most 16 leader/pair prefixes, independently of traversal order. Unless the
correlated bound tightens the cheap expected upper bound there by more than 3%, the solver skips that optional
bound and keeps the ordinary complete traversal. The probe is a cost policy, not a proof, and never removes
candidates; diagnostics still audit the correlated bound when the policy skips it.

The coefficient, correlated and fine bound checks never choose a controllable skill order. They are evaluated
under each native-root position map, summed with checked exact integer masses, and compared to the canonical
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

PT under joint traversal first evaluates legal physical seeds from the maximum-event-bonus regime under the
exact declared law, stopping after K results. This is only an incumbent search: if that regime contains fewer
than K decks, the original domain still receives a full search and no membership exclusion is made.

For each member, a uniform per-atom cap combines that member's event bonus, the four largest other-character
bonus maxima, the five largest unique Snap bonuses, and the largest reachable reward multiplier. Pairing,
score-tier feasibility, required-card and leader restrictions are relaxed, so the cap can only be too high. A
member is removed from a private search view only when `cap * totalMass` is strictly below the already
evaluated K-th payoff numerator. Equality is retained regardless of power or IDs. Every excluded completion is
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

Remaining Snap reuse, required-card obligations and remaining skill positions are relaxed. The assigned prefix
retains its exact pairs and native-root skill positions. In each final cell intersect the product envelope with
the three weighted-sum envelopes, then apply the maximum reward among reachable score tiers to that cell's event
bonus. The declared integer root masses combine caps within each bonus cell before maximizing over cells: a
physical deck must use the same bonus across all roots.

The same cells also cap power for ties. Every completion with an exact event bonus has at most that cell's
expected payoff and residual power, so a completion whose payoff reaches the overall cap lies in a cell whose
total is the cap. A branch whose payoff cap equals the K-th payoff numerator survives only when the largest power
among those cells reaches the K-th result's power.

Residual tables depend on leader profile, excluded characters and the set of remaining skill positions. They are
reused only within one compiled bound/domain; used Snaps need not enter the key because residual Snap reuse is
explicit. Limits on states, convolution work and retained cells disable this optional bound rather than dropping
DP states. All numeric equality cases retain the original canonical ranking rules. This layer supplements the
unique-resource and suffix bounds; it does not replace the exact leaf evaluator.

## Member compositions, physical layouts and power frontiers

For Live with Gekisou off, branch on the leader and an unordered set of four nonleaders. Increasing indexes in a
fixed heuristic order enumerate each set exactly once; that order never removes a member. The composition
envelope fixes the leader's native-root skill position, allows each nonleader any nonleader position, and allows
remaining Snap reuse. It combines distinct-character power, gain, bonus and weighted-sum maxima. These statistics
are compiled by leader profile and leader skill position, with a direct calculation fallback when the optional
table exceeds its capacity.

Every surviving member set expands all 24 physical nonleader permutations. This is an enumeration grouping, not a
claim that those permutations score equally. Every root still chooses the actual native performance order. A
fixed-layout Snap search enforces resource uniqueness and bounds remaining power with bipartite assignment.
Numeric ties also use a canonical lower key: the fixed member IDs and None in each unassigned Snap slot. This key
is no greater than any legal completion because None is always allowed and sorts before every Snap ID in the
physical-result contract.

When at most two Snap slots remain, the numeric assignment is solved directly. At most one real Snap can conflict
with the other slot, so each row's two best choices, including optional None, contain an optimal assignment. Zero
remaining slots need no assignment call. The general matcher remains the value reference; the fast residual tie
convention is not used as a physical Top-K certificate.

For PT, when the best-power proposal attains the layout's primary-payoff cap, a small resource DP separately
solves the **power-only** Top-K bindings of a fixed member layout. After scanning a Snap, future resources depend
only on the occupied-slot mask. Keeping K partial bindings per mask is therefore exact for power and None-first
identity ordering. The Snap increments in the validated power table are exact; all member/leader-only terms are
constant across bindings, including a complex leader's fixed-member contribution. The actual power of the last
proposal is read through the ordinary evaluator's power path before using it as the remaining-power cap.

Every proposed binding is scored under the complete root law. All unexamined bindings rank after the last power
proposal. The layout may close only when its uniform primary-payoff cap, the remaining-power cap and canonical
lower key cannot beat the updated global K-th result. An equal physical cutoff key is already evaluated. If fewer
than K legal bindings exist, the DP returned all of them and the layout is exhausted. Otherwise a failed closure
proof resumes the complete Snap traversal, excluding only the identities already evaluated in this layout. Higher
power is **not** assumed to imply higher PT, and a nonmonotone reward table does not justify representative-only
pruning. The cap-attainment condition is a cost policy only: skipping the frontier leaves complete Snap traversal
intact. No-Snap layouts have one binding and close immediately after its exact evaluation.

Live obtains its incumbents from this integrated assignment stage. Gekisou keeps joint member/Snap traversal with
the PT membership regime above. Both schedules optimize the same complete physical domain and share the fixed
evaluator.

## Class and resource envelopes as explicit harness schedules

Two diagnostic schedules separate two independent decisions: an upper-bound effect class in each physical slot,
then the unique physical Snap assignment inside those classes. A class is an identity in `JointFineBounds`
metadata only. Every allowed physical completion remains represented. A constrained assignment proves
infeasibility or maximum power; at a complete class vector the per-note fine bound applies to every binding
because its inputs are exactly members, classes, native-root positions and an upper bound on power
(`JointFineBounds::upper` projects each choice through `class_of`). Surviving bindings still receive the full
native-order simulation. There is no representative-only score reuse and no assumption that actual score is
monotone in power.

The resource-correlated cap addresses a different relaxation. For a fixed member layout and one root, write the
score envelope as `P*A`, where `A = a0 + sum(gain)`. For every positive `r`, `P*A <= (P+r*A)^2/(4*r)`. Each pair's
`P+r*gain` is rounded up to an integer before subtracting a row shift. An exact constrained assignment then
maximizes the sum with each Snap used at most once; None remains independently available only in allowed slots.
Restoring the row shifts and constant `r*a0` gives an upper bound on `P+r*A`. Floating operations round outward
and retain the scorer margin. The minimum over three positive scales intersects the other caps; unavailable
numeric/resource limits skip this optional cap entirely.

Incumbent preparation evaluates maximum-power and weighted power/gain assignments for all 24 layouts of a
completed member composition before deep traversal. These rounded surrogate objectives only propose legal
candidates. Their real values come from the fixed evaluator and never justify exclusion. Local physical identity
sets prevent repeat evaluations even when the global cache is disabled.

The harness exposes `production`, `classes` and `classesWithResource` schedules, audits every class-prefix and
within-class binding-prefix against each oracle completion/root, and compares the full ordered Top-K. Player
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

The reach depends on which converting Snaps a deck may hold, so Gekisou score partitions the physical domain:

- no converting Snap: an envelope compiled for that sub-domain;
- exactly one, `c` in physical slot `s`: the sub-domain keeps `c` as its only converting Snap, slot `s` must take
  `c` and every other slot excludes it;
- two or more: split by the first two slots in search order that hold converting Snaps; both must take converting
  Snaps and the other earlier slots exclude them.

The parts are disjoint and cover the domain; they share one Top-K, so canonical order and tie handling are
unchanged. A forced slot is charged in the cheap relaxation only with its allowed choices; excluded masks filter
enumeration only. All other bounds remain valid unforced relaxations. A failed part compile falls back to one
pool-wide search.

### COMBO

**Combo-count ramps.** A cumulative note score-up that counts the playing range's combo (7001) reads, in the
candidate cap only, the ramp value at an inclusive upper bound of the range's combo count from the candidate's
own combo bonus windows. The first chart time of a range and anything within 100 ms of its start keep the flat
factor (the previous range can still be current there), as do unordered ranges and entries outside combo ranges.
Prefix and gain tables keep the flat windows.

**Gated combo bonuses.** A combo bonus window whose trigger is a sole positive combo-count condition on its own
range counts at an entry only once the range's combo can reach the threshold by the entry's chart time. Its start
needs the threshold counted from judgements processed before that frame; judged in chart order, those are at
chart times up to the trigger time, and the bonus reaches the judgements from the trigger time on. The bound
counts every earlier entry of the range with its largest possible increment and lets each entry at the same
chart time add at most one step, the increment read with every open window included; a gate opened this way
only adds bonus. A window without such a gate, or whose gate names another range, always counts.

**Threshold trigger times.** A sole positive COMBO threshold trigger (7005) gains a necessary trigger time from
prior-frame Good-through-Just processing and the largest exact integer effect-12000 increment of any admitted
five-member deck (conditions, character uniqueness and Snap uniqueness relaxed; a member has its own rows plus at
most one allowed Snap row list). Same-frame skill phases cannot use increments before the controller recount.
Unsafe controller effects and compound trigger groups keep the broad bound.

### LUCK

The optional LUCK replay bounds Rush timing per candidate and root; its admission conditions and the window
construction are in [LUCK Rush timing refinement](search-envelope-programs.md#luck-rush-timing-refinement).

**Replayed Rush spans.** The replay also records, for every completed branch, the half-open chart-time span
`[added, disabled)` of each Rush score bonus command; spans of all branches, and of all roots sharing a
performance order, are merged. A score frame executes a factor command before its notes at the same or a later
time, so a note reads a command's bonus exactly inside its span, and the net bonus at a time is positive only
inside some span (a pair disabled before its addition only lowers it). In the candidate cap an entry outside
every span uses its coefficient without the Rush factor. Unknown spans (an unsupported or failed replay) keep the
factor everywhere.

**Future LUCK programs in prefix bounds.** The last-slot Rush bound holds each future ordered reduced-LUCK
program fixed through all declared root weights. Each physical pair contributes a positive coefficient
decomposition `H + alpha · C(mask)`; no term is subtracted from a previous upper bound. Buckets share exact alpha
bits, retain maxima of power, base gain and nine `power + scale * base` envelopes, and intersect the resulting
AM-GM bounds with an independent product cap. The last variant is maximized only after root weighting. PT
conversion still occurs separately for each root. Buckets bound physical choices; no representative is
substituted in scoring and representatives are used only for reduced replay.

Before replay, the actual last-slot domain is scanned to skip variants with no legal pair after required members,
used characters, used Snaps and slot rules. The numeric buckets themselves still relax these resources. An
unavailable feasible variant prevents a whole-prefix certificate. Pruning uses strict primary improvement, or a
strictly inferior power tie; equal canonical alternatives are retained.

**Command drift.** The command-drift packet of these bounds uses outward positive arithmetic, summed fixed-part
packets and componentwise future maxima. It preserves lifetime operation counts (`3E + 2N`), finite feedback
amplification and multiplicative cross terms. Interval sums subtract lower start prefixes from upper end
prefixes. The pool-wide epsilon can be very loose on LUCK charts and does not stand in for this packet. Nonzero
conversion budgets, unsupported replay, expired budgets or uncertifiable arithmetic disable only the optional
cap; candidates remain.

## Simulation work

### Simulation cutoff

A candidate whose expected payoff cannot reach the Top-K cutoff stops simulating as soon as the part of its
score that is already final shows it.

**Settled frames.** After a play frame, a later command lands at a time no earlier than the least of: the frame's
time (skill execution, finish and re-application times; live skill events not yet fired); the earliest chart time
of a note judged in a later frame (note commands, and checker override times taken from later notes); and the
start of every Gekisou range not yet finished (range-start and range-state override times, override times read
from the playing range's judged notes, the solo ranking rewind to the range start and the rank bonus at its end).
Commands file at the score frame of their time and a rewind keeps the frames up to its own, so no score frame
below that horizon is undone or executed again. Those frames are settled: their note and fixed scores are final.

**Cutoff tables.** For one candidate and native order the fine cap is split by score frame. Each entry's term
bounds its note's final score whatever happens later, plus a conversion gain when a budget row converts it; an
entry in a settled frame already counts with its final score. A rank bonus is a percentage of its range's entries,
filed at the range end: once that frame is settled it is part of the settled total; before, an unsettled entry
carries its share in its rank factor and a settled one adds its term times the percentage (and that share of its
conversion gain). Each budget row converts at most its count among the unsettled entries and as many among the
settled ones, a bound on its conversions among all of them. The cap after a frame is the settled total plus this
remainder, rounded outward; PT applies the candidate's bonus and reachable reward tiers to it.

**Test.** Every few frames the search adds the running root's cap to the exact payoffs of roots already simulated
and the whole caps of roots still to come (later atoms of the same root share its cap), weighted by their integer
masses. The simulation stops only when that total is strictly below the K-th payoff numerator, or equal with a
lower power than the K-th result; otherwise it runs to the end and the candidate is scored exactly. A stopped
candidate cannot enter the Top-K. Orders without a finite cap simulate normally.

### Deterministic idle trigger plan

A condition skill whose rows are all one-shot, without reset, and whose triggers are only the member's own
live-skill event (4010) or a fixed false trigger compiles an idle plan. Compound triggers never qualify, because
short-circuit checks can reset counters or consume skill random numbers. In a frame with no matching event and no
active execution the skill's triggers are pure false: the plan fills the trigger cache as the checks would and
skips condition, release, cumulative and applier work. Active and end-frame-pending instances keep the original
path. Trigger cache, phase order, errors and random-number consumption are unchanged, and effect metadata is
immutable after construction, so a compiled plan cannot go stale. A thread-local switch builds reference models
without the plan for the idle audit below.

## Validation

Every experiment below compares the search, or one of its bounds, with an independent computation under the same
shared scorer. Agreement establishes search correctness for the declared model, inputs and finite root law;
agreement of the model with the game is established separately ([native validation](native-validation.md)).
Counts of audited prefixes include repeated prefixes and duplicate root atoms; they are checks, not independent
samples.

### Tests without game data

`cargo test --release --features search-diagnostics` runs these on synthetic deck data:

- Exhaustive enumeration (`search::oracle::brute_force`) evaluates every legal deck (member set, leader, snap
  assignment) with the regular deck-power path and its own ordering, and is compared with the search on complete
  ordered results for several K, objectives and constraint sets (`crates/ournotes-search/tests/search_oracle.rs`). It shares no bound,
  decomposition or Top-K code with the search.
- Each returned deck is re-evaluated with the regular path (see Outcomes); skip and live scores are recomputed with
  the general score calculator.
- The prepared live and skip evaluators are compared with the general calculator on random plays and charts.
- The objective with snap skills is compared with an exhaustive enumeration that simulates every member set, leader,
  snap placement and performance order (`crates/ournotes-search/tests/search_snaps.rs`; the `OURNOTES_DECK_SNAPS_*` variables enlarge it), on
  synthetic pools whose snaps extend live skills, add score factors, recover life, guard, convert judgements, count
  judgements and draw random numbers, under the default stream and random streams with late frames, lost life and
  life that runs out.
- The recommendation facade compares `branchAndBound` with `exhaustive` on synthetic physical domains: full ordered
  Top-K with resource constraints, nonmonotone PT tiers, composition frontiers with K above the binding count, the
  PT membership regime with strict ties, and the fallback for wrapping PT and explicit clocks
  (`crates/ournotes-search/tests/adapter_fixture_export.rs`).

### Physical-domain oracle

The search harness ([tools/search-harness](../tools/search-harness/README.md)) enumerates a bounded physical
domain independently of the production pool resolution, enumeration, assignment, bounds, cache and Top-K, and
evaluates every candidate with the fixed evaluator under every root atom. A case passes when the ordered Top-K of
every experiment equals the oracle's, every returned deck agrees with the fixed evaluator, and every audited cap
is at least the actual per-atom payoff (and power) of every oracle completion it covers: joint prefixes, Rush
prefixes and leaves, character and resource prefixes, raw-judgement leaves, classes and class bindings,
compositions and layouts, bonus and expected-bonus prefixes, per-member PT caps, suffixes and next pairs. Any
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

- `cutoff_audit` settles after every frame of a complete simulation for each deck and distinct root, and checks
  that no settled score frame is undone later, that every settled total equals the final scores of its frames, and
  that every cutoff cap is at least the final score. Roots without a finite table are reported as unavailable.
- `slack_profile` reports, per root, the candidate's fine cap beside its actual simulated score and per-note
  scores; the cap must not be lower than the score. The per-entry terms are attribution, not bounds.
- `rush_audit` compares full-engine Rush probes, plain full simulation and reduced replay: the replay masks must
  include every Rush state the full engine observes, and replays with the same ordered signature must agree.
  Unsupported roots and admission criteria are reported separately from violations.
- `idle_audit` evaluates each deck with and without the idle trigger plan and requires identical outcomes,
  including every root atom, order, life/conversion observation and non-timing counter; `--search` repeats this
  for a whole deterministic search.
- `score_program` compares recorded score programs with fresh complete simulations at a list of powers
  ([score programs](score-programs.md)).

### Independent PT plateau certificate

When the global PT ceiling is already attained by K incumbents, `pt_power_certificate` exports model bonus values
and complete per-slot power matrices, and `pt_certificate.py` independently resolves the PT ceiling and runs a
Top-K DP over the five-slot subset mask while scanning each Snap exactly once. It enumerates every allowed physical
member layout and preserves None-first ties, without production pruning or Hungarian assignment; the
power-leading assignments are replayed under the original root law and their serialized results compared with
the production Top-K. The certificate applies only when exactly five members remain eligible under the
independent member caps and those assignments attain the ceiling; other inputs are reported as inapplicable.

### Native and WebAssembly outcomes

`tools/search-harness/browser.cjs` runs the recommendation package in a Chromium Worker on original UTF-8 inputs
and compares every semantic outcome field with a native reference outcome, keeping JSON number tokens as strings so
integers beyond JavaScript's safe range cannot compare equal by rounding.
