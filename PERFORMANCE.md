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

Array fixtures add 1, 8, 16, 128, 1,024 and 16,384 objects (78 B through 1.08 MiB),
measuring root/shallow/nested paths, array-result flattening, missing fields and
cancellation after the first item. Separate fixtures exercise nested contexts,
sparse mixed arrays, 64 nested arrays, empty/singleton results and nested array
values. Expected emission counts are checked before timing. Cancellation still
includes full-record validation and any lookahead needed to identify the first item.

Scalar fixtures add exact 100 B–1 MiB records for literals, arithmetic, nested
operands, comparisons, strings, boolean logic, missing, short-circuiting, mixed
expressions and reused output serialization. Additional workloads exercise singleton
and multi-item sequence operands, type errors, escaped strings, stream-to-array and
stream-to-stream equality, and objects with 8/128 fields. Expected results are checked
before timing. Path, scalar, filter and aggregate compilation have separate rows.

Filter fixtures add exact 100 B–1 MiB records for predicates, no matches, literal
first/last positions, computed negative positions and chained predicates. Wide arrays
(8/128/1,024 items), combined-sequence positions, nested groups, deep arrays and
16 chained computed negative indexes expose replay and length-counting costs.

Aggregate fixtures add exact 100 B–1 MiB records for count/sum/min/max, filtered
count/sum and no matches. Raw numeric arrays (8/128/1,024/16,384 members) distinguish
array folds from filtered sequences and computed mapped values. Nested inputs with
16/256/16,384 groups (up to 1,048,588 bytes) measure navigated and filtered folds;
64-deep arrays retain the scanner rescanning cost. Expected scalar results and
missing output are asserted before timing.

The process-wide counting allocator records allocation/reallocation calls and
requested bytes. Timed workloads are single-threaded; compilation allocates but
ordinary paths, filters, aggregates, scalar operators and preallocated output must report zero. Structural
object/sequence equality has explicit per-workload allocation budgets. All assertions
also run in `just all` via `--smoke`. The wrapper delegates to `System`;
it is the only unsafe code, isolated to the benchmark. Engine and CLI forbid unsafe.
Counter overhead affects allocating compilation; counts are not retained RSS.

## Extending evidence

Add constructors and further functions with their semantics. Use selected purpose-written
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

## Milestone 2 — array navigation and sequences

Same machine/compiler/profile as the initial baseline. Two paired process runs
compare committed milestone 1 (`a1ac257`) with milestone 2. All 98 repeated-execution
workloads (56 existing, 42 new) measured zero allocations; compilation is separate.
Selected array results below are medians of 14 samples across two runs. Validation
and complete result consumption are included; serialization and physical I/O are not.

| Workload | Input bytes | Median records/s | Median input MB/s |
| --- | ---: | ---: | ---: |
| array/shallow | 540 | 659,442 | 356.1 |
| array/shallow | 1,074 | 323,474 | 347.4 |
| array/nested | 1,074 | 291,119 | 312.7 |
| array/flatten | 1,074 | 304,624 | 327.2 |
| array/shallow | 69,558 | 5,403 | 375.8 |
| array/shallow | 1,135,782 | 333 | 378.2 |
| array/nested_contexts | 972 | 186,440 | 181.2 |
| array/deep | 136 | 80,048 | 10.9 |

For existing `customer.id` projections, paired median changes were:

| Fixture | Run 1 | Run 2 |
| --- | ---: | ---: |
| ascii/nested, 100 B | -4.82% | -4.43% |
| ascii/nested, 500 B | -1.56% | -2.17% |
| ascii/nested, 1,024 B | -0.89% | -1.37% |
| ascii/nested, 10,240 B | +0.52% | -0.17% |
| ascii/nested, 1,048,576 B | -0.05% | -0.32% |
| structured/nested, 485 B | +11.25% | +12.73% |
| unicode_escaped_keys/nested, 960 B | +0.06% | -2.16% |

The roughly 4–5% tiny-record regression repeats; it is not dismissed as noise.
The larger selected object workloads change less, while the structured fixture
improves. These measurements do not isolate the cause of code-generation/layout
changes. Keep the direct representation until profiling justifies a change.

Array traversal reuses validation scans for boundaries and may revisit bytes at
successive nesting/path levels. The deep-array fixture makes this cost visible;
these results establish a baseline for future profiling, not a full-engine speedup.

Raw paired runs: [M1 first](benchmarks/m2/m1-before-m2.csv),
[M2 first](benchmarks/m2/m2-baseline.csv), [M1 repeat](benchmarks/m2/m1-repeat.csv),
[M2 repeat](benchmarks/m2/m2-repeat.csv). [Environment, commands and source hashes](benchmarks/m2/environment.json)
make the comparison reproducible. Tail latency and retained RSS remain unmeasured.

## Milestone 3 — scalar expressions and operators

Same machine/compiler/profile. The suite has **169 workloads**: two compilation
cases and 167 repeated evaluations. Across two final runs, **163 evaluation workloads
allocated zero**. The four structural-equality workloads retain borrowed data:
16-item stream equality makes three allocation/reallocation calls (448 requested
bytes); 8/128-field object equality makes 3/7 calls (1,172/20,884 requested bytes);
the array of 16 one-field objects makes 16 calls (2,752 requested bytes). These
are total requested bytes, not retained memory. Scalar serialization reuses output.

Selected medians across 14 samples (validation and complete consumption included):

| Workload | Input bytes | Records/s | Input MB/s |
| --- | ---: | ---: | ---: |
| scalar/arithmetic | 500 | 1,072,254 | 536.1 |
| scalar/arithmetic | 1,024 | 576,620 | 590.5 |
| scalar/strings | 500 | 1,605,014 | 802.5 |
| scalar/mixed | 500 | 630,036 | 315.0 |
| scalar/sequence_array_equality | 330 | 443,724 | 146.4 |
| scalar/sequence_equality | 330 | 365,062 | 120.5 |
| scalar/object_equality | 125 | 834,530 | 104.3 |
| scalar/object_equality | 2,389 | 50,192 | 119.9 |

Final M3 versus the two fresh M2 (`abaf664`) baselines:

| Existing workload | Bytes | Run 1 change | Run 2 change |
| --- | ---: | ---: | ---: |
| ascii/nested | 100 | -0.89% | +0.14% |
| ascii/nested | 500 | -5.54% | -1.74% |
| ascii/nested | 1,024 | -2.29% | -1.39% |
| ascii/nested | 10,240 | -1.68% | -0.87% |
| ascii/nested | 1,048,576 | -0.16% | -0.86% |
| array/shallow | 1,074 | +0.51% | -0.24% |
| array/deep | 136 | -1.60% | +0.13% |

The first scalar implementation added about 4–5% to the tiny nested-path cost.
Symbol inspection showed an outlined `Expression::evaluate` where M2 had inlined
it. An inline hint alone did not help. Keeping scalar preparation in `evaluate.rs`
and reducing the public method to a small inline dispatch recovered that additional
tiny-record cost. The older M1→M2 gap remains; 500 B–1 KiB projections still show
small losses with run-to-run variation. The table records them rather than claiming
all regressions are resolved. Deep-array rescanning is unchanged (about 80k records/s
for 136 bytes); no trusted skipper or index was introduced.

Initial object equality rescanned each object per key. On the 128-field fixture,
the borrowed-member map moves throughput from about 1.2k to 50k records/s, trading
zero allocation for seven allocator calls. This is scoped to structural equality;
ordinary scalar/path operations keep their allocation contract. Scalar path operands
still scan separately after validation, so multi-field expressions revisit unused
payloads. Capture fusion remains a measured future opportunity, not current machinery.

[Raw runs and source/environment metadata](benchmarks/m3/environment.json) include
the initial implementation, the unsuccessful inline-only experiment and final repeats.
To replay the equality or dispatch alternatives, use the accompanying patches in a
separate copy of the final source and run `just bench`. The metadata states the
variant scope; these patches are benchmark artifacts, never built into the engine.
No cross-engine speedup or latency-distribution claim follows from these results.

## Milestone 4 — filters

Same machine/compiler/profile. **220 workloads** now include three compilation
cases and 217 repeated evaluations. Both final runs report zero allocations for
**213 evaluation workloads**, including all **50 filter workloads**. The four
structural-equality allocation counts are unchanged; 16-item stream equality now
requests 672 bytes (previously 448), retaining borrowed `Value` items so primitive
and undefined sequence items can participate too. No filter collects its candidates.

Selected medians across 14 samples, including validation and full consumption:

| Workload | Input bytes | Records/s | Input MB/s |
| --- | ---: | ---: | ---: |
| filter/predicate | 100 | 2,343,252 | 234.3 |
| filter/predicate | 500 | 1,180,120 | 590.1 |
| filter/predicate | 1,024 | 722,773 | 740.1 |
| filter/predicate | 1,048,576 | 944 | 989.3 |
| filter/wide_predicate | 2,736 | 44,706 | 122.3 |
| filter/sequence_last | 23,392 | 5,865 | 137.2 |
| filter/nested | 892 | 89,826 | 80.1 |
| filter/negative_chain | 20 | 100,152 | 2.0 |

Final M4 versus two fresh M3 (`533e90b`) runs:

| Existing workload | Bytes | Run 1 change | Run 2 change |
| --- | ---: | ---: | ---: |
| ascii/nested | 100 | -2.82% | -0.92% |
| ascii/nested | 500 | -3.12% | -2.41% |
| ascii/nested | 1,024 | -0.97% | -1.10% |
| array/shallow | 1,074 | +0.28% | +1.22% |
| array/deep | 136 | -0.29% | -0.21% |
| scalar/strings | 100 | -5.41% | -6.07% |
| scalar/sequence_equality | 330 | -4.31% | -4.98% |

The first filter implementation redundantly nested the stream enum inside the public
result enum, which already had a direct path variant. Storing expression/context
directly removes that duplication. The paired tiny nested-path improvements are
2.18% and 4.04% versus that variant; there is no consistent improvement at
larger sizes. The final path dispatch remains inlined. Remaining tiny projection
and scalar costs are recorded above, including the earlier M1→M2 regression.
Deep-array rescanning remains around 80k records/s; no scanner index or trusted
skipper was added.

Negative positions count and replay their input without collecting it. Scoped stage
lengths keep chained computed negative positions from recursively recounting the
same input. Filtered scalar operands and grouped expressions can replay to normalize
cardinality and preserve error behavior. These costs remain explicit workloads,
not a promise that every expression makes one pass.

[Raw runs, commands and source hashes](benchmarks/m4/environment.json) preserve two
M3 baselines, two nested-result runs and two final runs. The accompanying
[nested-result patch](benchmarks/m4/nested-result.patch) replays the layout comparison
in a separate copy. No cross-engine speedup, tail-latency or RSS claim is made.

## Milestone 5 — streaming aggregates

Same machine/compiler/profile. **302 workloads** include four compilation cases and
298 repeated evaluations. Both final runs report zero allocations for **294 evaluation
workloads**, including all **81 aggregate workloads**. The four structural-equality
allocation budgets are unchanged. Aggregates retain one pending value and primitive
state; neither raw-array nor filtered-sequence folds collect their arguments.

Selected medians across 14 samples, including validation and complete consumption:

| Workload | Input bytes | Records/s | Input MB/s |
| --- | ---: | ---: | ---: |
| aggregate/sum | 500 | 1,521,966 | 761.0 |
| aggregate/filtered_sum | 100 | 2,645,090 | 264.5 |
| aggregate/filtered_sum | 500 | 1,238,773 | 619.4 |
| aggregate/filtered_sum | 1,024 | 750,898 | 768.9 |
| aggregate/filtered_sum | 1,048,576 | 943 | 988.8 |
| aggregate/array_sum | 87,201 | 2,986 | 260.3 |
| aggregate/nested_filtered_sum | 1,036 | 83,654 | 86.7 |
| aggregate/nested_filtered_sum | 1,048,588 | 84 | 87.6 |
| aggregate/deep_filtered_sum | 136 | 79,186 | 10.8 |

Final M5 versus two fresh M4 (`c87ae05`) runs:

| Existing workload | Bytes | Run 1 change | Run 2 change |
| --- | ---: | ---: | ---: |
| ascii/nested | 100 | +1.64% | -1.68% |
| ascii/nested | 500 | +0.39% | -0.64% |
| ascii/nested | 1,024 | -0.12% | -1.65% |
| array/shallow | 1,074 | -1.09% | -0.82% |
| array/deep | 136 | +1.63% | -0.70% |
| scalar/arithmetic | 100 | +0.97% | -1.02% |
| filter/predicate | 100 | -0.73% | -1.46% |
| filter/chained | 100 | -2.87% | +1.59% |
| filter/nested | 892 | -0.00% | +0.12% |

The initial shared path helper was outlined. A narrow inline hint removes that call;
versus the two outlined runs it improves the 100 B predicate fixture by 2.41%/1.39%
and chained filters by 2.41%/4.93%. No scanner, cache or new execution framework was
added. The table retains residual variation and losses; it does not claim to resolve
the earlier M1→M2 tiny-record regression. Deep-array rescanning remains near 80k
records/s. Large nested aggregate inputs expose the same traversal cost.

The fold consumes the existing argument stream once without a cardinality preflight.
Source stages can still perform lookahead, group normalization or negative-index
replays; validation and path selection also revisit bytes. This is not a claim of
one scan of every byte or constant time per record.

[Raw runs, environment, commands and source hashes](benchmarks/m5/environment.json)
retain two M4 baselines, two outlined-helper runs and two final runs. The
[outlined-helper patch](benchmarks/m5/outlined-path.patch) reproduces the narrow
comparison in a separate copy. No cross-engine speedup, tail latency or RSS claim
is made.
