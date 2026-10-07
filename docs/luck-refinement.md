# Exact nominal LUCK refinement

The refinement provider evaluates the complete probability law for one specified performance order under the declared independent nominal LUCK draws. Each semantic draw uses the integer weights produced by the calculation model. Duplicate outcomes with the same result share their combined mass.

## Nominal skill probabilities

The provider also branches at each native `4011` probability check. Its success mass is the exact finite,
clamped binary32 rate stored by the condition constructor, after the original integer-to-float conversion
and division by 100. This is the same independent nominal skill-probability model used by the controller
DP. The two exact integer weights sum to their common denominator; a rate whose reduced denominator does
not fit the bounded integer representation, or a nonfinite rate, declines refinement.

The semantic draw stays at the original condition-evaluation point. The original trigger, condition,
release, short-circuit and reset order therefore decides which draws occur. Rates zero and one still
consume one native draw, while requiring no nontrivial branch-prefix choice. All other unmodelled raw
random calls continue to decline the complete law. Existing branch, arithmetic, work and cancellation
limits apply to skill branches as well as lottery branches.

Seeded playback continues to compare its original skill-stream random float against the original rate,
with identical stream state and draw consumption. The independent nominal law makes no claim about the
joint distribution over the finite seed space or the discretization of seeded random floats.

## Frame checkpoints

A pending branch contains a selected outcome prefix, its exact rational mass, a complete model checkpoint and the index of the next frame. Checkpoints are taken before a frame after a bounded number of completed frames. Sibling branches share an immutable checkpoint and clone it for execution.

A continuation extends the checkpoint's selected outcome prefix while preserving its consumed-choice cursor and draw counters. Frames before the checkpoint already have the state produced by that prefix. Replaying the suffix therefore gives the same state transitions as playback from the beginning. An interrupted frame is replayed from the latest complete checkpoint, including every action before the draw within that frame.

Each discovered draw partitions a pending branch into mutually exclusive outcomes whose masses sum to the incoming mass. Rational multiplication assigns each child its conditional mass. Terminal paths with the same score and final life are summed. The provider publishes a law once all pending branches terminate and the accumulated mass is exactly one.

## Equal initialized models

`LuckExactSession` borrows one immutable master, chart, frame schedule, timing
sequence and rank timeline. It compiles each performer order into a full initial
model, including formation predicates and cumulative counts. Orders with equal
initialized state share a complete terminal law in this fixed context.

The in-process identity contains the complete derived model state. The two score
lookup tables have sorted entries; other model maps use deterministic hashing.
Finite floating values retain their round-tripping representation, including
signed zero. Empty note/factor frame lists and arrays of bitwise-positive-zero
score diffs have a lossless representation containing their lengths. Nonempty
lists and any nonzero or negative-zero diff retain their complete contents.
The calculator and diff views destructure every field exhaustively, so adding
a field requires updating the identity. NaN-bearing and opaque states use
independent evaluation. Identity
construction is bounded to 512 KiB. The session retains at most 64 complete laws
and 32 MiB of identity and atom payload, evicting the oldest entries at capacity.

Every retained law has already passed the exhaustive traversal and mass-one
check. A hit therefore consumes zero additional replay segments or frames and
is available even when the execution allowance is exhausted. Cancellation is
checked before either replay or reuse. A session with zero entries evaluates
each order independently.

## Complete recorder identity storage

Within an immutable `LuckScoreSession`, a complete recorder key identifies its resolved rows, checkers,
performer positions, effect ordering, life-interpreter inputs and probe plan. Completed lottery curves can
be reused for equal recorder keys before another recording pass.

The cache stores the first retained complete raw key as an immutable byte dictionary. A later key retains
its original length and either its complete bytes or every consecutive run of bytes that differs from the
dictionary at the same positions. Reconstruction is exact: omitted positions equal the dictionary by the
encoder's comparison, and every other position is explicitly retained. Extending or shortening the key
preserves its original length and all necessary suffix bytes. Thus `decode(base, encode(base, key)) == key`;
no input field or floating-point representation is discarded. Lookup checks full reconstructed equality
after a hash prefilter.

Dictionary and key payloads use boxed slices. Their byte lengths and the entry buffer's actual allocated
capacity share an allowance of at most one MiB and 128 entries. Shared probability objects are not included
in this recorder-key storage allowance. The dictionary is independent of its original entry's lifetime;
it remains unchanged while entries use it. Eviction removes individual oldest entries, and duplicate
insertion preserves the existing certificate without charging it twice. Only the existing completed
recording and propagation paths insert entries. Cancellation, refusal, zero capacity and unrepresentable
or oversized storage keep independent evaluation available.

## Request-shared reduced recordings

`LuckDpCache` can reuse a completed reduced recording across score sessions after each caller passes the
original preparation, row/checker compilation and optional-life checks. This reuse requires both the
optional life interpreter and its performer input to be absent. Preparations with either present keep the
existing session-local recording and propagation path.

The identity combines the unchanged complete recorder key with an owned scope. The key contains the
resolved rows, ordered nonempty condition updaters, Gekisou appliers and controller tables/state, probe
metadata, and ordered actions with their probability and activation-time bits. The scope contains every
external note, event, setup, play frame and judgement, seed, delta-time bit, rank arrival and non-power
parameter, together with the complete remaining initialized model. It retains full initial life and random
state, score-frame shape, calculator tables and conversion metadata. Lists and options preserve lengths,
order and presence; floating scope inputs preserve their bits. NaN-bearing or opaque identities decline
shared reuse. Hashes only prefilter complete key and scope equality.

The reduced interpreter performs no further master lookup: its prepared rows, tables and initial state,
plus notes, play and deltas, determine every recording step. Initial total power can be normalized to zero
because the score-calculator constructor stores it only in the initial power field, which this interpreter
never reads. Its before/after controller calls supply literal score zero; admitted score appliers only
file factor commands. Frame shape and all other filing inputs remain scoped. Equal identities therefore
produce equal complete transcripts and certified curves, including their probe metadata. Candidate score
and ranking retain their separate proofs. Full initial life state remains necessary even without a life
interpreter, because a retained native checker may still read life.

A private, immutable `LuckScoreSession` memoizes one bounded owned scope at its current allowance. The
current reduced constructor places every deck-dependent initialized value in the original recorder key;
this is the additional condition that makes the memo independent of performer order. A future
deck-dependent residual field must extend that key or disable the memo. Full initialized-state formatting
alone does not prove that memoization condition. The first cross-session comparison checks all owned bytes
before interning equal scopes; later pointer equality refers to that retained immutable allocation.
Direct cache calls without a session construct their own owned scope. A changed scope misses and replaces
the table only when a complete result is inserted.

The shared table has an independent allowance of at most 1 MiB and 128 entries. It counts its retained
scope, dictionary and delta-key buffers, actual entry-buffer capacity, and each distinct retained curve
allocation and its vector capacities once. The existing session-local table keeps its separate 1 MiB,
128-entry key-storage allowance; both tables may retain a key while sharing the same curve allocation.
These component budgets exclude allocator metadata and do not specify total process memory. Duplicate
insertion preserves the existing curve and accounting, and zero capacity clears shared reuse.

Only an already completed transcript-cache result or successful complete propagation can create a shared
entry. Cancellation is checked after preparation and immediately around lookup, before a hit is counted
or returned. Interrupted or refused recording/propagation creates no entry. Scope, key and capacity
refusals keep independent evaluation available; cancellation remains cancellation.

## Certified summary reuse

`LuckScoreSession` fixes the master and classified skills, chart, parameters,
frame and timing schedule, and rank arrivals. Before weighting or replay, it
can identify the complete initialized model with the same lossless identity.
Equal initialized models in this context have the same nominal terminal law.
Any completed all-path score, support and life certificate for one therefore
also encloses the other. Only completed summaries enter this session cache.

The cache retains at most 64 summaries and 8 MiB of identity and result payload,
further limited by the supplied curve-cache byte allowance. A zero allowance or
no curve cache clears and disables summary reuse. Cancellation is checked
before initialization and before returning a retained certificate. Every
performance order still contributes its own equally weighted, distinct label
to the 120-order aggregate. A summary hit skips recorder and bound replay;
actual probability propagation counters count only work that executes.

Recorder admission also permits deterministic filtering of possible lottery filings.
Before the frame, only a range at `COMPLETE` can disable Rush while entering
`FINISH`; this check includes every mission, and actual weighted-recorder factor
commands remain recorded. After skills, pending target notes are captured before
the controller takes them, eligible current LUCK target judgements remain possible,
and every playing LUCK range retains its pending-draw opportunity. These guards
never inspect the recorder's lottery counts or results: multiple consumes can
switch Rush on and off within one frame. Direct 7021 score probes retain their
possible transitions whenever their common native mission gate is open. A closed
gate preserves the previous probe class; it does not force the effect off. Open
gates retain both the frame-time filing and an end clamped to the music length.
All score queries and probability-readiness events remain in their original order.
Without this admission, recording uses the unrestricted possible-filing schedule.

The factor replay also has exact identity transitions. A query of the current
score frame with no mandatory or possible lottery filing changes no replay
state. A frame with no ordinary float commands and no probe filings leaves
the factor and probe classes unchanged, records zero diffs, and immediately
undoes to those same factor values. Its notes still retain or join their
executed-state certificates according to whether every path executes that
frame. Later filings and rank rewinds use the ordinary replay rules.

For an immediate paired undo, probe branches write only the note-score-up field.
The other five fields see the same ordered ordinary commands on every branch.
Their end state and recorded command sum are therefore evaluated once from each
original start-class endpoint. The replay enumerates the note field separately,
keeping the start class, current class and exact binary32 state and sum in each
path identity. Taking the per-field hull gives the same enclosure as the full
vector path traversal: each endpoint admits every probe branch, and each field's
projection preserves all its possible values. Mixed positive and negative zero
endpoints use the full traversal to retain its min/max visitation order; nonfinite
results also retain its original refusal path. Differential tests compare every
endpoint bit with that full traversal across all six fields, tied-owner command
orders, probe switches, signed zeros and overflow cases.

## Private structural score recording

The bounds evaluator and upper preparation can omit numeric note and factor execution from their private
deterministic recorder. This applies only after recorder admission to a fresh solo model with an actual LUCK
range and empty weighted recording. The recorder still runs the original controller, conditions, life,
judgement conversion, command filing and frame schedule. It preserves every original Query ordinal, the
complete Combo observations with their binary32 bits, lottery-readiness events, and the frozen inputs of
every filed note.

Admission additionally excludes external ranking, public native-score observers, score-program capture,
minimum-score-up tracking, settled prefixes, raw frame hooks and previously started models. The discarded
private score and range-score snapshots are not observations: solo rank and its percentage are independent
of those amounts, and the later replay reconstructs every numeric rank bonus from the original query
identities. The optimized model cannot be returned or resumed as a native score model. Free Live,
Gekisou without LUCK and contexts declined by this additional gate keep their existing recorder path.

The structural calculator keeps the original rewind and execution interval, pending fixed-score overwrite
and filing order, raw fixed-frame identities and duplicate-frame error. A malformed note is remembered at
filing and fails at the first Query that would execute it, using the native frame, time, note-ID and insertion
order. Missing note or judgement percentage errors therefore still precede a duplicate pending fixed-frame
error when native execution would encounter them first. Allocation failure remains a refusal.

These checks rely on immutable note and judgement percentage maps throughout the private recording.
Fresh Combo-table shape and the original ComboCounter construction and judgement updates establish that
the omitted duplicate Combo reads cannot fail or mutate later observations; the complete original
ComboObserver path remains in place. A new writer of those maps, a new numeric observer, or a new fallible
numeric primitive requires a corresponding admission/checking proof before using this path. Unknown cases
keep the native recorder.

Recording alone supplies no numeric score, support, probability law or completion certificate. The existing
random-draw, FINISH, query-count and probability-readiness checks still apply, followed by the independent
probability and native-arithmetic enclosure. Differential tests compare every trace event and floating-point
bit against a recorder that executes native scoring, and compare all remaining controller, condition, life
and conversion state. Separate cases check error order, fixed-frame identity, changing Combo inputs and
observer refusals.

## Complete controller families

`LuckFamilyContext` and `LuckControllerFamily` provide optional expected-score node bounds with a complete
controller law for a declared family of physical bindings. The context borrows the exact master tables,
compiled skill catalogue, chart, events, parameters, Gekisou setup, play inputs and delta clock for its lifetime.
The search also holds the exact compiled reward template. A compatible DP curve alone is not this authority.

Context admission checks the complete causal, first-due judgement stream, unique chart-note identities,
nonnegative clocks and bounded native frame geometry. Each judgement note is judged once at its declared
time; late, repeated or omitted required inputs decline. Structural chart nodes omitted by the theoretical
stream remain in the full native note domain, range targets and end/score-frame checks. They need no declared
judgement; an explicit structural-node result is outside this capability. Every active range must reach
FINISH before a final empty frame. The
terminal mapping records the original note-occurrence multiset and checks the Query count, a last Query
whose prior probability-readiness covers its notes, and no pending rank. Solo ranking is required. Binding
the reward template later requires equality of the complete sorted note-time multiset. These checks establish
the mapping for the declared input stream rather than generalizing one scored candidate's terminal trace.

Family preparation fixes five complete members and their leader, and admits every allowed physical Snap choice
on every owner, including no Snap. Repeated resource IDs must have identical ordered support-source vectors
across owners; a slot may not list the same resource twice. Every pair's full native model passes recorder
admission and whole-model checks for effect identities, lifetime and handle limits, and the conditions needed
for numeric and runtime errors to remain observable. After reward-only rows are projected away, the complete
reduced controller program must also pass admission for its retained native phases, triggers, conditions,
releases, resets and probability values. Its law uses the declared independent nominal draws, including the
[skill-probability model](#nominal-skill-probabilities).

The admitted family can contain at most one physical Snap resource that writes the LUCK controller. Its
profiles are absence of that resource and each allowed owner of it; fixed member writers remain present.
Each retained writer must have the LUCK mission gate of its actual selected native updater. A support
writer therefore keeps the selected member's mission and support-activation metadata; its effect type alone
does not establish the gate. Writer triggers, conditions, resets and releases must not read life, ordinary
Live/support sources must not write the LUCK controller, and unsupported controller phases retain the
ordinary scoring path. Negative ordinary duration
extensions and timed sustained ordinary queues decline. Effect `15000` extends ordinary Live effects only;
it does not extend Gekisou writer lifetimes or justify dropping a possible judgement-conversion edge.

The union of every pair's possible conversion edges is closed transitively. Every judgement reachable on a
LUCK note must stay in its original controller class, and every reachable judgement must have the native
parameter rows needed by execution. Conversion rows that share the native memo's raw row identity must have
the same target vector across the entire pair domain. An unreachable conversion may be omitted only under
the separately checked late range-playing condition; short unextended windows or one observed execution do
not establish that condition. Unknown identity, conversion or life dependencies decline the family.

Each profile is evaluated in all 120 original member orders, with complete initialized performers and the
original source order at each performance position. A coverage bitmap checks every profile/order pair;
duplicates cannot stand in for missing pairs. The returned four buckets are the joint virtual direct-LUCK
score-probe bit and native Rush bit at a note's chart time. The virtual probe bit is meaningful even when no
member holds a probe, because complete program admission establishes its controller meaning. Projecting
reward rows preserves the complete member fields and their native mission/support-selection metadata.

For each profile the reward template averages coefficients over all 120 original orders, using each member's
actual position in each order. Only after these complete averages does it take a component maximum across
profiles. A fixed physical binding chooses one profile before the order draw, so linearity and these maxima
bound that binding's expectation. Existing all-history rank, conversion, additive drift and unclassified-window
allowances remain unweighted. The result is an upper only for Score; it supplies neither per-path support nor
the expectation of a nonlinear payoff.

The search's preparation limits are 4,096 pair models, at most six profiles and 720 order/profile evaluations,
16,000,000 units of declared frame work, and 32 MiB of accounted family storage. The frame-work check includes
the terminal mapping and every required order/profile. The retained coefficient-table cache separately uses
at most 64 entries and 8 MiB, further limited by `cacheEntries`. Container capacities, retained mappings and
referenced probability payloads are accounted by their respective owners; these budgets are not process RSS
or the combined memory of every cache.

A successful family requires complete coverage within every limit. Capacity failure, unsupported input or a
local work budget produces an unavailable optional bound; cancellation produces a stopped preparation.
Neither supplies a partial family or an optimistic maximum over only the completed profiles. Completed
individual probability curves may be reused through their existing exact recording keys, but the family cache
stores only complete coefficient tables or refusals in its immutable scope. Search completion and canonical
Top-K still require the ordinary whole-domain proof.

The [synthetic family oracle](../crates/ournotes-sim/src/live/full/luck_score_bounds/family_tests.rs) enumerates
legal physical bindings, all 120 orders and native nominal probability branches, comparing the joint masses
and checking total probability. [Admission cases](../crates/ournotes-sim/src/live/full/luck_score_bounds/family_admission_tests.rs)
cover cross-source conversion identity, selected mission gates, clock and extension boundaries. The
[node oracle](../crates/ournotes-search/src/search/joint/family_node_tests.rs) checks every legal descendant
against actual depth-four suffix and resource masks, including complete refusal and cancellation behavior.
An [end-to-end test](../crates/ournotes-search/src/search/joint/family_node_e2e_tests.rs) checks canonical Top-K
with shuffled public IDs and the cache enabled or disabled, and checks that exhaustion of the candidate
budget retains its unproven state. These finite checks complement the admission and upper-bound argument above.

## Expected-score upper preparation

Before full factor replay, a Score candidate with a valid K-th cutoff can prepare each order's terminal joint
Rush/probe law. `LuckScoreSession::rush_cap_preparation` returns `Ready`, `Unavailable` with a fixed refusal cause,
or `Stopped`. It requires the same admitted recorder and DP, completed ranges, a ready last query and no pending
rank bonus. The returned capability can supply an optional native whole-score expectation upper or weight a
caller-proved fine-cap decomposition. Neither supplies a score-support object, exact law or candidate value.
External ranking and unknown terminal mappings retain full scoring.

`LuckTerminalRush::probe_gate()` grants `Some(2)` only for held direct probes under the completed recorder's
common LUCK gate. `weighted_note_upper` changes only the native Rush multiplier. `weighted_note_bucket_upper`
can additionally weight matched ideal probe amplitudes in all four joint classes. Without that gate authority,
the latter requires equal caps across the probe bit. Both methods preserve the terminal note-time multiset and
sum the certified joint masses directly with outward arithmetic; no marginal product or independence assumption
is used.

The fine-bound caller matches only positive note-only direct 7021 effects with the admitted untimed sustained
shape and complete class-signature provenance captured at construction. Ordinary or unmatched windows, combo
and life caps, all native command-history drift, historical rank allowances and conversion gains remain.
If an eligible conversion-budget row adds a target outside the existing non-budget judgement mask, that note
keeps its original cap in every bucket, including zero-gain rows: a difference of native floors can increase
when either multiplier decreases. Already-contained targets leave the exact mask unchanged in every bucket
and need no such protection. The full combo-break mask remains unchanged.
The [search proof](search.md#terminal-rush-caps-for-expected-score-exclusion) specifies the conditional note caps
and the exact unchanged remainder containing rank, conversion and rounding terms.

The completed recorder can optionally supply `note_score_up_upper` for that exact terminal note multiset.
It combines each note's actual ordinary native-delta prefix with a drift certificate derived from every query
and possible filing, without constructing interval factor histories. A late preceding ordinary command forces
a terminal note to execute again, so a retained old execution cannot omit such a command. Checked execution and
undo counts bound both state and stored-diff roundoff, including subnormals; a finite feedback and magnitude
check rejects possible overflow. Untimed inverse probe rows contribute an unconditional bound across all
nondecreasing filing-time runs. Distinct probe-off/on amplitudes additionally require the common LUCK gate,
nonnegative rows and at most one run.

This optional route requires zero initial floating combo-score adjustment and no nonzero floating combo-score
or power command. Its field bounds include the initial one and full history drift. The caller retains the
existing combo, judgement, life, native-floor and chain envelopes, the old combined drift for their unchanged
arithmetic, and the original rank/conversion remainder. A failed optional certificate keeps the previous
terminal caps; cancellation remains `Stopped`. The [terminal prefix proof](search.md#terminal-factor-prefixes)
specifies the retained-execution argument, operation counts and rounding inequality.

`native_note_bucket_caps(power, times)` also uses every original note's actual conversion, type and frozen life,
all historical ordinary/Gekisou Combo observations, and six factor-prefix intervals in the existing native note
kernel. It preserves both floors. Same-time notes share their componentwise maximum for the exact time-multiset
interface. Probe-on lower amplitude is zero because a possible fixed row need not actually hold. A native cap
containing the actual converted terminal score can intersect a fallback cap without increasing its positive
conversion excess, so the original remainder remains valid.

`native_score_mean_upper(power)` is the first optional Score cap. It reuses that native kernel for terminal
notes and each rank's historical end Query, using only ordinary commands and Notes filed by that Query. The
complete history's drift and Combo hulls remain valid at every earlier execution. Adjacent rank snapshots must
have ordered nonnegative score-frame endpoints, no intervening possible filing and identical earlier fixed
bonus coefficients. Their common stored prefix then cancels, leaving only new frames. Every included note needs
its own Query's prior readiness marker; later markers do not repair a gap.

The range sum, rank bonus with a nonnegative percentage, and final terminal-plus-fixed sum each need an
independent nonnegative `i32` support proof. Rank expectation uses the outward product of the expected range upper and
`percent/100`, without flooring that expectation. The plan preserves native pending overwrite, next-Query
filing offsets and final fixed coefficients. Failure of any rank declines the entire optional whole-score
upper; successful per-note caps and the fine remainder remain available. A matching whole-score upper skips
Fine evaluation. The [native cap proof](search.md#native-terminal-note-and-rank-caps) states the cancellation,
integer arithmetic and retained-prefix conditions.

Preparation and full scoring share one session's recorder cache. Every unfinished order retains its previous
cap, and `UpperOnly` can return only after the whole 120-order cap sum proves exclusion. It is distinct from a
complete evaluation and from cancellation. Preparing all 120 orders alone never completes an evaluation;
`leaves.lotteryUpper` records this work separately from completed simulations. Nonlinear payoff objectives
continue to use the complete scorer and refinement provider below.

## Reusing factor histories at another power

An admitted recorder can also compile the completed factor and combo histories into
an immutable score-bound program. The recorder's dependency check establishes that
ordinary effects, cumulative values, converted judgements, life, query times and
rank arrivals do not read initial total power or score. Power enters only the note
calculator. Nonzero power commands are rejected by the existing admission checks.
Solo rank is fixed; external ranks retain their declared arrival timeline.

The program identifies the complete initialized model with only initial total power
normalized. Chart notes and skill events are retained once in an exact shared scope:
the constructor copies these same values into the model. That scope also includes
every classified skill, play frame, judgement, seed and binary32 delta-time bit.
All other initialized fields remain in the model identity. Reuse additionally
requires the same retained certified probability-curve object.

A second identity is available after a model completes its own admitted recording,
zero-random-draw, range-FINISH and query-count checks. It keeps the complete ordered
replay events, frame/query counts, probe rows, calculator fields with initial power
normalized, Rush percentage, final life and query allowance. These are every input
read by the subsequent compact LUCK replay; the recorder's combo observation and filing admission state,
and a note's later native accumulator, are not read there. Distinct initialized
models can therefore reuse a program when their completed replay inputs agree.
The same retained certified curve is still required. Every new power evaluates its
own note and rank arithmetic.

This recorded identity uses tagged integers, list lengths and floating-point bits.
It preserves every query, probability-readiness event and factor command, including
zero commands and signed-zero values. Factor commands retain their insertion
positions in the other event stream; that stream can share storage by full byte
equality without losing filing order. Each recorded identity admits at most 512 KiB,
further limited by the cache allowance. A size refusal skips that optional lookup;
the admitted evaluator continues independently. A cancelled recording or identity
construction supplies no completed bound.

Each interned note kernel keeps the note type, judgement, life-positive predicate
and both endpoint bits of two power-independent expressions in their original
grouping: Gekisou combo times the sum of skill and ordinary combo, and note score-up
plus the selected judgement score-up. The remaining arithmetic reads only these
expressions, so histories with equal resulting intervals can share one kernel.
Note uses separately keep their original probability-curve index and whether lottery
commands were already filed at that query. A new power reevaluates the remaining
original binary32 operations and integer floors; it does not scale a previous score.
Note means are added in their original filing order. Signed range differences,
integer rank percentages and shared fixed-bonus coefficients retain the same arithmetic.

Only rank-observed and terminal score queries need arithmetic on reuse. Their
unchanged-prefix cancellation thresholds are compiled from **all** intervening
queries, including unmeasured ones. A pending rank bonus is filed by its actual next
query; several confirmations before that query retain the native last-pending-value
rule. The final note expectation keeps its separate, fully probability-linked
observations instead of substituting an earlier query's mean.

This cache belongs to `LuckDpCache` and holds at most 128 programs and 32 MiB,
further limited by its configured byte allowance. It counts retained container
capacity, both identities, kernels and references, plus each shared run scope,
recorded event stream and retained curve allocation once. A secondary hit shares
the retained program and creates no extra initialized-model alias. Zero capacity
disables reuse. Only completed programs enter
the cache. Each evaluation checks cancellation and numeric admissibility again;
an overflow or interrupted evaluation supplies no completed bound. A cache hit is
still an all-path enclosure, and does not itself prove an exact payoff or ranking.

## Work and completion

`LuckExactBudget` bounds the number of replay attempts and executed frames across candidate orders. Each request starts with 240,000 replay segments and 8,000,000 executed frames. Each order admits at most 32,768 replay segments and 32 nontrivial outcome choices along a path. Pending paths are bounded to 32,768, and stochastic path depth bounds the retained ancestor checkpoints. The production search admits a chart when one playback fits the default frame allowance; its note count is independent of this admission decision.

Cancellation is checked during playback. Budget exhaustion, unsupported random draws and arithmetic capacity return a declined attempt while preserving the existing certified candidate enclosure. A completed law can refine score expectations, threshold probabilities, capped scores or joint score/life payoffs. The interval frontier establishes the resulting rank independently of whether every expectation has an exact rational representation.

## Synthetic checks

```sh
cargo test --release --locked -p ournotes-sim --lib luck_exact
cargo test --release --locked -p ournotes-sim --lib nominal_tests
cargo test --release --locked -p ournotes-sim --lib program_tests
cargo test --release --locked -p ournotes-search --lib refinement_tests
cargo test --release --locked --test adapter_fixture_export luck_refinement
```

The Cartesian tests compare all terminal score/life atoms and exact masses, including a long deterministic prefix, separated draws with intermediate checkpoints, external rank confirmations, partial-work interruption and draw-counter preservation.

Program tests compare fresh and reused enclosures bit for bit at several powers,
including binary32 integer-precision boundaries, and cover all 120 performer orders
at a second power. They also check complete weighted probability branches, rank
rewinds and simultaneous confirmations, run-scope and paired-Snap changes,
cancellation, numeric refusal, zero capacity and eviction. Recorded-program tests
compare distinct initialized models with equal replay inputs, and distinguish
changes in every event variant, ordering, floating-point bits, calculator context,
final life and probability curve.

## Boundary-candidate order storage

The ranking frontier retains certified score and payoff intervals for every relevant candidate. Small charts
retain detailed order state eagerly. For other charts, refinement reconstructs that state for one ambiguous
boundary candidate at a time from the immutable request and complete performer identities.

Each completed order law narrows its payoff and the aggregate expectation. Installed frontier bounds survive
releasing detailed order rows. A stopped probability tree preserves the previous certificate. Canonical ties
and the complete-domain ranking certificate determine the returned order independently of this storage policy.

The synthetic `long_stream_refinement_materializes_the_boundary_candidate` test exercises this path through
the public recommendation entry point. The storage-policy unit test distinguishes eager storage from backend
admission and checks that the ordinary long-chart domain remains eligible for bounded refinement.

## Refinement scheduling

Ambiguous candidates with smaller current certified payoff upper bounds receive refinement first, with
candidate identifiers breaking ties. Their upper bounds are closer to exclusion by a competitor's proved
lower bound. This priority uses the complete frontier and preserves every existing certificate.

Among the orders requiring refinement, the search first evaluates those with the widest payoff enclosure.
Every order has the same weight in the uniform-order objective. Canonical order indices break priority ties.
The interval frontier establishes each returned rank from complete certificates independently of this work order.

Additional synthetic checks cover several draws in one frame, conditional-state isolation, bounded stochastic
depth, initialized-model identity, complete-law reuse, and a complete threshold ranking over a 601-frame schedule
with request cache capacities of zero and 64.

## Initial proposals

A certified request evaluates at most `min(K, 3)` proposals during warm start when Snaps are available. A domain
with a single empty Snap binding admits up to 16 proposals. Capped-score and score/life threshold targets admit
up to 16 proposals. The subsequent traversal retains the complete requested domain and uses the
certified lower cutoff when enough candidates are available.
This proposal allowance controls search order, independently of the request's total work and time limits.
The bounded targets' finite shortlist uses the request's full deadline; other certified score targets begin
new seed proposals only in the first quarter of the remaining time.

The `certified_seed_budget_preserves_the_complete_canonical_ranking` test checks the canonical result against
exhaustive score search at several K and cache capacities, including the full candidate count.
