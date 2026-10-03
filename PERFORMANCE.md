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

## Residual compatibility and robustness

M29 broadens Unicode regex/date pictures, numeric builtin partial coercion and postfix
composition after tuple sorting. Static patterns/pictures still compile once. Known
legacy case-fold mismatches remain explicit; date pictures additionally check supplementary
literals exactly. The value, context, frame, scanner and plan layouts/paths are unchanged.
Only tuple rows add a local flag (32 → 40 bytes on arm64); existing ordered/grouped controls
retain allocation counts, with requested bytes rising 1.9% /3.1% respectively.

Seven 300 ms samples in both orders against M28 retain these median throughput changes:

| Existing control | Bytes | M29 versus M28, two orders |
|---|---:|---:|
| Shallow path | 500 | -1.7% /-0.3% |
| Planned arithmetic | 100 | -0.4% /+0.7% |
| Planned arithmetic | 500 | +0.7% /-0.6% |
| Three field arguments | 500 | -0.2% /0.0% |
| 8,192-key lookup | 500 | -0.5% /-0.2% |
| String callback | 100 | -1.0% /-0.3% |
| Numeric partial callback | 500 | -3.5% /-3.2% |
| Regex callback replacement | 500 | +0.9% /+1.3% |
| Static date parsing | 500 | +0.9% /+1.7% |

All 20 controls retain allocation counts and zero-allocation paths remain zero-allocation.
The partial callback control uses a lambda, not the new Math coercion helper; its modest
regression has no new frame/value/plan operations or allocations. Binary layout/inlining
changes remain possible; no instruction-level attribution is established. It is documented
rather than addressed with a speculative dispatch patch.

M28's string-callback regression remains: repeated M27 comparisons give -4.7% /-3.2%,
with 14 allocations unchanged (953 → 961 requested bytes). Five-second sampled profiles
put scanner helpers at 23–25%, allocation/drop/clone at 29–32%, and function boundaries
at 16–19%; frame/bridge self samples are below 2%. Reordering the plan/control checks
failed to improve it and was removed. Shared cooperative guards and nonprimitive bridges
remain the accepted cost; these sampled categories do not identify exact cycle overhead.

For the new post-sort workloads, borrowing the fixed `@` key removes avoidable owned-string
construction. Compared with M29 before that key change, grouping throughput improves
15–19% at widths 8/128/1,024 in both orders. Filter throughput varies -2.1% to +1.2%, so
no filter speedup is claimed. At width 1,024, allocations drop 10,259 → 7,187 for filtering
and 22,584 → 14,904 for grouping; grouping requests 976,152 → 891,672 bytes. Named binding
keys and required tuple objects still allocate. The final Unicode-fold guards are also
included in this comparison: 500 B Unicode date parsing varies -3.4% /-1.3% against the
earlier M29 implementation, with identical allocations; 10 KiB controls are within 0.3%.
The broader guard is retained for correctness.

Final new 500 B workloads, including validation/consumption but not serialization:

| Workload | Records/s | Allocations | Requested bytes/record |
|---|---:|---:|---:|
| Unicode case-insensitive contains | 1,212,873 | 7 | 240 |
| Unicode replacement | 922,386 | 16 | 442 |
| Static Unicode date picture | 1,081,016 | 6 | 1,400 |
| Numeric builtin partial | 1,155,872 | 8 | 736 |

The complete final sweep checks 1,802 groups /12,614 samples and allocation assertions.
All 1,679 upstream classifications and 54,685 differential comparisons remain green.
Seeded boundary properties run in ordinary tests; optional ASan/coverage-guided fuzzing
completed 500,000 executions without a failure. Normal/native tests ran on arm64 macOS
and x86_64 macOS under Rosetta. Linux/Windows CI is configured, not yet remotely executed.
Fuzz dependencies live in a separate development-only workspace; engine dependencies,
unsafe boundary and native coverage are unchanged. [M29 evidence](benchmarks/m29/environment.json)
records exact commands, hashes, profiles and platform limitations.
