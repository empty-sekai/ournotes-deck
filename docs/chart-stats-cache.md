# Chart statistics program cache

`chart-stats` can retain the expensive results of individual measurement programs between builds:

```sh
ournotes-deck chart-stats --data deck-data.json --jobs 4 \
  --stats-cache build/chart-program-cache -o chart-stats.json
```

The output remains `ournotes-deck.chart-stats/3`. It contains nominal expectations under independent lottery and
skill probabilities. `--seeds` controls the separate replay seed list; it does not control the precision or work
budget of these expectations.

## What is reused

The cache stores three kinds of completed computation:

| Record | Reusable work |
| --- | --- |
| Expectation | One compiled formation and ordinary-skill program at a specified power, play and rank schedule |
| Run | A deterministic whole-live run, including its integer score, range counters and converted judgements |
| Curve | Lottery-state propagation for the complete transcript recorded by the reduced native interpreter |

No-skill baselines, each ordinary kind at each position, each Gekisou skill variant, and their plain-kind cross
terms therefore reuse work independently. Adding a skill does not by itself invalidate the other programs.
Changing an effect or one of its resolved condition, target, cumulative, timing or score dependencies changes
the program identity. The probability tier can share a curve between different score programs when their
recorded lottery transcripts are identical.

An expectation cache hit is checked before probability construction. A warm build skips both propagation and
score recording for that program. A curve hit still records the current interpreter transcript before reusing
its propagation result.

Every build reads and validates its current input and creates the current metadata, kinds and shapes. Shape
indices, skill membership lists and check decks are assembled again. If an index or a real skill value changes
the generated check deck, that check uses its current program identity. The cache does not preserve an old
chart header or copy a check from a differently indexed skill.

## Identities and storage

Program identities describe the initialized interpreter and its play schedule, including resolved formation
predicates and condition checkers. Source identifiers that only name interpreter objects are normalized while
their alias relationships and execution order are retained. Expectation identities also retain the relevant
lottery classification used by the nominal expectation evaluator. Curve identities use the complete recorded
transcript, including the current probe layout.

Every key includes the cache schema, chart statistics format and model source SHA-256. A new model source uses
a separate namespace. The cache does not import older whole-chart statistics records. It never searches other
master versions for an approximately matching result.

The payloads store original integer results and binary64 interval endpoints as integer bit patterns. Decimal
rounding happens only when assembling the final statistics document, exactly as in a run without a cache.
Each record carries its key, model identity, schema and payload SHA-256. Truncated, structurally invalid or
checksum-mismatched records are misses and are replaced after a successful computation. A record larger than
64 MiB is computed normally without being persisted.

Completed records are written to unique temporary files, flushed and atomically renamed before the next
computation returns. An error or interruption preserves all earlier committed programs. One cache instance
can be shared by chart workers; requests for the same key are synchronized. Separate processes may compute
the same missing program concurrently, but publish complete records through independent temporary files.
The cache is disposable: deleting its directory changes the amount of work, not the output.
Records are retained across input changes and there is no automatic garbage collection. This preserves programs
that another chart, region or later input can still reuse. Operators can remove an obsolete source namespace or
the complete cache directory when reclaiming storage.

## Rust API

The original functions remain available. Call the corresponding cache-aware entry point to retain programs:

```rust
use ournotes_sim::chartstats::{self, ChartStatsCache, Options};

# fn build(data: &ournotes_sim::data::DeckData) -> Result<(), ournotes_sim::Error> {
let cache = ChartStatsCache::new("build/chart-program-cache")?;
let document = chartstats::document_with_cache(data, &Options::default(), &cache)?;
let counters = cache.snapshot();
# let _ = (document, counters);
# Ok(())
# }
```

For parallel callers, `chart_stats_with_cache(master, chart, kinds, options, &cache)` measures one chart using
the same shared instance. `chart_stats_with` and `document_with` remain the uncached entry points.

`snapshot()` reports `requests`, `hits`, `computed`, `writes`, `invalid` and `bytes`. Requests count calls through
the three cache tiers; computed counts successful computations, writes counts committed records, and bytes
counts their serialized size. Unsupported or failed computations are not stored and are not successful
computations. The CLI prints these counters to stderr, leaving the JSON output unchanged.
