# Performance and efficiency

`jx` compiles once for repeated independent JSON records. All public entry points
use the common IR and automatic acquisition. Performance depends on input layout,
result size, cardinality, lexical lifetime and consumption requirements.

## Controls

`cargo bench -p jx --bench throughput --locked` runs a compact ordinary-API suite:
validation, compilation, borrowed fields, scalar arithmetic, immutable lookup,
conditional calls, invariant code, 8/32/40/128-field projections and function forms,
numeric/filtered folds, callbacks and captured contexts. Exact expected results
are checked before timing. Smoke mode asserts allocation bounds for completed
fields, scalar kernels, acquisition and compatible sequence regions. Generic
callbacks may allocate for lexical and result ownership; their counts are reported.

```sh
just all
just build
cargo bench -p jx --bench throughput --locked -- --smoke
JX_BENCH_FILTER=projection JX_BENCH_SAMPLE_MS=200 cargo bench -p jx --bench throughput --locked
cargo bench -p jx --bench runtime_memory --locked
cargo bench -p jx --bench architecture --locked
```

`runtime_memory` separately measures allocated bytes, live heap, delivery and
post-drop retention for closures and sequences. `architecture` covers compilation
scaling, entry-point variation and demand acquisition. Unsafe allocation probes
are confined to benchmarks. Engine and CLI code forbid unsafe Rust.

The [absolute harness](../benchmarks/absolute/README.md) provides process CPU clocks,
input/output bandwidth, separate allocation/heap probes and actual copied jkq
transformation workers. Use its default-only build for release comparisons; compile
the identical harness against each source revision. Labels in older two-mode
fixtures are API controls, not alternative engines in the current library.
Schema-assuming Rust references have weaker coercion/error obligations and must
be reported separately from equivalent engine comparisons.

## Method

Record source and binary hashes, Rust version, release profile, hardware, input
shape/size, output bytes and exact commands. Match complete validation, errors,
ownership and serialization. Measure compilation separately. Warm entire fixture
distributions; exclude builds, profiling and allocation counting from throughput.
Use reversed independent-process orders and report ranges. Do not claim tiny
changes from a single process. Requested heap and RSS are different measures;
Kafka native queues and decompression are outside the engine's allocation probe.

There is no universal speedup or zero-allocation promise. General callbacks,
replay, sorting/grouping and escaping lexical values may materialize. Streaming
must preserve cancellation, output prefixes and error ordering. There is no
Cranelift benchmark or native archive.
