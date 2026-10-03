# Native comparison tools

This directory contains only resource-independent Python tools. It does not ship client binaries, master data,
chart assets or native captures; the inputs are supplied from your own copy of the client.

`make_source_manifest.py` hashes only explicitly supplied `--resource ROLE=PATH` files and records the provided
client/master/model identities. It copies neither their content nor their local paths. `inputs_recorded` is an
identity record, not a test pass; absent resources yield `not_run` and exit code 2. With working-tree changes,
record `--model-dirty` and supply the exact model/driver source or patch as separately hashed resources.

`compare_native_frames.py` compares ordered JSON frame captures using an explicit contract. `frame` and `timeMs`
are mandatory. Integer values and declared float32 bits must match exactly; wildcard fields include every array
element in order. An extra pool instance is a length difference. A missing requested field is `incomplete` and
cannot pass. Nonfinite float32 patterns cannot pass even if their bits match. A missing capture is `not_run`. Exit codes are 0 for equality within the declared contract, 1 for
differences and 2 for invalid, incomplete or absent input. Its JSON output retains the first and all differences,
input/contract hashes and the coverage qualification. It never shifts frames or applies tolerances.

Example using explicit external paths:

```text
python compare_native_frames.py --native NATIVE_CAPTURE.json --rust RUST_CAPTURE.json --contract ordinary-contract.json --output comparison.json
python -m unittest -v test_compare_native_frames.py
```

`ordinary-contract.json` specifies the existing model fields used by the heterogeneous ordinary-skill matrix.
Native elapsed/countDecrease fields are excluded explicitly; they are not reconstructed on the Rust side.
Its `arrayLengths` requires all 35 pool instances, so matching empty pools cannot vacuously pass. Other skill
configurations need their own explicit pool length; do not silently reuse the 35-instance contract.
The count of field comparisons describes checks, including identities and inactive pool state; it is not a count
of independent samples or a statistical proof of whole-game equivalence. A model trace must identify its actual
capture phases and input mechanism. Rust consuming native judgements validates downstream arithmetic and state
transitions, not independent touch classification.

[docs/native-validation.md](../../docs/native-validation.md) describes the comparison method, how to record both traces
and how to replay the unit-level reference vectors.
