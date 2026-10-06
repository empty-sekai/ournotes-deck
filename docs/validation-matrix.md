# Source-linked frame validation

`tools/native-validation/run_matrix.py` compares an explicitly enumerated collection of caller-supplied reference and model captures. A matrix passes when every listed case has the declared source and input identities, context and frame coverage, and every requested field agrees exactly.

The manifest format is `ournotes-deck.frame-validation-matrix/1`. The root declares `modelSourceSha256` and a nonempty `cases` array. Each case supplies:

| Field | Meaning |
|---|---|
| `id` | Unique nonempty case identifier |
| `reference`, `model`, `contract` | JSON paths relative to the manifest |
| `inputIdentity` | Identifier shared by both captures and the case |
| `context` | Object with string `region`, `clientVersion`, `masterVersion`, `scenario` and `play` fields |
| `frameCount` | Positive number of frames in the full declared trajectory |
| `finalTimeMs` | Exact terminal frame time |

Both captures contain `chartId`, `inputIdentity`, `context` and `frames`. The model capture also contains `modelSourceSha256`, obtained from `ournotes_sim::SOURCE_SHA256` in the executable producing that capture. Frame indexes are consecutive from zero and frame times are ordered. The contract uses the integer-field and binary32 bit-field schema of `compare_native_frames.py`, including at least one state field in addition to `frame` and `timeMs`.

```sh
python3 tools/native-validation/run_matrix.py work/frame-matrix.json work/frame-result.json
python3 -m unittest discover -s tools/native-validation -p 'test_*.py'
```

Exit code 0 denotes a passed matrix; 1 denotes value differences; 2 denotes incomplete or invalid inputs. The report preserves every case outcome and the digest and size of each input. Source metadata identifies the declared executable inputs; establishing the capture producer and build relationship remains a separate responsibility. Equality applies to the declared fields, versions, trajectories and scenario conditions.
