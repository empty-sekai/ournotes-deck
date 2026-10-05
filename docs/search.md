# Exact deck search

The played Live/PT recommendation facade searches teams (a leader, four other members and the Snap paired with
each member) under the uniform member-order target: the five members perform in a uniformly random order, and a
team's value is its mean payoff over the 120 performance orders. Its result order and proof contract are specified
in [uniform member-order search](#uniform-member-order-search); the bounds it uses are described after it, and
[validation](#validation) lists the reproducible correctness experiments.
The opening sections describe the canonical member-set solvers and the power/score components that the team
solver reuses. Each route declares its result identity explicitly. Live with Gekisou off uses the
[composition/Snap decomposition](#member-compositions-snap-pairings-and-power-frontiers);
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

## Uniform member-order search

### Target

A team is a leader in slot 2, four other members and the Snap (or none) paired with each member. In a performance
order the five members act at the five skill positions; the paired Snaps follow their members. The target value of a
team is the mean of its payoff over the 120 performance orders, each equally likely:

    U(team) = (1/120) * sum over the 120 orders of payoff(team, order)

Each order is simulated once from the start of the live with the declared judgement stream. The payoff is the final
score (or capped score), or the client event points of that order's final score, rank and life. Power and the
positions of the four non-leader members do not change the value: their slots are a layout, not a decision.

A skill probability check draws a random number. With Gekisou off such a draw ends the request with `Unsupported`.
In a Gekisou live without a LUCK range the probability gates nothing that can run, and each order keeps one exact
score (`"lottery":"noLuckRange"`, below). A LUCK range decides its lottery with random numbers; those decks are
ranked by certified intervals over the native lottery probabilities ([LUCK](#luck)).

### Result identity and shape

`resultIdentity` is `team` (`fixedTeam` for a fixed-deck evaluation). A team is reported in its canonical layout: the
leader in slot 2 and the other (member, Snap) pairs in slots 0, 1, 3 and 4 in ascending member card ID order. Every
layout of the same pairs is the same team, and a fixed deck given in any layout evaluates and reports as that team.
Results are ordered by

1. `expectedPayoff.numerator`, descending (every played-live result has denominator 120);
2. power, descending;
3. member card IDs of the canonical layout, ascending;
4. Snap IDs of the canonical layout, ascending, with "no Snap" before every ID.

Each result carries `expectedScore` and `expectedPayoff` as exact fractions over 120, `scoreSummary` (minimum,
maximum, lower quantiles and the probability of reaching a score target, over the 120 equally likely orders) and
`bestOrder`: the first performance order, in lexicographic order of slot permutations, with the highest payoff and
then the highest score. `bestOrder.performanceOrder` lists the result's slots in performance order,
`bestOrder.members` the member card IDs in that order, with that order's `score` and `payoff`. It reproduces one play;
it is not a decision of the search. `probabilityLaw` is `{"kind":"uniformMemberOrder","orders":120,"lottery":"none"}`.
In a Gekisou live without a LUCK range, a deck whose skills read a probability reports `"lottery":"noLuckRange"`:
without a LUCK range the controller consumes no lottery, the probability gates only lottery chains and
lottery-dependent score-ups, and each order still has one exact score.

`Complete` certifies that the results are the first K teams of this order over every team of the legal domain.
`TimedOut` results are teams with exact values; a better team may be missing, and `telemetry.proof` bounds what the
stop left unexplored.

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
supported-domain checks cover nonnegative score factors, conversion/recovery, frame re-execution drift, Gekisou
combo and rank bonuses, and the score overflow ceiling. Class classification is used only to read these upper
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
rate, and the complete point product against `i32::MAX`. The reward of one order is a step function of its score,
not necessarily increasing. Its prefix maximum over reachable thresholds is at least the reward, and the concave
majorant of that prefix maximum (the upper concave hull of its corners, flat after the last one) is a concave,
nondecreasing function above it. The mean reward over the orders is then at most the majorant at the mean score
cap, again by Jensen's inequality; the majorant is evaluated with rounding up, then the event bonus applies. At a
complete team the per-order caps apply the step reward to each order's own score cap.

A branch is removed only when its numerator cap is strictly below the full Top-K threshold, or the numerators tie
and its power cap is strictly lower. Equality of both retains the branch, preserving member-ID/Snap-ID tie order.
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
members are relaxed. A golden-section search over `ln(sqrt(lambda))` picks the scale; every scale gives a valid bound,
so the search only tightens it. Arithmetic rounds outward.

### Leaf evaluation

A complete team in its canonical layout is evaluated order by order (`search/leaf.rs`). Once the Top-K is full, the
team first gets one cap per performance order: the cheap envelope with that order's position gains, then the raw and
fine per-note caps of that order. If the sum of the caps is below the K-th payoff numerator (or equal with a smaller
power), the team is dropped without simulation. Otherwise the orders run in descending cap order; each simulation
checks the [simulation cutoff](#simulation-cutoff) against the exact payoffs of the orders already run plus the caps
of the orders still to run, and after each order the same total decides whether the remaining orders are needed. A
team that is not dropped is evaluated exactly over all 120 orders.

### Global upper bound

`telemetry.proof.globalUpperBound` bounds the best payoff numerator over the whole domain: the larger of the best
payoff found and the bounds of the branches still open. The composition traversal bounds every leader's subtree
before the search and drops a leader's bound when its subtree is done; the joint traversal in descending root-bound
order reads the bound of its remaining root children. The recorded value only decreases. It equals the best payoff
once the search is complete; after a stop it is at most the larger of the best payoff and `upperBound`. Every point
of the incumbent timeline carries the value at that time (`upper`). With several sequential search parts (Gekisou
conversion parts) it is reported only once the search completes.

### Initial decks

`initialDecks` lists up to 100 legal decks of the domain that the search evaluates exactly before its traversal, for
example the best decks of a fast heuristic. They only fill the Top-K earlier, which lets the bounds prune sooner;
each is visited once as a team, and the result and its proof do not depend on them.

With `search-diagnostics`, `search::diagnostics::prefix_upper`, `module_prefix_uppers` and `audit_order_caps` expose
the node bounds, the module bounds and the per-order caps of a team. The harness independently enumerates every team
and checks every prefix against its exact value and power, then compares the entire ordered Top-K (see
[validation](#validation)).

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
  Multiplayer keeps the unrestricted tier cap.

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

For PT, when the best-power proposal attains the composition's primary-payoff cap, a small resource DP separately
solves the **power-only** Top-K bindings of the composition's layout. After scanning a Snap, future resources depend
only on the occupied-slot mask. Keeping K partial bindings per mask is therefore exact for power and None-first
identity ordering. The Snap increments in the validated power table are exact; all member/leader-only terms are
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
set). The search stops at the first `T` above the K-th payoff. Below the leader the slots to fill take candidates
in ascending choice order, so a node's completions use the choices from its start on, and in a node's choice loop
every child from an offset on, with its completions, uses the choices from that offset on. The tables are compiled for
the pairs from each of a few suffix starts (0, 8, then about half again each time); a node reads the latest start at
most its own, and every 16 offsets the loop bounds all the children left by the latest start at most the offset and
stops when that bound is below the K-th payoff.

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

A LUCK mission decides its lottery with random numbers, so a LUCK chart's outcome is a distribution over draws
rather than one play per performance order. Each deck of such a live gets certified lower and upper bounds of its
expected payoff over the native lottery probabilities (`"lottery":"certifiedNativeLotteryIntervals"`). A rank is
proved only by separated bounds or a verified equal-program certificate; an overlapping frontier ends
`RefinementRequired`, unproven (see [account recommendation](recommendation.md)).

## Simulation work

### Simulation cutoff

A team whose value cannot reach the Top-K cutoff stops simulating as soon as the part of its score that is already
final shows it.

**Settled frames.** After a play frame, a later command lands at a time no earlier than the least of: the frame's
time (skill execution, finish and re-application times; live skill events not yet fired); the earliest chart time
of a note judged in a later frame (note commands, and checker override times taken from later notes); and the
start of every Gekisou range not yet finished (range-start and range-state override times, override times read
from the playing range's judged notes, the solo ranking rewind to the range start and the rank bonus at its end).
Commands file at the score frame of their time and a rewind keeps the frames up to its own, so no score frame
below that horizon is undone or executed again. Those frames are settled: their note and fixed scores are final.

**Cutoff tables.** For one team and performance order the fine cap is split by score frame. Each entry's term
bounds its note's final score whatever happens later, plus a conversion gain when a budget row converts it; an
entry in a settled frame already counts with its final score. A rank bonus is a percentage of its range's entries,
filed at the range end: once that frame is settled it is part of the settled total; before, an unsettled entry
carries its share in its rank factor and a settled one adds its term times the percentage (and that share of its
conversion gain). Each budget row converts at most its count among the unsettled entries and as many among the
settled ones, a bound on its conversions among all of them. The cap after a frame is the settled total plus this
remainder, rounded outward; PT applies the candidate's bonus and reachable reward tiers to it.

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

## Validation

Every experiment below compares the search, or one of its bounds, with an independent computation under the same
shared scorer. Agreement establishes search correctness for the declared model, inputs and target; agreement of the
model with the game is established separately ([native validation](native-validation.md)). Counts of audited
prefixes include repeated prefixes; they are checks, not independent samples.

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
