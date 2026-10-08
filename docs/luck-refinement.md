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
contains full-domain admission and a work allowance covering every possible profile. `LuckFamilyProfileDomain`
contains the same complete physical-domain admission with a cumulative allowance for requested native profiles.
Each `LuckFamilyProfile` contains one complete writer-owner profile.
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
the projected native model. A closed source/checker dependency proof takes the union of attribute reads across
every selected source and owner in the projected deck. It can normalize only unread character, band, card-type,
tag, category and mission fields in this private key. The proof follows the native target predicates' OR
semantics and their exact positive/nonzero selector rules; a selector is retained even when another selector
already matches the current deck. Character-reading, unknown, cumulative or conversion programs keep the
original construction path. All selected source IDs, levels, source-vector order and potentially read fields
remain exact. Uncached recording still constructs the original physical performers. These complete input keys
share the existing per-family recorder-key storage, its 128-entry/1 MiB limits and the configured curve
allowance. A hit still registers the original physical profile and labelled order independently.

The complete-family preparation API computes every profile, and `admit_domain` checks its full possible
order/profile work before returning. Search uses `admit_profile_domain` to check the entire physical pair
domain first and prepares complete profiles as actual depth-four nodes require them. This separate capability
retains every allowed physical choice and writer-owner mapping; its immutable identity prevents a
profile from another context or another admitted domain from being substituted. A profile is published only
after its own 120 original labels complete. Unrequested profiles remain explicitly unknown. Semantic admission
still checks every pair: an unrequested LIFE dependency, conversion or unsupported writer refuses the domain.

A completed profile may also supply a complete canonical probability program. Its key retains the full
projected Performer multiset and immutable context; normalization of unread fields requires the same closed
dependency proof. Each key also records its originating admitted domain, exact profile and fixed slot
bijection. Constructing a program requires that key's own completed profile, with all 120 unique labels.

Another fully admitted profile can reuse this program only when the complete canonical inputs match.
Transport enumerates the target's original 120 orders and maps each target slot to the corresponding
canonical slot. The ordered native input is identical under this bijection. The returned profile receives
the target domain identity, target writer-owner label and target positions, in the target's original
enumeration order. This preserves both physical coverage and the reward template's outward summation
order; it does not commute native commands or combine approximately equal probability curves. Missing
identity proof, capacity refusal or an unavailable program retains independent profile preparation.

For each completed profile the reward template averages coefficients over all 120 original orders, using each member's
actual position in each order. It retains every completed profile's base and member/Snap gains together with
immutable physical-binding metadata. A depth-four node fixes four bindings and enumerates every legal choice
for its final slot. Each resulting complete binding selects its unique certified profile and its own original
power-bound terms; their product bounds that binding's expectation. The maximum of these complete binding caps
bounds the node. Coefficients from incompatible writer owners are never combined. This selection happens only
after every profile needed by that node has completed all 120 orders. Required-profile discovery and the final
upper use the same complete physical-binding enumeration. If any legal last choice lacks its profile, the
entire optional node bound is unavailable. For two physical writers, a fixed depth-four prefix needs at most
three profiles for each possible final member. Ordinary historical rank, conversion, additive drift and
unclassified-window allowances remain unweighted. The separate direct-probe history capability described
below can tighten only its admitted historical contribution. The result is an upper only for Score; it supplies neither per-path support nor
the expectation of a nonlinear payoff.

Within one reward-template binding, labels that reference the very same immutable joint-probability object
can reuse its reward arithmetic. A bounded temporary table retains that curve's base coefficient and every
member/Snap choice at all five positions. It uses object identity, never approximate equality of probabilities;
the template, members and terminal query mapping are fixed for its entire lifetime. Every original label still
adds its own coefficients in the original outward summation order. The table has at most 128 entries and
1 MiB of accounted capacity; zero or insufficient capacity uses direct arithmetic. It supplies no new program
equivalence or candidate-ranking certificate.

The search's preparation limits are 4,096 pair models, at most 31 profiles, 3,720 reserved native order
evaluations, 16,000,000 units of reserved native frame work, and 32 MiB of accounted family storage.
Its profile-domain admission checks the full profile count and requires the allowance to fit the common
terminal mapping plus one complete 120-order profile. Before each native profile attempt it reserves another
120 orders and their full frame schedules against the same cumulative allowance; refusal or cancellation
keeps this conservative reservation. It never starts a partial profile merely because some orders would fit.
Complete-program transport retains the target's 120 original labels and executes no native recording frames,
so it consumes no additional native work reservation. `reservedProfileOrderWork`, `reservedProfileFrameWork`
and `profileBudgetRefusals` report this accounting separately from completed profiles and original order laws.
The complete-family `prepare` and `admit_domain` interfaces retain their full possible-cover work check.
The retained domain and coefficient cache separately uses
at most 64 entries and 8 MiB, further limited by `cacheEntries`. Container capacities, complete input choices and
performer vectors, retained mappings and
referenced probability payloads are accounted by their respective owners; these budgets are not process RSS
or the combined memory of every cache.
Complete probability programs share this same entry and byte allowance with admitted domains and profile
coefficients. Their collection additionally uses at most 1 MiB and at most 63 entries, leaving an entry for
a domain within the configured limit. Keys, all 120 curve references, container capacities, mappings and
distinct retained curve allocations are counted; shared allocations within this collection are counted once.

Admission and derived coefficient entries share a recency sequence with complete probability programs. A
program becomes protected against scans of derived entries only after a successful complete transport has
demonstrated reuse. Cold programs and derived entries compete by recency. The separate 63-entry/1 MiB program
limit can still evict the oldest program, so protection never expands a budget or prevents new programs from
being admitted. Rebuilding a derived table can reuse a retained program without repeating native profile work;
any new native attempt still consumes the original admitted domain's cumulative reservation.

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

An optional shared recording pass produces the complete ordinary score history and the nominal probability
transcript during the same native playback. The existing reduced-model constructor and compiler still establish
the initial controller templates, lottery machine, action plan and probe flags before any frame runs. A private
read-only observer then extracts probability inputs after both original skill phases and the second score query,
immediately before the controller consumes the frame's notes and pending lotteries. It reads each factor at the
original note chart time through the native binary32 filing order. It does not recompute, sort or commute those
factor additions. The ordinary playback path uses a compile-time empty observer.

This route requires the existing structural recorder admission, solo rank, only LUCK ranges, an action plan
that never reads LIFE, and a complete conversion check proving every declared judgement unchanged. Each observed
native result must also match that declared note and judgement. Expanded conditional-effect identities must be
unique across the full model: ordinary effects share native applier registries with retained lottery writers,
including the native wrapping ID expansion. Unproved identities, LIFE-dependent actions, changing conversions
and other unsupported inputs retain the original separate recorder. The structural observer gate is unchanged.

Both routes use the same transcript field extraction and complete transcript key. Every original frame is
observed before the existing quiet-frame compression; initial controller state is never taken from a model
that has already played. A transcript becomes available to the existing curve cache only after all declared
frames complete, the native draw count remains zero, and every range reaches FINISH. Probability propagation,
outward summation order and the complete cache identity remain unchanged. The shared path does not use or fill
the compiled-recording shortcut cache; that cache remains available to the separate recorder. No new retained
cache is introduced.

The five growing transcript arrays have a one MiB temporary capacity guard. Initial machine, templates,
compiled actions and range metadata remain separate input-dependent temporary storage. Capacity refusal or a
native error discards the partial recording and returns the existing unavailable result; cancellation returns
`Stopped`. A fallback starts its own fresh model and never repeats playback on a partially advanced recorder.
For a native failure outside the observer, the partial full model is released before the original separate
probability evaluator restores the old error priority: a reduced-model failure retains its probability-domain
reason and error, while a completed probability law leaves the full recorder's admission error. Only this
failure path repeats probability recording. It uses the original cancellation callback and cache limits;
a complete probability result may be retained, but no partial transcript or terminal score is published.
Diagnostic `fusedRecordings` counts completed shared native recordings, `fusedFrames` includes frames attempted
before interruption, and `fusedRefusals` counts declined optional admissions. These counters do not confer any
completion or score-law authority.

The [shared recording regressions](../crates/ournotes-sim/src/live/full/luck_dp/fused_tests.rs) compare complete
transcript words for all 120 orders against the independently stepped reduced recorder, including multiple
non-dyadic same-owner gauge writers, phase changes, multiple notes per frame, Miss actions and FINISH. They
compare every probability endpoint and terminal-capability field bit for bit, exercise zero cache capacity,
and reject changed judgements, LIFE actions, wrapped effect identities and partial/cancelled recordings. A
large valid native frame exercises the actual temporary capacity guard and verifies that an earlier complete
cache entry survives the refusal. Failure comparisons also cover overlapping ranges, malformed frame/note
inputs, ordinary-only pool exhaustion, original error payload priority and cancellation during the cold replay.

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

Separate probability recording and full scoring retain one session's recorder cache; the shared native route
reuses the completed transcript cache described above. Every unfinished order retains its previous
cap, and `UpperOnly` can return only after the whole 120-order cap sum proves exclusion. It is distinct from a
complete evaluation and from cancellation. Preparing 120 upper-only capabilities does not complete an
evaluation. In contrast, 120 complete terminal summaries can supply the full uniform expected-score enclosure
without factor-history replay. Overlapping candidate intervals can still require refinement; neither complete
summaries nor the `leaves.lotteryUpper` preparation counters alone establish the full-domain ranking or
`Complete`. Nonlinear payoff objectives continue to use the complete scorer and refinement provider below.

An optional identity for a completed uniform-order exclusion certificate can omit character IDs only after
a closed dependency check of every selected ordinary Live, support, Gekisou and Gekisou-support source.
The check includes triggers, conditions, releases, resets, cumulative conditions, formation predicates and
effect target consumers. A selected character target, missing row or unknown primitive preserves the original
complete identity. Unselected master rows do not participate in this request's proof.

The admitted identity retains the exact multiset of complete performers with only character ID normalized;
all other fields, selected source IDs and levels, and source-vector order remain present. The cache still
requires the exact native power and the same immutable request scope. This key is used only to retrieve or
retain a complete `UpperOnly` exclusion certificate, or to establish equal uniform Score objectives after
complete candidate evaluation. The frontier still retains every physical canonical tie key and requires
the unseen domain to close before declaring a ranking proven. Equal objective programs do not manufacture
an exact numerical mean. Candidate score-cache identities, canonical physical bases, per-order refinement
programs and the separately declared best-order objective keep their existing identities. The
score-cap cache still retains at most 64 entries, further limited by the request's `cacheEntries`.

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

Completed recipes may retain a lossless compact trace. Query, readiness, potential-filing and probe records
use fixed tags and exact integer/bit payloads; ordinary commands, frozen note commands, rank identities and
their original order remain complete. Decoding reconstructs the original trace for the unchanged terminal
kernel. This representation performs no event coalescing, score approximation or arithmetic reassociation.
An unsupported representation, unprofitable encoding or insufficient entry capacity retains the original
trace when it fits, otherwise the completed exact-power certificate can still be cached independently.

The same 32 MiB ledger accounts for all retained recipes and the largest active decode workspace among
resident recipes. Cache access is exclusive and one decoded trace is dropped before another result is
installed, so the workspace is reserved once at its maximum, without summing mutually exclusive decodes.
Actual vector capacities, enum and entry storage, identities, probabilities and recipe payloads remain
accounted. Decode allocation failure or cancellation cannot publish a partial trace or a score certificate.

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

## Complete score-law equality

After the physical domain closes, uniform expected-score search can compare one ambiguous boundary pair
before numerical refinement. The native provider proves equality by coupling complete execution histories
for all 120 original order labels. It does not infer equality from matching score intervals, marginal
probability curves, sampled outcomes or rounded expectations. The two candidates must have the same exact
initial power and payoff mapping. The current provider admits native solo-rank contexts whose selected
missions are all LUCK; best-order and nonlinear objectives retain their existing refinement paths.

A fixed member correspondence selects a bijection between the two complete order sets. This correspondence
does not project away any source: every native model still receives its original complete performers and
member/Snap bindings. Each corresponding order must pass the ordinary recorder's complete dependency checks,
finish the native controller, and match the complete controller transcript. Each score history is independently
admitted before either direct trace comparison or complete timeline evaluation. The
controller identity includes the original integer base and bonus lottery tables. Emitted action chances must
be exactly zero or one; matching probability enclosures alone is insufficient. Random LUCK outcomes remain
fully represented by the shared transition history and its complete DP admission.

The score comparison retains command owners and order, times, filing locations, queries, combo inputs, rank
snapshots and every original frame boundary. Its only score-event projection replaces a note's numeric LIFE
with whether it is positive, matching the actual score read after the recorder has proved the LIFE and
conversion feedback closure. Final LIFE is compared exactly. Controller LIFE reads remain subject to the
original phase-specific dependency admission; their emitted action probabilities must be exactly zero or one
and match in the complete transcript. An independent emission guard evaluates fixed probe predicates exactly
and compares active probes' owners, phases, gates, source
and effect ordinals, raw values and native signed mills. It retains active zero-value probes. Each owner can
have at most one active probe. Ordinary factor producers sharing that owner must precede the probe in the
native phase/source order: an earlier phase, a live skill in the same phase, or an earlier conditional source
in that phase. The original frame loop establishes the same-frame command order, including backdated filings;
the same conditional source or a later producer declines the optional certificate.

When admitted ordinary traces differ, the provider can retain the complete support of their shared
score-observable histories. Each dynamic key contains the original controller state plus an interned ordered
timeline. The hidden gauge, prefetched result, guarantee and once-Miss state remain in the key until every
range finishes. Edges retain each actual Rush switch, FINISH disable and common probe-predicate switch with
its original playback frame, phase and chart time. Multiple switches within one note or frame remain ordered
edges. Quiet transcript repeats consume the original frame clock, and partial clock or range coverage cannot
construct a complete support. No probability branch is sampled or removed because its mass is small.

For every supported timeline, both candidates file their own admitted ordinary commands and the actual
Rush/probe commands into the native incremental score calculator. The recorded-combo query entry shares
native factor sorting, apply/undo, frame diffs, note integer arithmetic and fixed-score filing. Each query uses
its exact recorded combo inputs; historical rank snapshots retain wrapping subtraction and integer percentage
truncation. Source owners and same-time filing order remain inputs to native arithmetic. This comparison
does not assume that differently ordered binary32 additions commute.

Complete timelines share their identical ordered prefixes during score playback. The prefix index retains
every supported terminal's original identity and visits only ancestors of those terminals. Each checkpoint
owns both candidates' full native incremental calculators, including retained per-note scores, every
binary32 frame difference, pending fixed bonuses and factor filing order. It also retains the exact ordinary
event cursor, combo observations, historical score snapshots and current Rush/probe flags. Checkpoints are
local to one admitted order comparison; distinct controller prefixes never share a calculator state.

The playback cursor stops before the anchored ordinary event. Several actual switches at one Query or
ProbabilityReady therefore remain separate ordered filings, including note-driven and pending-consume
switches at the same frame. A terminal that is also a prefix of another complete timeline evaluates its
no-further-switches suffix independently. By induction on the ordered edges, the copied state at each child
equals the state obtained by replaying that candidate's complete prefix from its initial calculator. Running
the remaining ordinary suffix consequently gives the same native integer terminal score as independent
full-tape playback, including historical rank rewinds and late probe endings filed at the music boundary.

Depth-first traversal retains checkpoints only at branches. A 32 MiB capacity estimate covers the prefix
index, traversal containers and live checkpoint state, including native frame vectors and recorded combo
and rank arrays. Reaching this optional allowance releases the traversal state and evaluates each remaining
complete terminal with independent full-tape playback. A terminal already compared successfully is not
visited again. All actual query, frame, event and recording work consumes the existing request budgets;
checkpoint capacity, cancellation or an unequal score cannot produce a partial equality certificate.

Every paired timeline must produce the same native integer terminal score, and every original order must
complete. The common exact controller law then couples equal scores path by path, proving equal expectations
without constructing an exact rational mean. A single unequal terminal score refuses this optional equality
proof; matching marginal distributions or enclosing probability masses cannot override that difference.

Only a successful comparison and admission of every labelled order constructs the opaque certificate.
The frontier then atomically joins the two complete equality classes, after checking their payoff scope,
power, intersecting intervals and any exact values. Physical candidates and their canonical tie order remain
distinct. This proves equal expected scores without manufacturing an exact numeric mean. It neither closes
unseen physical work nor changes retained per-order replay identities.

Recording starts and executed frames consume the existing shared exact-refinement budget, including work
performed before a refusal, cancellation or native error. Orders are processed in pairs and released without
adding a retained cache. Each recording's growing frame log has a one MiB guard; bounded fingerprint encoding
and native model tables are separate temporary storage. Timeline support has limits of 100,000 original frames,
100,000 live controller states, 100,000 prefix nodes, 1,024 terminal timelines, 4,096 edges per materialized
path and 10 million transitions per order. A conservative 32 MiB estimate covers its retained maps, prefixes
and path storage. The native score fold separately admits at most 8,192 allocated score frames and uses a
32 MiB recipe/scratch estimate. It checks that frame limit before cloning the optional native calculator;
actual score storage and the effect lifecycle's music boundary may have different horizons. Per attempt,
folding permits at most 128 million queries, 256 million score-frame executions/undos and 512 million tape
events, with regular cancellation checks. These arithmetic operations are counted separately from complete
live playbacks. The original native recording budget and request deadline remain in force. Unsupported inputs, unproved score equality, uncertain
ordering or exhausted resources preserve the prior frontier certificates and ordinary refinement fallback.

The `score_equivalence_` tests compare a successful certificate against independently enumerated native
terminal score/LIFE laws for all 120 orders, and cover score/controller differences, nontrivial action
chance, zero probes, phase ties, cancellation and partial work. The `equality_merge_tests` check canonical
ties, unseen domain work, atomic conflict rejection and the scope of redirected class identities.
Active ordinary-counter relocation also exercises complete timeline folding and independently enumerated
native laws over all 120 labels. A separate native nominal enumerator compares complete timeline-to-score
maps against the production fold with non-dyadic skill amplitudes and multiple actual performer orders.
It also checks late probe endings filed back at the music boundary, alongside ordinary endings at that same
time. Cancellation after actual fold queries must still return no partial uniform certificate and account
for every native recording run/frame already consumed. Recorded-query tests compare native integer scores
and binary32 frame-diff state through signed same-time filings, rewinds, changed combos and historical rank
snapshots. Timeline support tests compare actual unweighted native Rush/probe edges and reject cancelled or
incomplete clocks.

## Refinement scheduling

After the optional uniform-score equality comparison, expected-score and best-order expected-score searches
attempt complete factor-history summaries for every currently ambiguous boundary candidate. A second pass attempts the admitted
rank-residue summaries. Only after those passes does the existing complete-law tree receive the remaining
ambiguities. With retained detailed rows, each completed order immediately intersects the frontier and those
installed narrowings survive cancellation. Missing detailed rows are reconstructed from the immutable request;
this reconstruction installs a new aggregate only after all 120 orders complete. An interrupted reconstruction
keeps the prior frontier certificate. Both paths preserve the actual domain-completion state. Other terminal
objectives first attempt the complete terminal-payoff projection described below, then retain their existing
complete probability-law refinement.

Ambiguous candidates with smaller current certified payoff upper bounds receive refinement first, with
candidate identifiers breaking ties. Their upper bounds are closer to exclusion by a competitor's proved
lower bound. This priority uses the complete frontier and preserves every existing certificate.

Among the orders requiring refinement, the search first evaluates those with the widest payoff enclosure.
Every order has the same weight in the uniform-order objective. Canonical order indices break priority ties.
The interval frontier establishes each returned rank from complete certificates independently of this work order.

Additional synthetic checks cover several draws in one frame, conditional-state isolation, bounded stochastic
depth, initialized-model identity, complete-law reuse, and a complete threshold ranking over a 601-frame schedule
with request cache capacities of zero and 64.

## Complete terminal-payoff projection

For score-threshold probability, capped score and joint score/LIFE thresholds, an optional provider folds
one candidate's own complete observable timelines through its native score calculator. The controller
recurrence retains the full hidden state and the ordered observable prefix until every range finishes.
Terminal states with the same complete timeline contribute disjoint probability masses to that timeline.
Each native integer score is mapped to the requested terminal payoff before its mass is accumulated.
Deduplicated timelines are not equally weighted, and outward masses are never renormalized.

The recording, original clock, probe emission and native score-fold admissions are shared with the paired
score provider. This provider needs no coupling between different candidates or different initial powers.
Each original order still supplies its own complete performer array and controller law. Final LIFE may be
used only after the recorder establishes its deterministic value for that order. A random LIFE mechanism
requiring an additional joint state remains unavailable to this optional projection.

A result is published only after all supported terminals are evaluated exactly once. General weighted
results are outward payoff enclosures; they do not claim an exact expectation or alter the raw score law.
If every complete terminal has the same integer payoff, the provider additionally certifies that constant
exactly. Unit total probability follows from the complete native integer lottery partitions and full
recurrence, rather than from an interval merely containing one. This permits exact zero/one probabilities
or constant capped scores while the underlying random scores remain distinct.

The search tries this projection across ambiguous retained candidates before spending the remaining shared
budget on native RNG-leaf replay. Detailed completed orders narrow their own payoff certificates immediately;
an aggregate reconstructed without retained order rows is installed only when every order completes.
An unresolved nonconstant enclosure preserves the threshold/truncation metadata needed by later refinement.
No score-law equality class is merged by a payoff-only certificate. Cancellation, unsupported emission order,
capacity refusal and exhausted work preserve all earlier certificates and the original completion state.

During traversal, once the frontier contains at least K candidates, the just-offered nonlinear candidate
can receive one complete optional terminal-payoff stage. Its already available 120 raw-score rows are kept
for that stage; the long-chart policy retains at most one such row set. The stage stops if the candidate
leaves the frontier, a deadline/cancellation occurs, or a work guard refuses further computation. It does
not close the unseen domain or invoke an exhausted-domain proof.

One request-owned native work allowance is shared by early terminal work, final equality refinement and
full native lottery laws. An attempted stage is a scheduling state, not a payoff certificate: only fully
completed individual orders install evidence, and an unfinished stage never restarts its private fold
counters with a new allowance. Remaining unresolved orders can still use the independent native-law
provider. Optional work exhaustion does not stop the ordinary domain traversal. A removed candidate's
attempt marker never suppresses proof work for a later physical candidate with an equal program.

The initial candidate evaluation for these objectives can also use a complete raw-score terminal summary.
That summary provides the score mean, native integer support and deterministic LIFE required by the existing
per-order payoff bounds. A mean is never substituted for the expectation of a nonlinear payoff. Optional
terminal-summary refusal uses factor-history replay; a requested fresh summary refinement still executes
that separate replay enclosure. Final mapped-payoff and full-law refinements retain their own budgets and
complete-order requirements.

## Profile command bounds and leaf reuse

A completed controller profile can additionally bound the number of direct probe activations. Its original
frame transition masks define a two-state layered graph, starting with the probe inactive. A max-plus pass
counts false-to-true transitions. Combining edges from different hidden states can add impossible paths, so
this graph supplies an upper bound without replacing the controller's probability law. Every original frame
must be present, all terminal paths must close inactive, and every frame beyond the music boundary must
remain inactive. The complete physical pair domain must prove compatible fixed probe predicates and phases.
Missing evidence in any original order preserves the previous command allowance.

For each actual five-member/Snap binding, eligible untimed probe rows use the smaller of their original
command allowance and the complete profile's activation bound. Ordinary rows and unsupported timer, release,
reset or limit lifecycles retain their existing allowances. Per-position maxima cover all 120 shuffles, and
the original floating-point error derivation is recomputed from those conservative command counts. This
unweighted error allowance remains separate from expected score coefficients. The smaller independently
valid bound is used; no probability scales away a possible arithmetic history.

### Historical direct-probe rewards

A further capability can use the complete controller law for the direct probe's historical rank contribution.
For an included note, let `H` be its nonnegative historical coefficient with the full allowed Rush multiplier,
and let `p` be the complete law's probability that the direct probe is active at that note's chart time.
When the native historical query has the same ideal signed probe prefix as that chart-time class, `H * p`
bounds the expected probe contribution. Without the separate Rush-filing capability below, ordinary reward coefficients keep `H` and historical
Rush magnitude stays unconditional. Native floating-point drift and integer rank allowances always retain
their independent envelopes.

An empty-reward recording establishes only original clock and rank-query geometry. The full physical pair
domain must separately establish a direct positive 7021 effect, an untimed sustained lifecycle, fixed
predicates, a common native phase and paired inverse filings. The geometry check verifies every original
frame and Ready event, both adjacent rank-query identities, and complete cancellation of prior fixed-rank
identities and coefficients. Every note in their native closed-frame difference must already have filed.
Every possible later probe timestamp, including an inverse clamped to music length, must be strictly later
than each included note. A later note sharing the end query's ceil frame is a refusal even if the note is
present by the final query.

Only complete order laws with the admitted initial, terminal and music-boundary probe states can use this
witness. Missing geometry or lifecycle evidence retains the unconditional historical coefficient. Every
physical probe reward keeps the smaller of the complete conditional and unconditional arithmetic envelopes.
The original 120 labels contribute in the same outward summation order.

`rankProbeHistoryReadyLabels` counts ready labels in successfully bound complete profiles; a newly bound
profile can repeat a law used by another physical family or writer binding, and rebuilding an evicted profile
counts its new publication. Each published profile can have zero through 120 ready labels; unavailable labels
retain their original historical coefficient within the complete average. `rankProbeHistoryDiscountedProfiles` counts such
profiles with a positive coefficient reduction. `maximumMeanUnitProbeHistoryReduction` is the largest mean
reduction for a unit-amplitude probe covering the chart, rather than a physical team's saved score. Cache
hits on completed coefficient tables do not recount these profiles. A transported controller program still
counts the new physical reward binding it publishes. Native nominal-tree tests compare actual signed probe filings
at both historical queries and the terminal query over all original labels; descendant tests independently
check both the historical-only and combined upper bounds.

### Historical Rush rewards

A separate complete-profile capability can attach the controller's Rush class to each historical rank query.
Its empty-reward recording certifies the original query geometry; the full admitted controller profile
certifies which later Rush filings are possible. For every included note, every potential later filing and
actual nonzero Luck factor after the original start-query event must lie in a strictly later native score
frame. The comparison uses the query event, so multiple controller switches already filed in the same frame
are retained. Missing or inconsistent geometry refuses the whole optional witness.

Potential filings include pending notes, current judged Luck notes, pending lotteries and before-frame
completion opportunities for every range, including non-Luck ranges. Actual recorded nonzero Luck factors
are additional conservative filing opportunities. This coverage relies on complete profile
admission, including rejection of overlapping active Luck ranges and range-clock feedback; an empty context
recording alone never grants the capability. The fixed-rank identity/coefficient cancellation proof remains
required for both historical queries. Probe and Rush readiness can fail independently.

Let `r` be the rank-only upper multiplier compiled directly from the original rank envelope. Let `T[R]` be
that note's terminal coefficient under Rush class `R`, including the ordinary coefficient when Rush is off.
The normal historical contribution becomes `r * E[T[R]]`. A direct probe with both witnesses uses
`r * E[T[R] * I(probe)]`, evaluated from the complete joint law. It never multiplies independent marginals.
With Rush readiness alone, a possible probe keeps its full physical amplitude and uses the normal history
coefficient. With probe readiness alone, it retains the earlier `H * P(probe)` envelope. With neither,
it keeps the unconditional `H`.

The whole new physical-pair bound is intersected with the complete earlier pair bound. The base coefficient
also keeps the smaller complete old/new envelope. Ordinary skill windows, conversion budgets, opaque rows,
unweighted native arithmetic drift and integer rank allowances are unchanged. Each original order still
contributes in the same outward summation sequence; a cached curve additionally matches Rush readiness.

`rankRushHistoryReadyLabels` and `rankRushHistoryDiscountedProfiles` count successful complete profile
publications under the same accounting rules as the direct-probe diagnostics. The
`maximumMeanBaseRushHistoryReduction` diagnostic measures only the average base coefficient decrease over
120 original labels. It excludes team power, physical reward amplitudes and native error allowances; it is
not a saved-score or saved-time estimate. Native tests compare historical signed Rush and probe prefixes
with complete terminal joint laws, and the physical descendant oracle checks each independently enabled
capability as well as the combined bound.

The completed coefficient table also bounds individual physical leaves. A depth-four node may stay open
because one remaining binding is strong, while another child can be excluded using its own singleton cap.
This leaf lookup uses only the same immutable compiled scope and already completed cached profiles. It runs
no additional recording, extends no budget and inserts no cache entry. A missing table or profile restores
the ordinary leaf path. Canonical ties retain the existing strict payoff and power comparison rules.

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
