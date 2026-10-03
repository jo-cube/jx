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

M30's warmed five-second samples describe the current engine:

| Workload | Input bytes | Scanner/traversal | Plan | Allocation/drop/clone |
|---|---:|---:|---:|---:|
| Planned scalar | 500 | 83% | 10% | 1% |
| Static 8,192-key lookup | 500 | 87% | — | <1% |
| Numeric map / reduce | 50,102 / 41,828 | 77% / 77% | within call helpers | 2% / 3% |
| Filter/map/fold | 41,828 | 69% | 20% | 3% |
| Repeated string projections | 50,102 | 33% | — | 15% |
| Pure ordering / grouping | 41,828 | 58% / 47% | 7% / — | 7% / 12% |
| Structural update | 33,180 | 36% | — | 20% |
| Dynamic object callback | 6,149 | 50% | within call helpers | 23% |
| Large sparse scalar | 1,048,576 | ~100% | <1% | <1% |

These are disjoint sampled self categories, with inlined helpers attributed to callers;
function/callback categories include tree/plan/runtime helpers, not dispatch alone.
String projection maps spend another ~50% in those helpers; short 100 B string maps
spend ~29% in allocation/drop/clone, 24% in scanning and 18% in call helpers. Lookup
fingerprinting/search is ~7%/~2%. Full traces and definitions are retained in
[M30 evidence](benchmarks/m30/environment.json). General dynamic calls still need the
scope bridge; existing pure fixed-object plans now export only their result.

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

In the M29 run, M28's string-callback regression remained: M27 comparisons gave -4.7% /-3.2%,
with 14 allocations unchanged (953 → 961 requested bytes). Five-second sampled profiles
put scanner helpers at 23–25%, allocation/drop/clone at 29–32%, and function boundaries
at 16–19%; frame/bridge self samples are below 2%. Reordering the plan/control checks
failed to improve it and was removed. Shared cooperative guards and nonprimitive bridges
remain structural costs; those sampled categories do not identify exact cycle overhead.
M30's fresh repeats below do not reproduce the historical delta consistently.

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

## Runtime consolidation

M30 removes scope copying from dynamic callbacks whose bodies already have pure bounded
plans. Primitive results still export directly; fixed objects use the existing result
retention code for borrowed program literals/constants. No new value/frame representation,
cache, demand metadata or lowering/JIT coverage is added. Controlled calls, effects,
lexical state and failed type/shape guards keep the existing bridge/fallback.

Matched M29/M30 engines use identical expanded fixtures, seven 300 ms samples and both
orders. Dynamic object map throughput improves 38–40% at 8 rows, 59–62% at 128,
64–65% at 1,024 and 65–66% at 16,384. At 1,024 rows allocations fall 14,426 → 7,258
and requested bytes 1,283,735 → 472,727. These fixtures include runtime compilation,
validation and consumption, without serialization.

Live-heap fixtures additionally retain a constant string member. At 16,384 rows,
allocations fall 327,790 → 229,486 and total requested bytes 23,206,160 → 16,783,632;
peak live heap remains ~3.80 MB because the same output must survive. Transient captures,
genuine escapes, reductions and partial maps retain their existing peaks; all 200 final
observations have zero after-drop delta. Requested heap is not RSS.

The historical callback deltas are not a reliable current optimization target. Fresh
M28/M29 partial comparisons give -0.3%/+0.8%; M27/M29 short string comparisons give
+1.8%/-1.0%. Source inspection finds no new executed M29 lambda-partial operations;
Math coercion is not invoked there. M28's real changes are optional call/plan checks,
diagnostics and an eight-byte arena pointer, with unchanged allocation counts. Removing
those nil guard checks did not improve throughput. Moving checkpoints into dispatch
arms gave marginal/inconsistent results and was removed. Historical instruction-cycle
attribution remains unresolved; layout/inlining and measurement variation cannot be
excluded. [Investigation](benchmarks/m30/regression-notes.md).

Final M30 controls retain every unrelated allocation count and zero-allocation path.
The three-item numeric partial is ~2% slower at 500 B (-1.9%/-2.1%), but wider partial
maps are steady (-0.5% to +2.1% across 8–16,384 rows); no additional operation runs there.
The 500 B string callback is ~1% slower, while 100 B changes sign and 10 KiB is steady.
These modest build-level costs do not justify a specialized dispatch patch. Scalar/path,
lookup, ordering/grouping, regex/date and transform controls are within roughly 2.4%.

Repeated unplanned string projections/conversions, non-lowered dynamic bridges, genuine
captures, effectful comparators, sequence replay and transform reconstruction remain.
The measured map/fold scanner cost is much larger than plan dispatch. Keep existing
capture/plans and optional native kernels; no current evidence supports universal
indexing/caching or another representation change.

Same-binary native controls (unchanged kernel coverage) improve 500 B arithmetic ~9%,
1 KiB arithmetic ~5%, object folds 54–59%, numeric folds 69–71%, and dense filter/map/fold
34–36% in both orders, all without per-record allocation. The 1 MiB scanning control is
unchanged. Scalar interpreter/native self samples shift plan helpers from ~11% to ~3%;
object-fold samples shift them from ~40% to ~14%, with ~9% in unsymbolized native code.
On the canonical 500 B arithmetic fixture, the interpreter reaches ~89% and native
~97% of the validating purpose-written Rust control; that control does not implement
JSONata's general shape/error rules. Traversal therefore occupies a larger fraction
after acceleration. These are current controls, not M30 JIT changes; native remains explicit and bounded.

The final default sweep covers 1,806 groups /12,642 samples with allocation assertions.
`just all` and `just build` pass; all 1,679 upstream classifications and 54,715 comparisons
across 25 differential suites remain green. Boundary properties ran 100,000 seeded
mutations; ASan/coverage-guided fuzzing completed 200,000 executions without a finding.
Normal/native tests passed on arm64 macOS and x86_64 macOS under Rosetta; Linux/Windows
CI remains configured but unexecuted. [M30 evidence](benchmarks/m30/environment.json)
records commands, binary/source hashes, fresh profiles and measurement limits.
