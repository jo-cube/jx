# Current performance

The intended workload is compile once/evaluate millions of independent records, usually
500 B–1 KiB. Complete UTF-8 JSON validation is included. The current engine combines
specialized paths/demand capture, a bounded scalar/loop plan, and tree fallback.
No worker pool or universal input DOM/index is used.

These measurements are scoped controls from an Apple M4 arm64 macOS machine, Rust
1.98.1, release thin LTO, seven 300 ms samples, with repeated comparisons in both orders.
They measure warmed evaluation and consumption, **not CLI I/O or serialization**.
See [recorded environment and commands](../benchmarks/m30/environment.json) and
[detailed results/profiles](../PERFORMANCE.md#runtime-consolidation).

| Workload | Current evidence |
| --- | --- |
| 500 B scalar arithmetic | Interpreter ~2.69 M records/s; optional native ~2.93 M/s; validating fixed-shape Rust control ~3.03 M/s |
| Numeric kernels | Optional native improves 500 B arithmetic ~9%, 1 KiB ~5%, object folds 54–59%, numeric folds 69–71%, dense filter/map/fold 34–36% |
| 1 MiB sparse arithmetic | Scanning dominates (~100% profile samples); native does not improve throughput |
| Pure dynamic object callbacks | Avoiding frame copies improves throughput 38–66% across 8–16,384 rows; at 1,024 rows allocations fall 14,426 to 7,258 |
| Retained output | A 16,384-row dynamic constructor fixture still peaks near 3.80 MB; output lifetime dominates memory |

Ordinary path/scalar/native controls retain zero per-record allocations. Construction,
lexical capture, dynamic programs, grouping/sorting and owned strings/results can allocate;
allocation assertions and live-heap fixtures distinguish counts, requested bytes and
retained output. Requested heap is not RSS. The Rust control does not implement general
JSONata shape/error semantics, and these figures are not cross-engine marketing claims.

Remaining costs are mostly complete validation/traversal (often 58–99% of sampled time),
string callback projections/conversions, non-lowered dynamic bridges, genuine captures,
effectful comparators and transform reconstruction. Small build-level callback deltas
remain: a three-item numeric partial fixture is ~2% slower, with wider maps steady;
a 500 B string callback is ~1% slower. Repeated historical regressions were inconsistent;
removed experiments and limits of attribution are recorded with the evidence.

## Native policy

Default builds do not include Cranelift. `jit` is an explicit Cargo feature, and callers
also opt into code generation with `enable_native()` / CLI `--jit`. It compiles only
bounded primitive numeric/boolean regions. JSON scanning, sequences, strings, allocation,
dynamic calls/effects, provenance and general constructors stay in Rust. Guards and
compilation failures preserve interpreter fallback; compiled expressions share code safely.

Native code helps dense numeric work most. Warm kernel installation in the recorded experiment cost ~0.25–0.39 ms, amortizing
after ~8,000–13,000 typical scalar records or tens of large folds. This is a
per-expression cost; amortization depends on the eligible region and record size. Compilation-cost/code-size
measurements are in the [native experiment](../benchmarks/m26/environment.json).
Measure your own expression before enabling it. Native execution is not selected
implicitly and unsupported target requests fail clearly at build time.

Run `just bench` for the full default suite, `just bench-plan` / `just bench-jit` for
same-binary numeric comparisons, and `just bench-runtime-memory` for live heap.
Benchmark compilation separately; preserve toolchain, inputs, flags, sample count and
record sizes. End-to-end CLI throughput includes framing/serialization/I/O and should
not be inferred from library evaluation numbers.
