# Power-parameterized score programs

The `search-diagnostics` feature can record one declared solo execution as an
exact score expression. `LiveModel::compile_score_program` consumes a fresh model,
runs the supplied play with its complete post-shuffle random state, and returns
the recorded `ScoreProgram` plus the normally executed terminal model.
`ScoreProgram::evaluate` changes only initial total power.

This is a separate building block for reducing repeated simulation. It does
not participate in production search or merge deck candidates.
An optional checked certificate can prove nondecreasing score on a supplied
power interval; ordinary evaluation makes no monotonicity assumption.

## Control flow and identity

For the supported judged-stream path, total power initializes the score
calculator. The complete `Performer` vector determines skills, their order and
all target attributes. Conditions, conversion, life, combo and lottery updates
cannot read total power. Score reads feed range snapshots and score reporting;
native solo ranking confirms rank 1 independently of the score.

Each program belongs to that entire model and execution context: master data,
ordered performers, chart, play, clocks and complete native random state.
Sharing a skill-order permutation is insufficient because the remaining random
state can differ. Sharing a search-bound class is also insufficient. A future
program cache must prove equality of this complete context before reuse.

Recording rejects models that have started, raw input callbacks, external
rank confirmation and externally supplied lifecycle state. The returned terminal
model still describes the original power; its recorded numeric score snapshots
are not the terminal model for a different power.

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
wrapping-subtract and rank-percentage nodes. Unreachable nodes are removed.
Historical add/subtract pairs are not yet normalized away. Compilation checks
that evaluating the program at its original power exactly reproduces the
recorded terminal score.

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

## Search integration still requires program identity

Within an identical program, certified monotone score plus the score
objective's secondary descending-power order would permit power-based Top-K
binding recovery. Physical identities, resource uniqueness and canonical ties
must still be retained. PT additionally depends on the physical event bonus and
must be calculated for each native-root outcome before weighting; monotone score
alone does not order all PT bindings.

The `score_program` harness binary compares each recorded program against fresh
complete runs at an explicit list of powers. Deliberate negative/overflow probes
test expression fidelity; they are not legal-roster examples. Numerical replay
equality is distinct from game parity, cross-deck equivalence, monotonicity and
end-to-end search speed.

`ClassKey`/`RowSig` is insufficient for this identity contract.
Bound preparation can omit inert rows and records only fields needed by the
relaxation. Exact execution must also preserve update phases, skill boundaries,
effect order and conversion ownership aliases, including equality of raw effect
IDs across skill tables. Pre-live effects with no score applier can still run
trigger, condition, reset, release and cumulative machinery. A future compiler
must preserve these semantics before grouping them.
