# Performance

Optimize repeated single-threaded execution over independent records. Compilation
cost is separate. No speedup claim against another engine is established yet.

## Reproduce

```sh
just all
just bench > /tmp/jx-bench.csv
```

`crates/jx/benches/throughput.rs` is a standalone release harness using `Instant`
and `black_box`. It emits CSV: records/s, input bytes/s, elapsed time, iteration
count, allocations/record and requested allocation bytes/record. Compilation has
its own row (bytes/s is zero). Each workload warms 64 records and takes seven
samples of at least 50 ms, checking the clock after batches of 32 records. The
clock and loop overhead are included; input creation and expression compilation
are outside evaluation timing. Keep raw samples; compare medians and spread over
multiple process runs on an idle machine. Do not optimize within measurement noise.

Fixtures cover exact 100 B, 500 B, 1 KiB, 10 KiB, 64 KiB and 1 MiB ASCII records,
plus structured arrays and Unicode/escaped keys. Each runs validation, identity,
shallow/nested/missing paths, terminal array selection, and identity plus compact
writing into a reused Vec. These are library workloads; record framing, OS I/O
and physical output throughput are excluded. Payload-heavy fixtures alone cannot
predict performance on wide/deep records.

The process-wide counting allocator records allocation/reallocation calls and
requested bytes. Timed workloads are single-threaded; compilation allocates but
implemented repeated evaluation and preallocated output must report zero. Allocation
assertions also run in `just all` via `--smoke`. The wrapper delegates to `System`;
it is the only unsafe code, isolated to the benchmark. Engine and CLI forbid unsafe.
Counter overhead affects allocating compilation; counts are not retained RSS.

## Extending evidence

Add arrays/sequences, arithmetic, comparisons, filters, aggregates, constructors,
functions and mixed expressions with their semantics. Use selected purpose-written
Rust controls when they clarify overhead. Add realistic whole-CLI pipelines and
latency distributions separately; sample duration is not per-record tail latency.
Fair cross-engine comparisons must use identical input, expression semantics,
validation, result cardinality and serialization work, with pinned versions,
commands, machine/compiler details and repeated raw results.

Profile before adding scan fusion beyond the current path, SIMD, indexes, caches,
execution IR or JIT. Allocation is permitted when semantics need construction,
retention, equality, broad sequences or closures. Preserve fast common paths
without complicating them for speculative features.

## Initial baseline — 2026-09-23

Apple M4, arm64, 24 GiB RAM, macOS 26.5.1; Rust 1.98.1 / LLVM 22.1.8,
workspace release profile. One process run, seven samples per workload. Selected
validation + `customer.id` evaluation results (no serialization):

| Fixture | Bytes | Median records/s | Sample range records/s | Median input MB/s |
| --- | ---: | ---: | ---: | ---: |
| ascii | 100 | 10,764,755 | 10,639,076–10,875,430 | 1,076.5 |
| ascii | 500 | 3,265,653 | 3,217,226–3,384,297 | 1,632.8 |
| ascii | 1,024 | 1,730,386 | 1,710,589–1,756,501 | 1,771.9 |
| ascii | 10,240 | 189,021 | 187,629–191,097 | 1,935.6 |
| ascii | 65,536 | 29,834 | 29,721–30,791 | 1,955.2 |
| ascii | 1,048,576 | 1,864 | 1,859–1,890 | 1,954.5 |
| structured | 485 | 1,727,052 | 1,705,275–1,736,446 | 837.6 |
| unicode_escaped_keys | 960 | 1,125,202 | 1,100,003–1,137,432 | 1,080.2 |

All 56 repeated-execution workloads measured zero allocations/record. Compiling
`customer.id` measured four allocations (106 requested bytes) per compilation.
These establish a local baseline, not a speedup or production capacity claim.
[Raw samples](benchmarks/2026-09-23.csv) and [environment/source hashes](benchmarks/2026-09-23.json)
are retained; rerun on the same machine before drawing regression conclusions.
