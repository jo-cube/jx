# Performance

Optimize compile-once/evaluate-many execution, especially independent 500 B–1 KiB
records. Report scoped measurements rather than general speed claims. Input validation,
output construction and semantic work all count in end-to-end throughput.

## Reproduction

Rust 1.98.1, release thin LTO/one codegen unit. Current evidence is from Apple M4/arm64
macOS; no cross-host claim is established. The benchmark counts allocations/bytes requested,
not allocator size classes or process RSS. Inputs/compiled programs live outside timed
record evaluation. Compilation is measured separately. Record/byte throughput includes
full validation and result consumption; most engine benchmarks omit serialization.

- `just bench`: complete workload suite; `JX_BENCH_SAMPLE_MS=25` is a shorter full pass.
- `just bench-embedding`: result access, bindings, host calls and configured controls.
- `just bench-runtime`, `bench-runtime-memory`: callbacks and peak live heap.
- `just bench-plan`, `bench-jit`, `bench-jit-memory`: identical native/interpreter fixtures.
- `just all`: format, strict Clippy, tests/conformance/native parity and allocation smoke.

Use seven extended samples in both timing orders for regressions. Retain source/binary
hashes, environment, exact commands and results. Never time while builds/profiles or other
benchmarks run. See [historical scoped experiments](benchmarks/HISTORY.md) and their
versioned evidence directories for earlier compiler/traversal/native decisions.

## Current costs and architecture

M27 samples put scanner/traversal helpers at roughly 87–91% in representative planned
scalar/static lookup workloads, and 79% in dynamic numeric map after removing its frame
bridge. String callback boundary helpers account for 47–51% in the profiled string map;
nonprimitive dynamic results still need scope loan/export. These are sampled/inlined
helper categories, not exact instruction-cycle attribution.

Demand capture, primitive callbacks, single-pass nested raw-array flattening, local grouping
indices and lazily retained pure sort keys are already implemented. Unplanned consumers,
variable-derived string projections, cardinality/negative-position replay, effectful sort
comparators, genuine captures/older-frame writes and transform reconstruction remain
structural costs. No evidence currently justifies universal indexing, memoization, GC,
owned short strings, a general IR or broader native coverage.

M27's 16,384-row transient capture peak fell from about 10.35 MB to 0.39 MB; a primitive
reduce fell from 9.96 MB to 904 bytes. Genuine escaping closures kept their required
~6.29 MB. Numeric dynamic map/reduce/sort improved substantially; nonprimitive bridges
and ordinary paths retained their allocation counts. [Evidence](benchmarks/m27/environment.json).

Opt-in native kernels improved typical 500 B arithmetic 8–14%, dense numeric loops
33–65%, and a 1 MiB scanning control essentially 0%. Warm kernel installation costs
~0.25–0.39 ms; typical scalar amortization was ~8,000–13,000 records, large folds tens
of records. Successful kernels add no per-record allocation. Native code/module memory,
compilation allocations and host-specific page sizes are reported separately.
[Evidence](benchmarks/m26/environment.json). Keep native selection explicit.

## Public API and CLI hardening

M28 preserves borrowed/primitive value layouts and execution plans. Empty options delegate
to ordinary evaluation; external names constrain builtin resolution before optimization.
Host calls always remain effectful. Snapshot ownership and decoded escape allocation are
explicit caller choices. Configured controls allocate per-evaluation state and use tree
fallback for planned loops, trading throughput for cooperative checkpoints. They are not
an instruction counter or a hard process memory/time budget.

Initial node-by-node work checks slowed lexical closures ~5%; they were removed in favor
of call/traversal/retained-argument boundaries. Seven 300 ms samples, sequential in both
orders against committed M27, give these final median changes (native disabled):

| Existing workload | Bytes | M28 versus M27, two orders |
|---|---:|---:|
| Shallow path | 500 | -0.4% / -0.3% |
| Planned arithmetic | 100 / 500 | +0.1% to -2.0% / -0.5% to -1.2% |
| Three field call arguments | 500 | -1.0% / -1.4% |
| 8,192-key static lookup | 500 | +0.1% / +0.4% |
| String callback | 100 | -3.9% / -3.9% |
| Transient captures | 50,102 | -1.9% / -2.4% |

Allocation counts are unchanged in all 14 controls. Each optional lexical arena now
includes one eight-byte control pointer; dynamic bridges add sixteen requested bytes.
The residual call/callback costs are accepted for shared cooperative guards and clearer
diagnostics. These timings include code-generation differences; they do not establish
instruction-level attribution. No traversal, value layout or native coverage changed.

Final 500 B embedding fixtures include validation and consumption, without serialization:

| Operation | Records/s | Allocations | Requested bytes/record |
|---|---:|---:|---:|
| Direct arithmetic | 1,058,949 | 0 | 0 |
| Default options | 1,057,971 | 0 | 0 |
| External binding | 937,526 | 5 | 400 |
| Host call with signature | 864,433 | 5 | 400 |
| Configured limits | 964,504 | 5 | 392 |
| Borrowed string access | 3,022,458 | 0 | 0 |
| Decoded escaped string | 2,867,018 | 1 | 8 |
| Owned scalar snapshot | 2,977,729 | 0 | 0 |
| Constructed object snapshot | 728,725 | 13 | 930 |

Only direct/default-options/limits execute the identical arithmetic expression; binding,
host and ownership fixtures perform different work. These are scoped rates, not isolated
API overhead estimates. Host fixtures reuse their callable; binding fixtures include
the per-record options vector. Owned object results include construction plus detachment. Borrowed/primitive
access and scalar snapshots remain allocation-free; decoding and container ownership are
explicit boundaries. Host map/aggregate widths 8–1,024 and controlled filter/aggregate
fixtures up to 10 KiB are included in `embedding-final.csv`.

The complete default-feature sweep passed 1,788 groups / 12,516 samples, including allocation
assertions. [M28 evidence](benchmarks/m28/environment.json) records commands, source/binary
hashes, both timing orders and the final differential run. CLI serialization uses a reusable
bounded line buffer, adding one copy per result to prevent partial serialization output.
Engine benchmarks omit that CLI copy.
