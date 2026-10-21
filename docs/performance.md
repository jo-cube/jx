# Performance and efficiency

`jx` is designed for one compiled expression evaluated over many independent records.
Compilation can spend work on analysis so repeated execution stays small. Performance
depends on expression shape, record layout, result size and the caller's consumption.
There is no universal comparison with other query engines.

## Where work is saved

- Constants, constructors, regexes, pictures and static lookup tables are prepared once.
  Static object lookup uses a compact fingerprint index with exact UTF-16 equality checks.
  Immutable nested object tables retain their snapshot owner instead of being copied
  into each expression or clone.
- Validation captures demanded fields/paths where possible. Repeated planned loads,
  pure library/call arguments and bounded pure root constructor regions reuse spans
  instead of scanning the object again. Root capture is stack-scoped with no added heap
  allocation; callbacks and dynamic contexts retain their existing execution paths.
  Traversal of validated input skips subtree boundaries without parsing their grammar
  again, reducing work for callbacks, navigation, sorting/grouping and transforms.
- Raw input and unescaped strings stay borrowed. Primitive scalar operations do not need
  boxed values; immutable construction retains borrowed leaves. String conversion
  avoids a UTF-16 round-trip and reuses its UTF-8 text buffer when escaping permits.
- Sequences stream when cardinality and effects allow. Selected filter/map/aggregate
  regions fuse iteration and primitive computation without intermediate collections.
- Bounded plans share loads/computations and reduce repeated tree dispatch. Grouped
  static filter sources reuse fold lowering; proven numeric local declarations use
  registers. `InputPlan` shares acquisition with scalar, fold and constructor plans. Unsupported
  shapes retain normal tree execution.
- Frames recycle or retire when captures no longer escape. Ownership is paid for where
  lexical retention, sorting, grouping, construction or detached snapshots require it.

Allocation budgets are executable assertions in the benchmark suite. Ordinary paths and
eligible scalar/fold plans have zero-allocation cases; this is not a promise for every
expression. Construction, string decoding/conversion, sorting, grouping, callbacks and
owned snapshots can allocate.

## Trade-offs

| Workload | Main cost / useful approach |
| --- | --- |
| Small static scalar expressions | Validation, demand acquisition and primitive execution; plans can share field loads |
| Large records, few demanded fields | Complete validation remains necessary; capture removes extra traversals but cannot skip bytes |
| Numeric filter/map/folds | Fused plans reduce per-item dispatch; native execution can help suitable kernels |
| Strings and general callbacks | Decoding/conversion, projections, invocation and retained values remain Rust work |
| Sorting/grouping | Candidates must be retained; pure keys can be reused, observable callbacks cannot |
| Transforms | Clone conversion and rebuilding affected containers remain necessary |

Unplanned consumers and nested-array/cardinality operations can still rescan. Numeric
plan guard failures may replay a pure prefix through the tree to preserve errors.
Selected immutable object views retain their entire snapshot owner; eager map results
still retain their sequence. Genuine escaping closures and retained outputs must keep their data alive. Effects, errors,
randomness and dynamic calls cannot be memoized to improve a benchmark.

## Optional native execution

Default builds are independent of Cranelift. The `jit` feature plus explicit enablement
accelerates eligible numeric/boolean registers and selected folds; it does not replace
the JSONata runtime. Validation, traversal, strings, ownership and dynamic/effectful
operations remain in Rust. Unsupported regions and guard failures use normal execution.

Native execution helps most when suitable numeric work repeats many times per record
or the expression is reused over many records. Scanning-dominated workloads may gain
little. Measure installation cost separately from warmed evaluation; compilation is
not a per-record operation. Keep native opt-in and compare it with the same plan binary.

## Measuring

Use the workspace release profile and record compiler/target, host, source revision,
feature flags and executable hashes. Keep compilation, profiles and allocation probes
outside timed execution. Repeat warmed samples with both version orders and retain
raw samples under ignored `target/` or external review storage.

The [experimental execution workflow](../benchmarks/execution/reproduce.md) covers
matched legacy/foundation execution, compiler scaling, allocation/heap probes and
isolated transformation workers. The foundation remains opt-in; semantic coverage
and output obligations must match before comparing rates. Projection throughput
does not establish compatibility or end-to-end broker capacity.

Benchmarks live in `crates/jx/benches`, with workloads separated by semantic area.
They cover compilation, validation, paths/sequences, scalar kernels, callbacks,
filter/map/folds, lookup tables, constructors, pictures, sorting/grouping and transforms.
Fixtures range from tiny records through typical 500 B–1 KiB records to 1 MiB and wide
nested arrays. Purpose-written Rust controls exist for selected transformations; they
do not implement all JSONata shape/error rules.

```sh
just bench
JX_BENCH_FILTER=arithmetic JX_BENCH_BYTES=500 JX_BENCH_SAMPLE_MS=300 just bench
JX_BENCH_ROOT_ONLY=1 JX_BENCH_FILTER=root_regions/string JX_BENCH_BYTES=500 just bench
just bench-runtime
just bench-runtime-memory
JX_BENCH_REALISTIC_ONLY=1 just bench
JX_BENCH_REALISTIC_ONLY=1 just bench-runtime-memory
just bench-embedding
just bench-plan
just bench-jit
just bench-jit-memory
```

Throughput CSV reports records/s, input bytes/s, allocations and requested bytes per
record. Compilation has separate workload rows. The normal harness warms up, then runs
seven samples; `--smoke` runs short allocation/correctness checks rather than a timing
comparison. Most execution rows consume results without serialization or I/O;
`identity_write` explicitly includes compact output into a reused buffer.

For a reproducible comparison, retain the commit/dirty diff, commands, compiler/target,
release settings, machine/OS, workload sizes and raw samples alongside your result.
Save local output under `target/`, use matched inputs/builds, reverse comparison order,
and repeat meaningful changes. The current representative selections are in
`crates/jx/benches/controls.json`; reusable tools preserve raw samples and compare medians:

```sh
# Use executables printed by cargo bench --bench throughput --no-run.
python3 scripts/compare-benchmarks.py BASELINE_BINARY CANDIDATE_BINARY \
  crates/jx/benches/controls.json target/comparison
python3 scripts/compare-native.py NATIVE_FEATURE_BINARY target/native --ms 300
# Repeat each with --reverse to check ordering bias.
```

Profile long warmed runs of the same executable with your platform profiler (`sample`
on macOS, `perf` on Linux). `JX_BENCH_FILTER`, `JX_BENCH_BYTES` and
`JX_BENCH_SAMPLE_MS` select/extend an individual workload. Keep full traces; inlining
and unsymbolized native code can make aggregate attribution approximate.
Requested/peak live heap is not RSS; `runtime_memory` measures both transient and escaping
captures, while `native_memory` measures code installation/sharing/retention.

For native/interpreter comparisons, `just bench-plan` and `just bench-jit` select the
same kernel fixtures with execution disabled/enabled in the same feature build. Record
code size and compilation cost. Amortization is compilation seconds divided by saved
seconds per record; a gain inside a kernel may disappear end-to-end when scanning dominates.
