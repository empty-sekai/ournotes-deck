# Exact nominal LUCK refinement

The refinement provider evaluates the complete probability law for one specified performance order under the declared independent nominal LUCK draws. Each semantic draw uses the integer weights produced by the calculation model. Duplicate outcomes with the same result share their combined mass.

## Best conditional order expectation

Played-live requests may declare `metric:{"kind":"bestOrderExpectedScore"}` in the native search request or
the account recommendation request. For a legal team `D`, let `S(D,o,L)` be its terminal score at performance
order `o` and nominal lottery outcome `L`. This objective is

\[
V(D)=\max_{o\in\mathfrak S_5}\mathbb E_L[S(D,o,L)].
\]

The `score` metric keeps its uniform-order expectation, averaging all 120 conditional laws with weight
`1/120`. The best-order metric preserves the same legal teams, every eligible leader, and all legal paired
Snap bindings. Its expectation still uses the nominal lottery law inside each order. A maximum reachable
terminal score would be a separate maximum over lottery outcomes. Score thresholds, capped scores, life
events and native event payoffs continue to use their own expectations and boundary refinements.

For every order the search retains a certified expectation enclosure `[l_o,u_o]`. A pending order can use
the native score range and any independently proved whole-order cap; it has no evaluated outcome. The team
value remains enclosed by `[max_o l_o,max_o u_o]`, including every pending order. Node bounds use position
maxima where a uniform-order bound uses position means. The bound grid stores `120` times this maximum;
per-order exclusion uses `120*max_o u_o`. Mean-only bound providers are outside this objective's admitted
route. No evaluated subset is averaged or treated as the complete maximum.

Selected members also receive an assignment bound: a 32-mask dynamic program places their existing
per-position gain envelopes in distinct positions, while free members retain conservative column maxima.
This excludes impossible combinations where several members each claim the same peak position. Every
addition rounds upward, and the bound retains the native score envelope's existing rounding allowance.
It tightens team-prefix exclusion before conditional-order simulation. This dynamic program combines
upper envelopes; it does not merge native controller or factor-history states merely because they have
used the same subset of members.

The result's `bestExpectedOrder` contains an actually evaluated physical-slot permutation, member IDs,
its conditional `expectedScore` when exact, its `scoreInterval`, and `evaluatedOrders`. Its `optimality`
is `proven` only when its expectation beats every other order, or is proved at least as large while its
physical permutation wins the lexicographic tie. Search results use the canonical team layout, with the
leader in slot 2 and the other member/Snap pairs sorted into slots 0, 1, 3, 4; native and formal account
transports use this same basis. Exact rational comparisons can establish a tie even
when the enclosing binary64 intervals overlap. Completing 120 evaluations alone does not prove an order
when their expectations still overlap. Conversely, valid caps can prove the selected order while some
losing orders remain unevaluated. Conditional evaluations visit larger upper bounds first and skip an
order once its cap proves it cannot beat the evaluated witness, including the canonical tie. Exact
frontier refinement likewise excludes order caps below the team's proved maximum floor, and visits
the remaining largest upper bounds first.

The enclosing team's `scoreInterval` concerns the maximum over all orders. The witness's interval concerns
that evaluated order. A timeout after one complete order preserves the witness and the pending-order caps,
with unproven order optimality where necessary. A timeout before any complete order supplies no witness.
Team rank certification remains separate: equal complete score programs can establish a team tie while
the best internal order still needs refinement. Overall interruption and completion retain the declared
search budget semantics.

Order-score caches use complete canonical performer identities and the fixed total power. A reused order
is mapped back to the current team's physical slots before selecting the lexicographic witness. Pending
bounds and exact nominal refinements retain those same labels. Cache capacity changes storage and work;
it does not change the legal order domain or the meaning of a certificate.

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
original preparation, row/checker compilation and optional-life checks. When those checks construct a life
interpreter, the shared key includes its initialized model through the lossless initialized-state identity.
The complete score-recorder dependency check may prove that no life, conversion or lifecycle reader consumes
score values or score-derived rank bonuses. Only under that certificate may the key normalize initial total
power. Solo ranks remain fixed and external rank arrivals remain exact inputs. A declined certificate keeps
the actual power, and a proof-mode byte prevents the two routes from aliasing. Compiled skills and checkers,
controller and rank state, converted judgements, and clocks remain in the identity. Score lookup maps use
sorted entries and non-NaN floating values retain their round-tripping encodings, including signed zero.
Arithmetic admission remains a separate requirement.

The life model's notes and events are represented once by the existing order-exact shared scope. Key
construction temporarily moves those fields out, builds the initialized-state identity, and restores both
before returning. It also temporarily omits the raw performer input from the reduced key because the full
compiled life state represents every value consumed by that interpreter; it restores that input afterward.
The final length-delimited key combines the reduced recorder identity with the initialized life identity.
Equal keys therefore preserve both the ordered reduced transcript inputs and the state consumed by the
life interpreter. A difference in a raw card attribute can share a key only when its fully compiled state
is identical; a difference in a consumed power, skill, damage or conversion input changes the identity.
Power projection changes only identity construction; the actual native model is restored before playback.

NaN-bearing, opaque or over-512-KiB initialized identities decline shared life reuse. The final combined
key must also fit the existing one-MiB limit. Refusal keeps session-local raw-key reuse or independent
recording available, under the same cancellation and complete-propagation requirements.

The request may also reuse the completed life transcript independently of the reduced lottery-controller
rows. Its key uses the same certified initial-power projection or exact-power fallback, plus the complete
remaining native life model, exact owned context and full shared scope. Notes and events are omitted from the initialized model only while
keying because that context preserves their complete order and contents. The transcript stores the life
integer observed at each of the two skill phases of every frame, and every judgement-kind change. Consecutive equal phase pairs and
unchanged judgements use lossless omission. Every reduced frame, action and controller query still executes
in its original order, reading the reconstructed life transcript.

This separate table publishes only after a successful complete recording and complete curve propagation
or completed-curve reuse; cancellation and failed or partial recordings cannot insert a transcript. Its
allowance is the smaller of the curve-cache allowance and 1 MiB, with at most 128 entries. Accounting
includes the retained scope, dictionary/delta keys, entry-buffer capacity, and each distinct transcript
allocation and its vector capacities. Zero capacity clears reuse, and a refused key or allocation falls
back to independent native life recording. The `lifeRecording*` telemetry fields report counts and retained
bytes; they do not expose model identities.

The shared reduced-recording identity combines the unchanged complete recorder key with an owned scope. Its key contains the
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
never reads. An optional life calculator uses the separate dependency certificate described above, retaining
its exact power whenever that certificate is unavailable. The reduced
interpreter's before/after controller calls supply literal score zero; admitted score appliers only
file factor commands. Frame shape and all other filing inputs remain scoped. Equal identities therefore
produce equal complete transcripts and certified curves, including their probe metadata. Candidate score
and ranking retain their separate proofs. Full initial life state remains necessary even without a life
interpreter, because a retained native checker may still read life.

A private, immutable `LuckScoreSession` memoizes one bounded owned scope at its current allowance. The
current constructor places every deck-dependent initialized value in the combined recorder/life key;
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

## Identity judgement projection

The reduced lottery recorder may omit its auxiliary life/judgement interpreter when a complete dependency
check proves that no retained controller action reads life and every possible conversion preserves each
declared judgement. The check uses the full conversion closure, including raw-row target aliases and the
native destination casts. A condition that happened to be false in one recording is not such a proof.
Unknown judgements, conflicting aliases and possible judgement changes retain the interpreter. The full
score recorder and its life/admission checks remain responsible for the candidate score and final life.

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

`LuckFamilyContext` admits a declared family of physical bindings for optional expected-score node bounds.
`LuckControllerFamily` contains its complete controller laws. The separate `LuckFamilyDomain` capability
contains full-domain admission, while each `LuckFamilyProfile` contains one complete writer-owner profile.
The context borrows the exact master tables,
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

The admitted family can contain at most two physical Snap resources that write the LUCK controller. Each
resource is absent or belongs to one allowed owner, and two resources cannot occupy the same member slot.
These exact resource-to-owner vectors give at most 31 profiles; fixed member writers remain present.
Distinct physical resources retain distinct profiles even when their selected source rows are identical.
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

Every completed profile is evaluated in all 120 original member orders, with complete initialized performers and the
original source order at each performance position. A coverage bitmap checks every profile/order pair;
duplicates cannot stand in for missing pairs. The returned four buckets are the joint virtual direct-LUCK
score-probe bit and native Rush bit at a note's chart time. The virtual probe bit is meaningful even when no
member holds a probe, because complete program admission establishes its controller meaning. Projecting
reward rows preserves the complete member fields and their native mission/support-selection metadata.

After complete pair-domain admission, an optional input identity can reuse a finished curve before rebuilding
the projected native model. A closed source/checker dependency proof must show that character identity is
unread before that one field is normalized. The ordered performers retain every other field and source;
unknown, cumulative or conversion programs keep the original construction path. These complete input keys
share the existing per-family recorder-key storage, its 128-entry/1 MiB limits and the configured curve
allowance. A hit still registers the original physical profile and labelled order independently.

The complete-family preparation API still computes every profile. Search instead admits the entire physical
pair domain first and prepares complete profiles as actual depth-four nodes require them. The full-domain
capability retains every allowed physical choice and writer-owner mapping; its immutable identity prevents a
profile from another context or another admitted domain from being substituted. A profile is published only
after its own 120 original labels complete. Unrequested profiles remain explicitly unknown.

For each completed profile the reward template averages coefficients over all 120 original orders, using each member's
actual position in each order. It retains every completed profile's base and member/Snap gains together with
immutable physical-binding metadata. A depth-four node fixes four bindings and enumerates every legal choice
for its final slot. Each resulting complete binding selects its unique certified profile and its own original
power-bound terms; their product bounds that binding's expectation. The maximum of these complete binding caps
bounds the node. Coefficients from incompatible writer owners are never combined. This selection happens only
after every profile needed by that node has completed all 120 orders. Required-profile discovery and the final
upper use the same complete physical-binding enumeration. If any legal last choice lacks its profile, the
entire optional node bound is unavailable. For two physical writers, a fixed depth-four prefix needs at most
three profiles for each possible final member. Existing all-history rank, conversion, additive drift and unclassified-window
allowances remain unweighted. The result is an upper only for Score; it supplies neither per-path support nor
the expectation of a nonlinear payoff.

Within one reward-template binding, labels that reference the very same immutable joint-probability object
can reuse its reward arithmetic. A bounded temporary table retains that curve's base coefficient and every
member/Snap choice at all five positions. It uses object identity, never approximate equality of probabilities;
the template, members and terminal query mapping are fixed for its entire lifetime. Every original label still
adds its own coefficients in the original outward summation order. The table has at most 128 entries and
1 MiB of accounted capacity; zero or insufficient capacity uses direct arithmetic. It supplies no new program
equivalence or candidate-ranking certificate.

The search's preparation limits are 4,096 pair models, at most 31 profiles and 3,720 order/profile evaluations,
16,000,000 units of declared frame work, and 32 MiB of accounted family storage. Admission retains the complete
family's work check even when search initially requests only one profile: the frame-work check includes the
terminal mapping and every order/profile in the full family. The retained domain and coefficient cache separately uses
at most 64 entries and 8 MiB, further limited by `cacheEntries`. Container capacities, complete input choices and
performer vectors, retained mappings and
referenced probability payloads are accounted by their respective owners; these budgets are not process RSS
or the combined memory of every cache.

A successful complete-family capability requires complete coverage within every limit. Capacity failure, unsupported input or a
local work budget produces an unavailable optional bound; cancellation produces a stopped preparation.
Neither supplies a partial family or an optimistic maximum over only the completed profiles. Completed
individual probability curves may be reused through their existing exact recording keys. The search cache
stores full-domain admission with independently completed profile coefficients or explicit unknown/refused
entries in its immutable scope. Cached admission alone cannot prune a node. Search completion and canonical
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
caller-proved fine-cap decomposition. These upper-only interfaces supply neither score support, an exact law
nor a candidate value. The separately checked `terminal_summary(power)` can instead supply a complete score
expectation enclosure, as described below. External ranking and unknown terminal mappings retain full scoring.

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
interface. Probe-on lower amplitude defaults to zero because a possible fixed row need not actually hold. A
positive lower additionally requires a unique native-row match and a complete fixed-true condition for each
contributing row, the common LUCK gate, nonnegative rows, at most one nondecreasing filing run and only LUCK
missions among active ranges. Distinct rows retain their identity even when they share an owner. Mixed mission
finishes retain zero; unproved or nonmonotone filing schedules retain the unconditional enclosure. Complete command-history drift
still covers signed apply/undo and retained earlier executions. A native cap containing the actual converted
terminal score can intersect a fallback cap without increasing its positive conversion excess, so the original
remainder remains valid.

`native_score_mean_upper(power)` is the first optional Score cap. It reuses that native kernel for terminal
notes and each rank's historical end Query, using only ordinary commands and Notes filed by that Query. The
complete history's drift and Combo hulls remain valid at every earlier execution. Adjacent rank snapshots must
have ordered nonnegative score-frame endpoints, no intervening possible filing and identical earlier fixed
bonus coefficients. Their common stored prefix then cancels, leaving only new frames. Every included note needs
its own Query's prior readiness marker; later markers do not repair a gap.

The range sum, rank bonus with a nonnegative percentage, and final terminal-plus-fixed sum each need an
independent nonnegative `i32` support proof. The private kernel keeps both native integer endpoints for each
original note occurrence and weights all four joint masses directly. Historical rank expectation encloses
the outward product of the expected range interval and `percent/100`, then subtracts an enclosure of the native
integer-division remainder and intersects with the independent rank support. Neither endpoint floors the
expectation. The plan preserves native pending overwrite, next-Query filing offsets and final fixed
coefficients, including coefficient two. Failure of any rank declines the entire optional whole-score
certificate; successful per-note caps and the fine remainder remain available. A matching whole-score upper
skips Fine evaluation. The [native expectation proof](search.md#native-terminal-note-and-rank-expectations)
states the cancellation, integer arithmetic and retained-prefix conditions.

`LuckTerminalRush::terminal_summary(power)` exposes the complete two-sided score expectation, independent
native integer support and deterministic final life only after this whole-score proof succeeds at exactly the
prepared power. Only singleton support supplies exact constant-score metadata. This summary covers every
terminal note and admitted historical rank; it is not a probability law or an exact rational mean merely
because its interval is narrow. `LuckScoreSession::summary_or_terminal` uses the completed terminal summary
when available and otherwise calls the existing factor-history scorer. Cancellation returns no summary and
never becomes a successful fallback. A caller that already prepared an order reuses its complete summary, or
uses the original scorer after refusal, without repeating a successful preparation.

Preparation and full scoring share one session's recorder cache. Every unfinished order retains its previous
cap, and `UpperOnly` can return only after the whole 120-order cap sum proves exclusion. It is distinct from a
complete evaluation and from cancellation. Preparing 120 upper-only capabilities does not complete an
evaluation. In contrast, 120 complete terminal summaries can supply the full uniform expected-score enclosure
without factor-history replay. Overlapping candidate intervals can still require refinement; neither complete
summaries nor the `leaves.lotteryUpper` preparation counters alone establish the full-domain ranking or
`Complete`. Nonlinear payoff objectives continue to use the complete scorer and refinement provider below.

Completed terminal preparations can also be reused before another lottery recording or prefix-kernel build.
The key contains the lossless full initialized model, exact native initial power and immutable request scope.
Every caller first passes the full recorder and native arithmetic admission. Both a successful optional
terminal kernel and its conservative refusal are retained as part of a completed preparation; an unfinished
preparation is never published. Cancellation is checked around lookup and return as well as construction.

Terminal preparations share the factor-history program cache's FIFO and its existing limits of 128 entries
and 32 MiB. Accounting includes the full scope, identities, terminal vectors and distinct retained probability
curves. A zero allowance disables this reuse. These entries supply the same capabilities as their original
preparation; an upper-only hit does not become a candidate evaluation.

A completed terminal recording and factor preparation can also form a power-independent recipe. Its key
retains the same complete initialized model and immutable scope, with only initial total power normalized
after the recorder dependency proof. The recipe contains the full recording, terminal factor ingredients,
probability readiness and deterministic final LIFE. It contains no old native-note scores, whole-score
enclosure, rank bonuses, integer-support conclusion or power-specific arithmetic refusal.

Every recipe lookup constructs and admits the current native model. A hit then rebuilds the terminal numeric
kernel with the current calculator and exact requested power, repeating the original binary32 operations,
integer floors, historical rank operations and all magnitude and support checks. A refusal at one power does
not refuse another power. Cancellation cannot publish an unfinished recipe or return a completed score.
Recipes share the same FIFO, 128-entry limit and total byte allowance with replay programs and exact-power
terminal certificates; complete trace, prefix, probability and container capacities are included. Zero
capacity leaves the independent recording path available.

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

## Rank expectation from score residues

`LuckScoreSession::rank_summary` first completes the factor-history score replay, then optionally refines the
integer rank bonuses. For a rank percentage `p`, native truncation of `p*S/100` depends on the score's sign and
its residue modulo `100/gcd(abs(p),100)`. The current score adapter admits nonnegative range sums and obtains
their rewards from the actual historical end Query, with the same unchanged-prefix proof and prior probability
readiness required by the score replay. Terminal note rewards cannot substitute for this earlier snapshot.

Each native note bucket with singleton integer support contributes its residue. Coincident notes contribute
one summed reward at their shared controller observation. A non-singleton bucket moves its path into an
absorbing unresolved class; it neither chooses an interval endpoint nor invents a probability distribution.
The native lottery transition graph propagates the resulting joint residue law, preserving controller state,
both note-observation phases and delayed probes. It observes every requested chart-time group before marginal
curve coalescing, including groups whose marginal curve did not change. Windows run sequentially under the
existing state and cancellation limits. A saved law is taken at its last required observation, while the
remaining transcript still completes its ordinary admission checks.

The refined bonus uses the existing score mean and the known residue masses to enclose the truncation
correction. Only the unresolved mass receives the full possible remainder interval. Known and unresolved
masses must together enclose one. This retains correlation across notes without enumerating the complete
terminal score law. The final rank contribution is rebuilt from the original bonus identities and their
proved terminal coefficients, then intersected with its previous certificate. Integer support and final life
are unchanged; missing rewards, unsupported snapshots or local capacity refusal keep the previous enclosure.
Residue and ordinary summaries have distinct cache identities.

## Refinement scheduling

After the physical domain closes, expected-score and best-order expected-score searches first attempt complete
factor-history summaries for every currently ambiguous boundary candidate. A second pass attempts the admitted
rank-residue summaries. Only after those passes does the existing complete-law tree receive the remaining
ambiguities. With retained detailed rows, each completed order immediately intersects the frontier and those
installed narrowings survive cancellation. Missing detailed rows are reconstructed from the immutable request;
this reconstruction installs a new aggregate only after all 120 orders complete. An interrupted reconstruction
keeps the prior frontier certificate. Both paths preserve the actual domain-completion state. Other terminal
objectives retain their own probability-law refinement.

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
