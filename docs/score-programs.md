# Power-parameterized score programs

`LiveModel::compile_score_program` records one declared judged-stream execution
as an exact score expression. It consumes a fresh model, runs the supplied play
with its complete post-shuffle random state, and returns
the recorded `ScoreProgram` plus the normally executed terminal model.
`ScoreProgram::evaluate` changes only initial total power.

Production search reuses completed lottery-free order programs at each candidate's
actual power, retaining each team's identity and canonical rank.
An optional checked certificate can prove nondecreasing score on a supplied
power interval; exact evaluation requires only the program's execution contract.

## Control flow and identity

For the supported judged-stream path, total power initializes the score
calculator. The complete `Performer` vector determines skills, their order and
all target attributes. Conditions, conversion, life, combo and lottery updates
cannot read total power. Score reads feed range snapshots and score reporting;
native solo ranking confirms rank 1 independently of the score, and declared
external rank confirmations are fixed execution inputs.

Each program belongs to that entire model and execution context: master data,
ordered performers, chart, play, clocks and complete native random state.
The complete random state is part of this identity even when two runs share a
skill-order permutation. Production reuse fixes the execution context within
one request and compares the complete performer values as described below.

Recording accepts fresh judged-stream ordinary Live and Gekisou with solo ranking
or a declared external rank timeline. Started models, raw input callbacks and
already-started lifecycle state return errors. The returned terminal model and its
numeric score snapshots describe the original power; the program supplies scores
at other powers.

## Exact arithmetic under replay

Each note execution records a fresh kernel with the actual binary32 factor state
after all preceding factor updates and undo operations. Its expression preserves
the scorer's multiplication grouping, both integer conversions, positive-infinity
special case and integer wrapping. Changing power never substitutes idealized
real-number arithmetic for these operations.

Undo subtracts the expression for the exact previous execution of that note.
A replay creates a new kernel because undo can change the final bits of the
factor state. A fixed ranking bonus captures the actual start and end score
expression handles, then applies the native wrapping difference and integer
percentage. Later replay does not rebind an earlier bonus to newer snapshots.

The resulting directed acyclic expression has literal, note, wrapping-add,
wrapping-subtract and rank-percentage nodes. Export normalizes additive expressions
modulo 2^32 and removes unreachable nodes. Note kernels combine when all integer
fields and binary32 bit patterns agree. Each rank-percentage input is normalized
separately, preserving its integer division boundary. Compilation checks that
evaluating the program at its original power exactly reproduces the recorded
terminal score.

## Optional monotonicity certificate

`certify_nondecreasing(lo, hi)` returns exact endpoint scores only after proving
nondecreasing score for every integer power in the interval. Otherwise it returns
None. Wrapping additions and subtractions are normalized into checked signed
coefficients on immutable note and rank-percentage atoms. This cancellation is
an exact identity modulo 2^32 even if intermediate score sums wrap.

The certificate requires all surviving coefficients and the accumulated constant
to be nonnegative. Every note kernel must have nonnegative finite factors, a
positive divisor, nonnegative nonwrapping effective power and finite endpoint
intermediates. Each constituent arithmetic operation is then monotone, so the
endpoints bound its entire interval. Saturating finite casts and floors retain
this property; the positive-infinity exception is excluded.

A rank-percentage atom is not expanded through integer division. Its input sum
gets its own certificate, its percentage must be nonnegative and its quotient
must fit i32. The normalized final sum must also fit nonnegative i32 throughout.
These checks make the modular identities equal to ordinary signed sums on the
certified interval. The implementation additionally checks the endpoints against
exact evaluation; those two observations alone are not the interval proof.

Two monotone snapshot values do not make their difference monotone. Negative
surviving coefficients, uncertain arithmetic and relevant wrapping therefore
return None, even when the program might happen to be monotone.

## Production reuse and identity

The request-local cache in `search/program_cache.rs` fixes the master, chart,
play, events, clocks, ranking inputs and every simulator parameter except power.
Its key contains the member identities and their complete `Performer` values,
including vector order, in a canonical member layout. A hash selects a bucket;
each hit compares the complete key. Order indices are relabelled bijectively
between that layout and the candidate's slots.

The cache retains completed orders with zero random draws, including completed
orders of a tree whose remaining orders were pruned or interrupted. Each retained
order supplies its score program and power-independent final life. On a hit,
search evaluates the program at the current candidate's actual power and recomputes
its payoff, including that candidate's event bonus and final-life conditions.
Missing orders remain explicit and receive evaluation or a sound cutoff proof.
A scalar team result enters Top-K only after all 120 order values are available.

Program capture and cache retention have bounded capacity. Admission, oversized
records and eviction determine which computations can be reused; ordinary
evaluation supplies the missing work. Every candidate keeps its own member/Snap
identity and canonical tie key through reuse.

Within an identical program, certified monotone score plus the score
objective's secondary descending-power order supports power-based comparisons
on the certified interval. This is a separate proof from the cache's exact score
evaluation. Binding selection also preserves physical identities, resource
uniqueness and canonical ties. PT depends on the physical event bonus and is
calculated for each performance order before averaging, with its own payoff-order
proof required for power-based binding selection.

The `score_program` harness binary compares each recorded program against fresh
complete runs at an explicit list of powers, including integer-boundary cases.
The comparison checks expression fidelity for each declared execution context.

`ClassKey` and `RowSig` describe fields used by the bound relaxation.
Execution identity preserves update phases, skill boundaries, effect order and
conversion ownership aliases, including equality of raw effect IDs across skill
tables. Pre-live effects also participate in trigger, condition, reset, release
and cumulative execution. The production cache retains the complete performer
identity under the fixed request context for these semantics.
