# Absolute execution controls

Standalone benchmark crates keep timing and allocation instrumentation out of the
engine. Generated inputs, copied worker sources, binaries, profiles and samples
belong in an ignored `target/` directory. Rust 1.98.1 and cached dependencies are
required. The sibling `jkq` checkout is read only to this tooling.

## Current public API

```sh
python3 benchmarks/absolute/prepare.py target/absolute-run --jkq ../jkq --default-only
ABS_CASE='field/500|project/1024/40/direct|worker/fold/8' \
  ABS_MS=500 ABS_SAMPLES=5 \
  target/absolute-run/default-harness/target/release/jx-absolute-default
ABS_CASE='scalar/500|project/1024/40/direct' ABS_PHASE=compile \
  target/absolute-run/default-harness/target/release/jx-absolute-default
```

`default.rs` uses ordinary `CompileOptions::compile` and `Expression::evaluate`.
The `worker/*` controls copy the actual sibling transformation worker and use its
public `InputPlan`. Both now compile the common semantic IR. There is no engine
selection in this build. Compare retained binaries from distinct source revisions
with identical harness sources, lockfile dependency versions and compiler settings.
Run reversed independent-process orders; preserve raw CSV and binary/source hashes.
Do not time while builds, validation or profiles run. Process ranges are not
confidence intervals. Record power state and exclude sleep transitions.

Each timed ordinary evaluation includes complete JSON validation, acquisition,
evaluation and compact NDJSON serialization into a reused output buffer. The
compile phase excludes evaluation and input processing. Worker measurements include
payload copying, shared preparation, ordered predicates, tombstones, owned output
and buffer recycling. Kafka polling, compression, scheduling and physical output
are excluded. Validate these separately using the real application.

`metrics.rs` disables allocation counting during throughput timing and separately
probes allocation count, allocated bytes and live peak heap after warmup. Report
wall and process CPU time, actual input/output lengths and bandwidth. The probes
exclude code, constants and warmed reusable output capacity; they are not RSS.

## Fixtures and semantic obligations

`fixtures.rs` defines deterministic, complete inputs and JSONata source. Scalar and
lookup records are 500 B through 1 MiB, with a 4,096-entry immutable label table
and 25% misses. Projections cover 8/24/32/40/80 metrics, nested fields, Unicode
uppercase, conversions and lookup, using direct, lexical, conditional and function
forms. Sequence cases cover empty, singleton and 2/8/128/1,024/8,192 rows, 0/50/100%
selectivity, numeric and filtered folds, aliases, callbacks and constructed output.
Nested controls exercise correlated ancestor navigation. Read actual generated
lengths rather than treating a size in a case name as an exact byte count.

Worker fixtures use the same complete validation, action selection, serialization
and ownership obligations for each engine revision. Assert action and JSON output
against the handwritten result before timing. They remain controlled distributions,
not production-traffic replays. The public projection worker has one ordered drop
and one tombstone predicate. The larger application fixtures additionally contain
compound whitelist/blacklist keys and separate lookup tables; use the real Kafka
application for offsets, ordering, shutdown and backpressure acceptance.

## Additional diagnostic references

The full diagnostic crate and schema-assuming Rust/SIMD references remain available:

```sh
python3 benchmarks/absolute/prepare.py target/absolute-reference --jkq ../jkq
ABS_CHECK=1 target/absolute-reference/harness/target/release/jx-absolute
ABS_MODE=simd-copy ABS_CASE=worker/project/1024/40 \
  target/absolute-reference/harness/target/release/simd-reference
```

Historical `legacy`, `foundation` and `facade` labels in diagnostic output name
public entry points, all of which now use the same engine. They cannot compare
retired execution systems in the current source. Use separate revision binaries
for that comparison. `run.py` retains the diagnostic phase/profile commands; the
public-only commands above are the release controls.

Handwritten Rust references deliberately assume these fixture schemas and numeric
types. SIMD variants can mutate a scratch copy; typed variants differ in field
validation, coercion, effects, alias identity, error order and invalid-shape handling.
They are performance controls, not semantically equivalent JSONata implementations.
Do not attribute their entire advantage to engine overhead. No reference changes
production execution, dependencies or default configuration.
