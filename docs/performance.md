# Performance and efficiency

## Current measurements

| Workload | Input bytes | Mode | Records/s | Input MiB/s | Allocations/record |
| --- | ---: | --- | ---: | ---: | ---: |
| Shallow path | 500 | normal | 3.21 M | 1,529 | 0 |
| Scalar arithmetic | 1,024 | normal | 1.61 M | 1,572 | 0 |
| 8,192-key static lookup | 500 | normal | 2.82 M | 1,343 | 0 |
| Filter/map/sum, 1,024 rows | 41,828 | normal | 5.92 k | 236 | 0 |
| Constructed object | 500 | normal | 2.45 M | 1,168 | 3 |
| Numeric filter/map/sum, 1,024 rows | 14,794 | native | 10.24 k | 144 | 0 |

The table reports medians across two passes, each with seven 300 ms warmed samples.
It includes full input validation and result consumption, not compilation, serialization
or I/O. Allocation counts include allocator growth/reallocation; requested bytes are
not retained heap or RSS. The constructed object uses 152 requested bytes per record.
Pass medians differ by 0.2–4.5%; the table rounds rates accordingly. These measurements
are scoped examples, not portable throughput guarantees.

[Raw samples, selections and summary](../benchmarks/current) and the
[reproduction environment](../benchmarks/current/environment.json) identify source,
compiler/target, build settings, executable hashes and machine/OS. Hardware details
are retained there only for reproduction.

| Selection | Expression/fixture |
| --- | --- |
| `ascii/shallow`, 500 B | `id`, one borrowed scalar |
| `execution/arithmetic`, 1 KiB | Repeated numeric field loads/arithmetic, one primitive result |
| `acquisition/lookup_8192_short_varying`, 500 B | Compiled 8,192-key object, rotating hit/miss keys |
| `regions/filtered_sum`, 41,828 B | `$sum($map($filter(rows,…),…))` over 1,024 nested rows |
| `execution/constructor`, 500 B | Fixed schema plus computed arithmetic member, one object |
| `jit/filter_map_fold`, 14,794 B | Native-enabled numeric filter/map/sum over 1,024 rows |

Normal/native rows use different fixtures; their rates describe each workload independently.
The measurements use the normal build for ordinary rows and a `jit` build with native
execution enabled for the final row.

`jx` is designed for one compiled expression evaluated over many independent records.
Compilation can spend work on analysis so repeated execution stays small. Performance
depends on expression shape, record layout, result size and the caller's consumption.
There is no universal comparison with other query engines.

## Where work is saved

- Constants, constructors, regexes, pictures and static lookup tables are prepared once.
  Static object lookup uses a compact fingerprint index with exact UTF-16 equality checks.
- Validation captures demanded fields/paths where possible. Repeated planned loads and
  pure call arguments can reuse spans instead of scanning the object again.
- Raw input and unescaped strings stay borrowed. Primitive scalar operations do not need
  boxed values; immutable construction retains borrowed leaves.
- Sequences stream when cardinality and effects allow. Selected filter/map/aggregate
  regions fuse iteration and primitive computation without intermediate collections.
- Bounded plans share loads/computations and reduce repeated tree dispatch. Unsupported
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

Unplanned consumers and nested-array/cardinality operations can still rescan. Genuine
escaping closures and retained outputs must keep their data alive. Effects, errors,
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

To reproduce a table row, use its exact workload selector and input size:

```sh
JX_BENCH_FILTER=ascii/shallow JX_BENCH_BYTES=500 JX_BENCH_SAMPLE_MS=300 just bench
JX_BENCH_FILTER=jit/filter_map_fold JX_BENCH_BYTES=14794 JX_BENCH_SAMPLE_MS=300 just bench-jit
```

Repeat all six selectors from `benchmarks/current/controls.json` in reverse order for
the second pass. Refresh this small current measurement set when publishing new figures;
keep development comparison snapshots outside user documentation.

Benchmarks live in `crates/jx/benches`, with workloads separated by semantic area.
They cover compilation, validation, paths/sequences, scalar kernels, callbacks,
filter/map/folds, lookup tables, constructors, pictures, sorting/grouping and transforms.
Fixtures range from tiny records through typical 500 B–1 KiB records to 1 MiB and wide
nested arrays. Purpose-written Rust controls exist for selected transformations; they
do not implement all JSONata shape/error rules.

```sh
just bench
JX_BENCH_FILTER=arithmetic JX_BENCH_BYTES=500 JX_BENCH_SAMPLE_MS=300 just bench
just bench-runtime
just bench-runtime-memory
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
