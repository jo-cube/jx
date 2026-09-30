# Performance

Optimize repeated single-threaded execution over independent records. Compilation
cost is separate. No speedup claim against another engine is established yet.

## Reproduce

```sh
just all
just bench > /tmp/jx-bench.csv
# Profile one existing workload with longer samples:
JX_BENCH_FILTER=filter/predicate JX_BENCH_BYTES=100 JX_BENCH_SAMPLE_MS=1000 just bench
```

The optional controls select a workload-name substring and exact input size, and
set each sample's minimum duration. Defaults remain seven 50 ms samples. `--smoke`
ignores selection controls so allocation checks cannot silently disappear.

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
before timing. Path, scalar, filter, aggregate, constructor, lexical and navigation compilation have separate rows.

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

Constructor fixtures add computed objects, borrowed whole-record members, nested
containers, navigation back through constructed output and filtered summaries at
100 B–1 MiB. Wide inputs (8/128/1,024/16,384 rows) compare streamed mapped objects,
collection into an array, reused serialization and aggregate-only summaries. Nested
inputs reach 1,048,588 bytes. Expected semantic JSON output is checked before timing.

Lexical fixtures cover lookup, repeated bindings, retained sequences, ordinary and
escaping closure calls, and filter/aggregate/constructor pipelines at 100 B–1 MiB.
Wide fixtures compare mapped calls with repeated use of one retained projection;
nested retention reaches 1,048,588 bytes. Allocation budgets are asserted separately
from existing zero-allocation paths, scalars, filters and direct aggregates.

Navigation fixtures cover singleton retention, wildcards/descendants, ordering,
grouping, membership, fallbacks and mixed lexical pipelines at 100 B–1 MiB. Wide
inputs reach 16,384 rows; distinct-key grouping and 8/32/64-deep descendants expose
key-search and subtree-scanning costs. Range output is constructed and consumed.
Compilation separately measures navigation and nested fallbacks; the latter has an
allocation budget to prevent exponential tree expansion.

Compiler fixtures add static tables with 8/128/1,024/4,096 keys: last-key hits,
misses, a rotating pool of 256 keys, and retained lexical tables. Record-size cases
span 100 B–1 MiB. Constant-heavy scalars, repeated field demands, static constructors
with dynamic leaves, invariant aggregates, builtin references and filtered sums
separate folding gains from unchanged traversal costs. Compilation is measured
separately for each table size. Rust controls combine the same validating key selector
with `HashMap`; repeated-field controls validate the whole record and parse two numeric
fields once using the fixture's fixed ASCII layout. They are architectural controls,
not general JSONata implementations. Evaluation timing excludes serialization.

Execution fixtures compare arithmetic-heavy expressions, comparisons, conditional
branches and static constructors with computed members at 100 B–1 MiB. Streamed
numeric predicates and mapped sums cover 8–16,384 rows; a separate static lookup
contains 8,192 keys. A type-error fixture measures plan fallback. Rust controls
validate the same records, read fixture-specific numeric fields once and compute the
same scalar or filtered sum. Constructor controls return the static schema plus its
computed member; neither side serializes during timing. They do not implement general
JSONata navigation or error behavior. Numeric compilation has its own row.

Region-plan fixtures add boolean/conditional control flow, fixed objects with shared
numeric demands, 4,096-key quote calculations, and nested-path invoice folds with
optional output construction. Records span 100 B–1 MiB and 1–16,384 rows. Wide objects
and untaken branches measure sparse demands and capture overhead. The invoice Rust
control uses validating path capture plus a reader for the fixed ASCII row layout;
it checks the same numeric total, without general JSONata shape/error handling.

Builtin fixtures cover string normalization, literal split/join, numeric helpers,
streaming average, type/keys, native and closure callbacks, reduction and sifting at
100 B–1 MiB. Mixed 8/128/1,024-row inputs add filter/reduce, planned average, grouping,
ordered labels, merging constructed objects and distinct projections.

The process-wide counting allocator records allocation/reallocation calls and
requested bytes. Timed workloads are single-threaded; compilation allocates but
ordinary paths, filters, aggregates and scalar operators without lexical features
or construction must report zero, including preallocated output. Structural
object/sequence equality and constructors have explicit per-workload allocation budgets. All assertions
also run in `just all` via `--smoke`. The wrapper delegates to `System`;
it is the only unsafe code, isolated to the benchmark. Engine and CLI forbid unsafe.
Counter overhead affects allocating workloads; counts are not retained RSS.

## Extending evidence

Extend workloads alongside further functions and tuple navigation. Use selected purpose-written
Rust controls when they clarify overhead. Add realistic whole-CLI pipelines and
latency distributions separately; sample duration is not per-record tail latency.
Fair cross-engine comparisons must use identical input, expression semantics,
validation, result cardinality and serialization work, with pinned versions,
commands, machine/compiler details and repeated raw results.

Profile before adding scan fusion beyond the current path, SIMD, indexes, caches,
a broader execution IR or JIT. Allocation is permitted when semantics need construction,
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

## Milestone 6 — constructors

Same machine/compiler/profile. **358 workloads** include five compilation cases
and 353 repeated evaluations. Both final runs retain **294 zero-allocation evaluation
workloads**. All **55 constructor workloads** allocate their requested structure;
ordinary paths, filters and aggregate folds still allocate zero. The four existing
structural-equality allocation counts are unchanged (their retained-value byte
footprint can differ).

Selected medians over 14 samples, including validation and complete consumption:

| Workload | Input bytes | Records/s | Input MB/s | Allocations/record | Requested bytes/record |
| --- | ---: | ---: | ---: | ---: | ---: |
| construct/raw_member | 500 | 2,642,583 | 1,321.3 | 3 | 168 |
| construct/raw_member | 1,048,576 | 1,854 | 1,944.1 | 3 | 168 |
| construct/filtered_summary | 500 | 562,452 | 281.2 | 5 | 368 |
| construct/nested | 500 | 443,525 | 221.8 | 15 | 1,352 |
| construct/mapped_objects | 4,334 | 28,336 | 122.8 | 192 | 18,944 |
| construct/collected_objects | 4,334 | 28,237 | 122.4 | 199 | 21,992 |
| construct/filtered_aggregate | 624,958 | 180 | 112.2 | 3 | 296 |
| construct/nested_summary | 1,048,588 | 38 | 39.8 | 3 | 296 |

`{"record":$}` requests 168 bytes in three allocations at every measured size,
including 1 MiB: the input remains borrowed. The 128-row mapped-object fixture
constructs 64 results with three allocations each; collecting them into an array
adds storage for retained values. Streamed callbacks can release each object before
the next one. Allocation totals include growth and temporary grouping storage;
they are not peak live memory. Counter overhead is included in allocating timings.

Final M6 versus two clean M5 (`ab912aa`) runs:

| Existing workload | Bytes | Run 1 change | Run 2 change |
| --- | ---: | ---: | ---: |
| ascii/nested | 100 | -2.77% | +3.32% |
| ascii/nested | 500 | -4.26% | -0.76% |
| ascii/nested | 1,024 | -3.30% | -0.04% |
| array/shallow | 1,074 | -4.53% | -4.80% |
| array/deep | 136 | -4.52% | -4.52% |
| scalar/arithmetic | 100 | -3.85% | -5.65% |
| filter/predicate | 100 | -12.91% | -12.52% |
| filter/predicate | 500 | -6.68% | -6.77% |
| filter/chained | 100 | -15.81% | -17.18% |
| filter/nested | 892 | -18.44% | -16.79% |
| aggregate/sum | 500 | -3.21% | -2.44% |
| aggregate/nested_filtered_sum | 1,036 | -19.14% | -18.98% |

The common filter and nested-aggregate regressions are material and remain open.
Containers made values retainable and introduced ownership/shape handling into the
shared evaluator. Inspection found extra owned context/stage state; evaluation now
borrows contexts and scoped views borrow operands. The retained owned-view comparison
shows no meaningful filter throughput recovery from the latter simplification, so
it is retained for clear ownership rather than claimed as a speed optimization.
These measurements do not isolate how much cost comes from tags, drops, code layout
or scanning; no additional specialization, cache, scanner index or IR was added.

Tiny projection variation remains visible, including the older M1→M2 regression.
Deep-array traversal still revisits nested bytes, now around 76k records/s on its
136-byte fixture. Large nested filtered summaries expose repeated traversal of the
same demand for each member. Dynamic object-key grouping uses a linear search;
wide grouping can require quadratic key comparisons and is not covered by a speed
claim. These are explicit targets for later profiling, not resolved costs.

[Raw runs, commands, environment and source hashes](benchmarks/m6/environment.json)
retain two M5 baselines, the owned-view prototype and two final runs. The accompanying
[prototype patch](benchmarks/m6/owned-views.patch) replays that benchmark in a separate
copy; it predates the grouped-array focus fix and is not a replacement implementation.
No cross-engine speedup, latency-distribution or retained-RSS claim follows.

## Milestone 7 — architecture and performance consolidation

Scalar evaluation now consumes known missing/single-value path selections directly.
Only unresolved navigation and retained sequences need stream cardinality discovery.
Routes, predicates and recursive lookup borrow live contexts; boolean filters move
accepted candidates instead of cloning them. There is no new language feature, value
hierarchy, scanner, cache, IR or JIT. All 21,640 differential comparisons still pass.

The M6 regression is not explained by larger runtime unions or per-record heap
allocation. On this compiler, M5 and M6 both have 24-byte `Value`, 48-byte
`PathEvaluation`/`Stream`/`Operand`, 32-byte `Context`, and 48-byte `Node`.
Constructors change the value-bearing types from `needs_drop=false` to `true` and
add raw/constructed dispatch. Non-constructing filter/aggregate workloads still allocate zero; `needs_drop` describes
type-level cleanup, not a heap allocation or reference-count update for every raw value.

Five-second warmed `sample` profiles locate substantial scanning cost and additional
stream/callback/drop work around scalar operands. Removing that round trip reduces
this work without changing shape rules or traversal passes. An exploratory shortcut
inside `Stream::operand` gave uneven results and did not improve nested filtering;
returning selections at the scalar boundary avoids constructing the stream itself.
Scoped borrowing alone gives little recovery on the principal regressions.

Deep arrays remain dominated by scanner work (over 90% of leaf samples in
`Scanner::value`); subtree rescanning is unchanged. Constructors still evaluate member
demands separately. The remaining costs do not justify an IR: it would not eliminate
these traversals or ownership checks by itself. Sampling does not apportion every
remaining loss between tag checks, drops and compiler code layout.

Two sequential baseline/final pairs use the same harness, machine, compiler and
release profile as M6. **358 workloads**, **294 zero-allocation evaluations**;
allocation counts and requested bytes are identical across all four runs, including
constructors and equality. Selected medians over 14 samples, with paired changes
relative to fresh M6 (`d6c4594`) runs:

| Workload | Input bytes | M7 records/s | Pair 1 change | Pair 2 change |
| --- | ---: | ---: | ---: | ---: |
| ascii/nested | 100 | 10,045,212 | -1.21% | +1.02% |
| ascii/nested | 500 | 3,160,786 | -3.16% | -0.19% |
| array/shallow | 1,074 | 310,305 | -0.09% | +0.34% |
| array/deep | 136 | 76,845 | +0.09% | +0.62% |
| scalar/arithmetic | 100 | 3,183,503 | +4.32% | +3.98% |
| filter/predicate | 100 | 2,209,792 | +7.83% | +8.52% |
| filter/predicate | 500 | 1,150,544 | +4.17% | +4.59% |
| filter/chained | 100 | 1,668,061 | +11.20% | +9.52% |
| filter/nested | 892 | 76,490 | +3.76% | +3.55% |
| aggregate/sum | 100 | 3,774,431 | -3.23% | -2.68% |
| aggregate/filtered_sum | 500 | 1,222,353 | +3.56% | +4.02% |
| aggregate/nested_filtered_sum | 1,036 | 75,308 | +8.82% | +8.12% |
| aggregate/nested_filtered_sum | 1,048,588 | 75 | +7.91% | +8.31% |
| construct/raw_member | 1,048,576 | 1,854 | -0.51% | -0.19% |
| construct/filtered_summary | 500 | 596,999 | +3.60% | +4.07% |
| construct/filtered_aggregate | 624,958 | 210 | +4.73% | +9.93% |
| construct/nested_summary | 1,048,588 | 41 | +8.57% | +8.81% |

This is partial recovery of the M6 regressions, not a return to M5 throughput in
every case. Tiny projections and deep rescanning remain unchanged within these
runs' variation. A few small workloads regress repeatedly: plain sum at 100 B by
2.7–3.2%, raw-array sum at 23 B by 2.4–2.7%, and per-group positions by 2.2–2.8%.
No claim attributes those residual changes to one instruction or allocation; no
extra specialization was added for those marginal differences. The retained change
simplifies scoped ownership and materially improves the targeted composed workloads.

[All comparisons](benchmarks/m7/comparison.csv), [profile extracts](benchmarks/m7/profile-extracts.txt),
[layouts](benchmarks/m7/layouts.txt), and [commands/source hashes/raw runs](benchmarks/m7/environment.json)
include the negative results and exploratory variants. Rates are derived from
records/elapsed time before taking medians, avoiding the display column's rounding
on large inputs. Timings exclude profiler runs. Host activity was not controlled;
no cross-engine, tail-latency or retained-memory claim is made.


## Milestone 8 — lexical/function runtime

Bindings, blocks, closures and dynamic calls share the existing values and evaluator.
The compiler resolves unshadowed builtins and marks stages that cannot safely replay.
Ordinary expressions create no lexical arena; direct aggregates retain their streaming
folds. All **294 existing zero-allocation evaluations** remain zero-allocation. Allocation
counts and requested bytes for all earlier evaluation workloads are unchanged across
the retained baseline/final runs. The suite now has **406 workloads**, including six
compilation workloads and 47 lexical evaluations.

Bindings and call arguments store evaluated values, never replay recipes. Retained
sequences share their member storage on reuse. Investigation of the new wide workloads
removed repeated retention of already retained values and a redundant final-stage
buffer; intermediate stateful stages still need retention to prevent replay. Mapped
calls allocate argument/parameter storage per invocation. Captured frames remain live
until that record's evaluation ends; uncaptured terminal frames are reclaimed on return.
Lookup is linear through small frame binding lists. These are measured baseline costs,
not a slot compiler, frame cache or garbage collector hidden in the runtime.

On the pinned compiler, `Value` stays 24 bytes and `PathEvaluation` stays 48 bytes.
`Context` grows from 32 to 48 bytes; `Stream`/`Operand` from 48 to 56 bytes; `Node` from
48 to 64 bytes. Optional scope propagation and state checks add real execution work,
even where no arena is created. Direct tree evaluation remains appropriate; introducing
an IR would not itself remove binding retention or the existing repeated JSON scanning.

Two fresh M7 baselines (`5c97ba5`) and two final M8 runs use the same machine,
compiler, harness and release settings. Selected medians over 14 final samples:

| Workload | Input bytes | M8 records/s | Pair 1 change | Pair 2 change |
| --- | ---: | ---: | ---: | ---: |
| ascii/nested | 100 | 10,093,946 | -0.62% | +0.54% |
| ascii/nested | 500 | 3,194,592 | -2.13% | -1.85% |
| array/deep | 136 | 76,383 | -1.21% | -0.61% |
| scalar/arithmetic | 100 | 3,118,384 | -2.62% | -3.14% |
| filter/predicate | 100 | 2,029,347 | -6.39% | -8.81% |
| filter/chained | 100 | 1,598,954 | -0.45% | -4.03% |
| filter/nested | 892 | 76,988 | -1.65% | -3.07% |
| aggregate/computed_sum | 23 | 1,803,427 | -14.37% | -15.11% |
| aggregate/filtered_sum | 500 | 1,158,527 | -6.11% | -6.50% |
| aggregate/nested_filtered_sum | 1,036 | 70,619 | -6.04% | -5.48% |
| construct/filtered_summary | 500 | 586,918 | -2.69% | -1.81% |

There are real residual regressions: roughly 6–9% for selected filters/folds and
14–15% for the tiny computed sum. No additional allocation explains them. Larger
context/operand layouts and scope checks are relevant structural costs; these runs
do not isolate their share from dispatch, compiler code layout or host variation.
An experiment extracting lexical arms from scalar dispatch improved the tiny sum
by only 3.5–4.5%, with inconsistent chained-filter results. The extra dispatch was
not retained. Tiny-workload timings varied across builds and sampling setups;
there is no claim that this experiment explains or fixes the regression.

New lexical costs include retained values and per-call frames/arguments. Requested
bytes below are cumulative allocations, not peak live memory:

| Workload | Input bytes | Records/s | Allocations/record | Requested bytes/record |
| --- | ---: | ---: | ---: | ---: |
| lexical/lookup | 500 | 1,338,506 | 5 | 504 |
| lexical/repeated_binding | 500 | 1,209,696 | 5 | 504 |
| lexical/retained_sequence | 500 | 1,056,727 | 7 | 648 |
| lexical/closure_call | 500 | 802,739 | 8 | 768 |
| lexical/escaped_closure | 500 | 756,975 | 11 | 1,032 |
| lexical/mixed | 500 | 523,915 | 14 | 1,480 |
| lexical/mapped_calls | 251,044 | 344 | 32,788 | 3,801,624 |
| lexical/repeated_projection | 251,044 | 763 | 19 | 786,888 |
| lexical/nested_retention | 1,048,588 | 70 | 20 | 1,573,320 |

Compilation includes scope/effect analysis and is measured separately. Existing
subtree rescanning and member-wise constructor demands remain unchanged. No IR,
frame cache, slot compiler or scanner specialization was introduced.

[Comparisons](benchmarks/m8/comparison.csv), [layouts](benchmarks/m8/layouts.txt),
[discarded dispatch experiment](benchmarks/m8/dispatch-split.json) and
[raw runs, commands and source hashes](benchmarks/m8/environment.json) retain the
evidence. Benchmark processes ran sequentially; host activity was not controlled.
No cross-engine, tail-latency or retained-RSS claim follows.


## Milestone 9 — path and sequence expansion

The suite now has **478 workloads**: eight compilation cases and 470 evaluations.
Both final runs report **300 zero-allocation evaluations**; allocation counts and
requested bytes for every earlier evaluation workload are unchanged. `Value`,
`Context`, `Stream`/`Operand` and `Node` retain their M8 layouts. Four explicit array
shapes replace independent flags, and mapped traversal now lives in `route.rs`.
No lexical arena is introduced for ordinary expressions.

Five-second warmed M8 profiles of a 100 B predicate, 23 B computed sum and 1,036 B
nested filtered sum show substantial scanner work, plus scalar dispatch, callbacks
and drops. They reveal no variable lookup or frame creation on these paths. The
larger contexts/operands introduced in M8 remain shared costs; the profiles do not
isolate their contribution from code layout and dispatch. No clean frame-removal
change was identified, and splitting the evaluator would duplicate semantics.
Direct tree evaluation remains appropriate; no IR, cache or scanner index was added.

Two baseline/final pairs use M8 `7126491` and the same machine/compiler/release
settings as above. Medians below combine 14 final samples; paired changes compare
the corresponding seven-sample medians, derived from records/elapsed time.

| Existing workload | Input bytes | M9 records/s | Pair 1 change | Pair 2 change |
| --- | ---: | ---: | ---: | ---: |
| ascii/nested | 100 | 9,818,811 | -1.63% | -1.41% |
| ascii/nested | 500 | 3,160,373 | -2.45% | +0.21% |
| array/shallow | 1,074 | 310,852 | +0.70% | +0.61% |
| array/deep | 136 | 76,433 | +0.66% | +0.29% |
| scalar/arithmetic | 100 | 3,119,386 | +0.14% | -1.88% |
| filter/predicate | 100 | 2,101,816 | +4.80% | -1.23% |
| filter/chained | 100 | 1,593,370 | +2.55% | +0.72% |
| filter/nested | 892 | 76,762 | -1.05% | +8.61% |
| aggregate/computed_sum | 23 | 1,908,943 | +6.99% | -0.98% |
| aggregate/filtered_sum | 500 | 1,181,098 | +2.76% | -1.34% |
| aggregate/nested_filtered_sum | 1,036 | 72,146 | +2.55% | -0.27% |
| aggregate/nested_filtered_sum | 1,048,588 | 72 | +2.64% | -0.29% |
| construct/filtered_summary | 500 | 578,300 | -0.75% | -0.36% |
| lexical/mixed | 500 | 521,819 | -2.03% | -2.19% |

Baseline variation exceeds final-run variation on several targeted workloads;
first-pair gains do not establish a recovery of the M8 regressions. No earlier
workload loses more than 5% in both pairs. The small repeated projection and lexical
losses remain recorded, as do the older tiny-record and nested-rescanning costs.

One traversal change has a clear isolated benefit. Infallible wildcard/descendant
cardinality lookahead stops at two items instead of enumerating the whole result
before replay. Two focused pairs on the final source improve wide descendant sums
by **44–47%** across 8–16,384 rows. At 600,386 bytes, the two pairs move about 98 to
144 records/s; allocations fall from 65,540 to 32,772, requested bytes from
8,782,360 to 4,391,448. This removes a redundant traversal, not the member table
needed for decoded duplicate keys and reference key ordering.

Selected new workloads, including validation and complete consumption:

| Workload | Input bytes | Records/s | Input MB/s | Allocations/record | Requested bytes/record |
| --- | ---: | ---: | ---: | ---: | ---: |
| navigation/keep | 500 | 1,039,155 | 519.6 | 2 | 144 |
| navigation/descendants | 500 | 476,104 | 238.1 | 8 | 1,072 |
| navigation/ordering | 500 | 964,518 | 482.3 | 6 | 320 |
| navigation/grouping | 500 | 849,489 | 424.7 | 11 | 824 |
| navigation/mixed | 500 | 755,101 | 377.6 | 11 | 968 |
| navigation/wide_sort | 600,386 | 64 | 38.4 | 18 | 1,441,840 |
| navigation/wide_group | 600,386 | 288 | 172.8 | 32 | 1,180,088 |
| navigation/distinct_groups | 27,486 | 242 | 6.7 | 13 | 294,712 |
| navigation/range | 600,386 | 575 | 345.3 | 15 | 786,408 |
| navigation/deep_descendants | 136 | 38,590 | 5.2 | 4 | 536 |

Sorting retains candidates and stable merge indices; it evaluates keys on each
comparison to preserve lexical effects and error order. Grouping retains grouped
contexts and still searches keys linearly: the 1,024-distinct-key fixture exposes
quadratic comparisons. Wildcards with array-valued members and explicit `[]` retain
the required result shape. Range constructors materialize their explicit array,
even when immediately aggregated. Leaves remain borrowed. Deep descendant paths
still normalize/replay and rescan nested bytes; the 64-deep fixture makes this cost
visible. Allocation bytes are cumulative requests, not peak live memory.

A compiler regression test also caught exponential subtree duplication from lowering
fallbacks to copied conditionals. Keeping the left expression once and evaluating it
again when selected reduces ten nested coalescing expressions from **11,286 to 113
allocations** per compile (450,459 to 7,921 requested bytes). The benchmark enforces a
bounded allocation budget; runtime rebinding and repeated-evaluation semantics remain
covered by differential tests.

[All comparisons](benchmarks/m9/comparison.csv), [profile extracts](benchmarks/m9/profile-extracts.txt),
[layouts](benchmarks/m9/layouts.txt), and [raw runs, commands, patches and hashes](benchmarks/m9/environment.json)
preserve the evidence. Focused traversal comparisons use seven 100 ms samples per
workload per run. Profiler runs are excluded from timing comparisons. Processes ran
sequentially; host activity was not controlled. No cross-engine, tail-latency or
retained-RSS claim follows.


## Milestone 10 — compiler specialization

Same machine/compiler/release settings. **560 workloads** include 12 compilation
cases and 548 evaluations; **359 evaluations allocate zero** in both final runs.
Existing allocation counts are unchanged except six membership fixtures, which fall
from four to two calls. Constant containers allocate one 16-byte identity token;
primitive static lookups allocate nothing. Counts are cumulative requests, not RSS.

Two full M9 (`4edc571`) runs bracket development of the specialization layer. Two
final runs and isolated incremental comparisons retain seven samples per workload.
The lookup baseline is M9 plus the generic `$lookup` builtin, with specialization
disabled; it is not a claim about a previously supported M9 function. Table entries
are final 14-sample medians, versus seven samples of the expanded direct baseline.
Validation and full result consumption are included, serialization is excluded.

Static-object lookup, last-key hits on 501-byte records (records/s):

| Object keys | Unspecialized | Compiled lookup | Rust HashMap control |
| ---: | ---: | ---: | ---: |
| 8 | 605,098 | 2,581,181 | 3,335,310 |
| 128 | 7,064 | 2,216,392 | 3,336,420 |
| 1,024 | 133 | 2,044,176 | 3,335,691 |
| 4,096 | 10 | 2,004,931 | 3,336,989 |

The direct evaluator repeatedly groups literal object keys, with quadratic comparisons.
Compilation pays this cost once and builds a sorted UTF-16 key index. A rotating pool
of 256 keys in the 4,096-entry table reaches **1,700,926 records/s**, versus
3,271,241 for the ASCII HashMap control. The index still decodes key units during
binary search; the control uses byte hashing and omits general JSONata key semantics.
Both validate every record. Bound tables also reuse compiled data, while retaining
six lexical/identity allocations per record, independent of table width.

Folding alone removes reconstruction but retains ordinary argument evaluation.
At 1 MiB, folding-only lookup measures **943 records/s**;
reusing validating path capture raises this to **1,854**, removing a second scan
and the temporary object identity. Compilation of the 4,096-key fixture takes about
91 ms and requests 3.22 MB across 20,520 allocations. This is deliberate compile-once
work; constructor grouping still makes large-table compilation quadratic.

Other 500-byte workloads (records/s; allocations before → after):

| Workload | Unspecialized | Compiled | Allocations |
| --- | ---: | ---: | ---: |
| constant_scalar | 2,581,713 | 3,265,283 | 0 → 0 |
| dynamic_leaves | 982,535 | 1,334,453 | 11 → 4 |
| invariant_aggregate | 1,237,008 | 1,570,332 | 2 → 0 |
| builtin_reference | 2,566,895 | 3,268,194 | 5 → 0 |
| static_array | 1,992,757 | 3,167,526 | 7 → 1 |
| repeated_fields | 239,003 | 238,643 | 0 → 0 |
| filtered_sum | 1,141,558 | 1,150,141 | 0 → 0 |

The new compiled-container variant leaves `Value` (24 B), `Context` (48 B),
`Stream`/`Operand` (56 B) and `Node` (64 B) unchanged, but grows container iterators
from 24 to 32 B. Initial full runs exposed raw-array aggregate losses. Hoisting
storage dispatch out of exhaustive iterator loops improves all 16 focused aggregate
fixtures by 2–11% across two pairs. A smaller remaining-slice cursor was slower and
was discarded. The retained change uses `Iterator::for_each`, not a separate fold engine.

Selected existing workloads versus M9 (paired median changes):

| Workload | Bytes | Pair 1 | Pair 2 |
| --- | ---: | ---: | ---: |
| ascii/nested | 100 | -1.07% | -0.14% |
| ascii/nested | 500 | -3.90% | +1.23% |
| array/deep | 136 | -1.02% | +0.16% |
| filter/predicate | 100 | +3.15% | -2.34% |
| aggregate/nested_filtered_sum | 1,048,588 | +1.55% | +1.35% |
| aggregate/array_sum | 87,201 | -2.98% | -5.62% |
| construct/filtered_summary | 500 | +1.35% | +1.94% |
| scalar/escaped_strings | 26 | -5.69% | -6.06% |
| navigation/wide_group | 600,386 | -5.44% | -5.43% |

The tiny escaped-string and wide-grouping losses repeat; they are not claimed as
resolved. Neither adds allocations or lexical frames. Additional representation
cases and iterator dispatch remain costs; the measurements do not isolate every
code-generation effect. Existing tiny-record and nested-subtree rescanning limits remain.

Repeated fields are deliberately unchanged: at 1 MiB, the expression reaches
147 records/s versus 1,854 for the purpose-written control that validates once
and reads each field once. Multi-field capture, context-safe propagation across lexical
bindings, and broader demand fusion are stronger next opportunities than native code
generation. The current tree plus small specializations remains sufficient; no IR/JIT,
per-record cache or input DOM was introduced.

[Raw runs, commands, variants and hashes](benchmarks/m10/environment.json),
[all M9 comparisons](benchmarks/m10/comparison.csv) and
[specialization comparisons](benchmarks/m10/specialization.csv) retain the evidence.
Processes ran sequentially; brief source/artifact work overlapped timing and host
activity was not controlled. No cross-engine, tail-latency or retained-RSS claim follows.

## Milestone 11 — bounded numeric execution plans

Same Apple M4/compiler/profile. **606 workloads**: 13 compilation and 593 evaluation
cases, including 399 with zero allocations. Every evaluation workload has exactly
the same allocation calls and requested bytes as the M10 tree baseline. Final
`just all`, `just build`, 568 classified conformance cases and **31,200 differential
comparisons** pass. No language semantics were added.

Retain the small numeric register plan described in ARCHITECTURE. The full-run table
below uses seven-sample medians, with validation and complete consumption included.
Focused process repeats confirm the gains; no samples are discarded. Rust controls
have the narrower fixture-specific contract described above.

| Workload | Bytes | M10 tree records/s | Plan records/s | Rust control records/s |
| --- | ---: | ---: | ---: | ---: |
| Repeated fields | 500 | 238,075 | 1,011,311 | 2,952,456 |
| Repeated fields | 1,048,576 | 147 | 632 | 1,855 |
| Arithmetic-heavy scalar | 500 | 295,934 | 1,074,021 | 3,134,733 |
| Arithmetic-heavy scalar | 1,048,576 | 174 | 631 | 1,856 |
| Static schema + computed member | 500 | 286,069 | 918,536 | 3,147,412 |
| Numeric filter + sum, 128 rows | 1,082 | 41,160 | 56,449 | 290,316 |
| Numeric map + sum, 128 rows | 1,082 | 34,037 | 72,569 | — |
| Static lookup, 8,192 keys | 501 | 1,855,943 | 1,952,211 | 3,258,258 |

Repeated fields improve about **4.3×**, arithmetic **3.5–3.6×**, mapped sums **2.1×**,
and filtered sums **1.37×**. Lookup is an unchanged control: its existing indexed
node and validating key capture already avoid tree traversal through the table.
Different fields still scan separately after validation; the two-field 1 MiB plan
reads the record three times versus the Rust control's validation plus fixed-position
field reads. General capture fusion, nested subtree rescanning and replay remain
larger opportunities than native arithmetic code generation.

Two prototype runs with load sharing disabled isolate instruction lowering from
scan elimination. At 1,082 B, numeric filter and mapped-sum throughput improve
24–25% and 29%, respectively, over their paired trees. Arithmetic over a 500 B record
barely changes without shared loads. Lowering earns its place inside numeric loops,
but shared demands explain most of the record-wide arithmetic gain. Those controls
used the initial inlined prototype; the final plan keeps its register frame separate.

Inlining the prototype enlarged each recursive tree frame from 960 to 1,408 bytes
on this compiler and regressed sequence equality by 17–20%. Keeping the plan call
out of line restores the 960-byte tree frame; those workloads now differ by roughly
2–3%. This is the only retained adjustment beyond lowering and load sharing. Runtime
value/context/sequence representations and lexical boundaries are unchanged.

Costs and open regressions:

- The deliberately invalid numeric input runs about **30% slower** (9.83M → 6.84M
  records/s): the guard retries the original pure tree to preserve exact diagnostics.
  It still allocates nothing. Numeric-region compilation rises from 69 allocations /
  4,674 requested bytes to 129 / 7,694, and from about 1.85 to 2.82 µs.
- Unlowered 500 B validation/path workloads are about **2% slower**. The 100 B
  validation case loses **9–11%**; wide last-position filters lose **8–12%** in
  repeats. These are real measured differences, not an overall speedup claim.
  There is no plan execution, added scan pass or allocation on these paths. Scanner
  instruction counts are unchanged; the precise code-generation/layout cause remains
  unisolated. Preserving existing enum tags did not recover the loss and was discarded.
- The two M10 regressions remain open. Escaped-string comparison is within 1% of M10;
  600,386 B grouping loses a further **5%**. Sampling still puts grouping mostly in
  scanning, and escaped comparison in scanning/UTF-16 decoding, without lexical frame
  work. Consuming owned scalar conversion recovered only 2–3% on escaped strings and
  no repeatable grouping gain; that experiment was discarded. No broad scanner or
  value redesign is justified by this investigation.

The plan keeps ordinary borrowing and streaming, including predicates inside existing
folds. Constructed-output fixtures retain their four allocations / 312 requested
bytes; numeric loops remain allocation-free. There is no general IR, JIT, branch
lowering or replacement sequence evaluator. Further lowering should earn its place
against this baseline, with attention to unlowered workloads as well as hot regions.

[Full comparison](benchmarks/m11/comparison.csv), [tree samples](benchmarks/m11/tree-full.csv),
[final samples](benchmarks/m11/final-full.csv), [numeric repeat](benchmarks/m11/final-execution-repeat.csv),
and [compiler-workload repeat](benchmarks/m11/final-compiler-repeat.csv) retain the evidence.
[Environment, commands, variant mapping and source hashes](benchmarks/m11/environment.json)
cover focused regression runs, rejected patches, sampling profiles and frame disassembly.
Profile timings are instrumented and excluded from these tables.

## Milestone 12 — branches, capture and enclosing operations

Same machine/compiler/profile. **646 workloads**, including 632 evaluations and 423
zero-allocation cases. Fresh M11 tree and plan baselines use the same final harness.
Seven-sample medians below include full validation and consumption. Focused process
repeats confirm the main gains. The final full dataset combines completed groups
following a session interruption; raw partial/rerun samples and the deterministic
replacement rule are recorded in the environment file.

| Workload | Bytes | M11 tree records/s | M11 plan records/s | M12 records/s |
| --- | ---: | ---: | ---: | ---: |
| Arithmetic-heavy scalar | 500 | 295,059 | 1,076,523 | 1,552,112 |
| Conditional with reused fields | 500 | 468,507 | 646,768 | 1,594,220 |
| Boolean branches | 500 | 538,015 | 529,445 | 1,447,749 |
| Fixed object with computed members | 500 | 432,457 | 425,583 | 1,473,751 |
| Arithmetic-heavy scalar | 1,048,576 | 174 | 632 | 943 |
| Filter + sum, 128 rows | 1,082 | 40,969 | 56,591 | 84,391 |
| Numeric map + sum, 128 rows | 1,082 | 33,575 | 71,951 | 82,666 |
| Invoice filter/map/sum, 128 rows | 4,488 | 27,939 | 26,589 | 32,839 |
| Mapped fixed objects, 128 rows | 4,488 | 17,425 | 17,323 | 30,975 |
| 4,096-key lookup + repeated quantity | 490 | 427,506 | 417,703 | 597,840 |
| Two demanded fields among 514 keys | 7,583 | 21,151 | 28,466 | 71,756 |

The isolated no-capture control retains branches, folds and fixed objects but loads
fields separately. At 500 B, capture adds about **1.45×** for conditionals, **2.62×**
for boolean branches and **1.41×** for fixed objects over that control. Broader lowering
alone helps whole conditionals and constructors; shared input work remains essential.
Standalone indexed lookup remains essentially unchanged (8,192 keys: 1.95M → 1.99M
records/s); its useful plan role is inside a larger region with repeated demands.

All 632 evaluation cases preserve or reduce allocation calls **and** requested bytes.
Twenty constructor cases improve: fixed-object output goes from 4 calls / 568 B to
3 / 248 B; mapped two-member objects go from 3 / 296 B to 2 / 136 B per output.
Primitive programs and fused numeric folds remain allocation-free. Compilation is
slightly costlier: the conditional fixture goes from 75 allocations / 3,282 B to
85 / 3,780 B, and about 1.42 → 1.56 µs. Arithmetic compilation is nearly unchanged
(2.82 → 2.86 µs). Register cells and instructions remain 16 B; the bounded register
array is 512 B. Out-of-line execution keeps the recursive tree frame at 976 B versus
M11's 960 B, rather than incorporating that array into every tree frame.

Costs and boundaries:

- Type-error fallback is **8–9% slower** than M11 (6.93M → 6.34M records/s), still
  allocation-free. Boolean-capable loads can reach the numeric guard later before
  retrying the original tree. Exact diagnostics and error precedence remain intact.
- The untaken 496 B branch loses **5–6%** because capture reads compiled demands from
  the unselected arm. Disabling capture recovers it. Guards and computations remain
  lazy; no unselected expression errors are exposed. Adding adaptive branching or a
  general value register model was not justified.
- General sequence boundaries, nested candidate arrays, dynamic keys, lexical calls
  and directly borrowed constructor members retain the tree. An early prototype that
  converted bare numeric outputs was rejected: it lost original large-number tokens.
  The retained lowering boundary preserves those bytes and borrowing.
- Fresh full/focused runs mostly recover M11's tiny-record and last-position losses:
  100 B validation is 11.09M → 12.23M records/s in repeats; wide last-position filters
  gain about 8–11%. The 600,386 B grouping control returns from 265 to 280 records/s.
  No scanner-source change explains this; code-generation/layout causality remains
  unisolated. Escaped-string comparison remains essentially unchanged. Other unlowered
  controls include roughly 5–6% single-run losses for tiny lookup and distinct grouping;
  these are not evidence for another representation change.

Rust controls still delimit the opportunity: arithmetic reaches 3.23M records/s at
500 B and 1,859 at 1 MiB, versus 1.55M and 943. The latter is largely validation plus
one demand scan versus validation with fixture-specific reads. Invoice Rust reaches
65,788 records/s versus 32,839; the simpler filtered-sum control reaches 310,948 versus
84,391. Source-prefix/container rescanning, numeric parsing, generic traversal and
instruction dispatch remain costs. A 3 s invoice profile places about 58% of leaf
samples in scanner value/string routines and 18% in the program loop (capture and
dispatch combined); instrumented timings are excluded. Broadening the plan has not removed general
sequence normalization or positional replay. Demand-aware validation and traversal
are the next useful experiment; these results do not yet justify Cranelift.

[Full comparison](benchmarks/m12/comparison.csv), [M11 tree](benchmarks/m12/m11-tree-full.csv),
[M11 plan](benchmarks/m12/m11-plan-full.csv), [M12 samples](benchmarks/m12/final-full.csv),
[focused region repeat](benchmarks/m12/final-plan-repeat.csv),
[numeric repeat](benchmarks/m12/final-execution-repeat.csv), and
[no-capture control](benchmarks/m12/no-capture.csv) retain the measurements.
[Environment and commands](benchmarks/m12/environment.json) and
[control patch](benchmarks/m12/no-capture.patch) describe reproduction.
[Invoice profile](benchmarks/m12/profile-invoice.txt) and
[frame measurements](benchmarks/m12/tree-frames.txt) retain the supporting inspection.
`just all`, `just build`, all 568 classified cases and 35,761 differential comparisons pass.

## Milestone 13 — demand-aware validation and traversal

Same machine/compiler/profile; **679 workloads**, 665 evaluations, 456 allocation-free.
The final harness runs against committed M12 and M13, including full validation and
consumption. Each full run completes uninterrupted; medians use all seven samples.
Longer process repeats cover planned regions, nested demands and regression controls.
No JSONata semantics, dependencies, input index, cache or JIT are added.

| Workload | Bytes | M12 records/s | M13 records/s | Ratio |
| --- | ---: | ---: | ---: | ---: |
| Arithmetic | 500 | 1,553,608 | 2,705,413 | 1.74× |
| Arithmetic | 1,048,576 | 946 | 1,847 | 1.95× |
| Conditional, tiny record | 100 | 4,717,401 | 6,743,427 | 1.43× |
| Nested object fields | 500 | 686,457 | 2,784,691 | 4.06× |
| Nested conditional | 1,048,576 | 318 | 1,856 | 5.84× |
| 4,096-key lookup + quantity | 490 | 582,496 | 1,684,848 | 2.89× |
| Repeated 4,096-key lookup | 500 | 617,088 | 1,438,780 | 2.33× |
| Sparse nested object | 38,497 | 8,320 | 29,195 | 3.51× |
| Invoice filter/map/sum | 4,488 | 32,485 | 60,605 | 1.87× |
| Nested-field filter/map/sum | 8,454 | 15,585 | 38,410 | 2.46× |
| Untaken conditional | 496 | 1,606,784 | 2,973,333 | 1.85× |

Root plans capture compiled path demands during validation instead of scanning the
record again. Nested object prefixes share traversal; static lookup keys participate
in the same capture. For the nested conditional with padding inside `payload`, M12
walks that large region during validation and five field-selection scans; M13 walks
it once. Fold source paths are captured in that first pass, then element cursors
capture candidate fields while locating their boundaries. Candidates are not collected.
The ordinary standalone path/lookup selector remains separate and lightweight.

Captured raw spans are converted only when a load executes, removing number conversion
for untaken branches. Key matching and span capture still cover those branches; choosing
an arm before complete validation would complicate duplicate-key and error behavior.
All **665 evaluation cases retain the same allocation calls and requested bytes**.
An initial 24-byte-per-slot capture representation lost about 9% on mapped constructors;
optional raw spans plus a deferred-array bit mask and less initialization reduce
that cost. The original enum scratch representation is removed. Programs retain 32 primitive
registers; capture adds 32 borrowed slots and one mask. The recursive `Node::run` frame
is 960 B on this build, versus M12's 976 B; scratch stays outside that frame.

A 3 s invoice profile still places **59%** of leaf samples in scanner routines
(including capture), **8%** in key matching and **11%** in the primitive program.
The M12 program includes capture work now attributed to scanner routines, so those
percentages are not a like-for-like dispatch comparison. Rust controls reach 3.15M
arithmetic records/s at 500 B and 1,858 at 1 MiB; M13 reaches 2.71M and 1,847.
The invoice Rust control reaches 65.4k versus M13's 60.6k. These fixture-specific
controls validate input but do not implement general JSONata semantics.

Remaining costs and tradeoffs:

- Raw mapped constructors still create capture scratch after their enclosing tree
  cursor has located each candidate. The 128-row case is 30.8k → 29.2k records/s
  in the full runs and 30.4k → 29.1k in repeats (about 4% slower). General array
  normalization, negative-position replays and capture across independent tree
  regions remain outside this milestone.
- Across 137 longer-repeat workloads, tiny validation/path controls lose about
  2–3%; unplanned numeric-array aggregates lose roughly 3–7%. These paths do not
  initialize or use capture slots, but share the refactored scanner. No extra parser
  dispatch call survives inlining; the precise code-generation cause is unisolated.
  An isolated out-of-line root-plan entry reduces `evaluate::scalar`'s frame from
  1,040 B to 448 B (M12: 336 B), but does not recover those aggregate losses and
  slows tiny plans about 2%. It is not retained. A second grammar or input index
  is not justified to chase these losses. The
  apparent 10% raw-member serialization loss at 10 KiB does **not** repeat
  (80.1k → 79.8k). All full and repeat samples remain available.
- Compilation pays for demand metadata: scalar fixture allocations rise from
  65 / 3,604 B to 72 / 4,054 B; numeric-plan compilation is 133 / 7,768 B versus
  132 / 7,864 B. Ordinary path compilation remains 8 allocations / 578 B.
- Arrays along an object demand defer that load to the existing pure tree. Capturing
  and then falling back can still repeat work. Source arrays are validated before
  a second traversal computes folds; no trusted selective parser has been introduced.

[Full comparison](benchmarks/m13/comparison.csv), [M12 samples](benchmarks/m13/m12-full.csv),
[M13 samples](benchmarks/m13/final-full.csv), [M12 repeats](benchmarks/m13/m12-repeat.csv),
[M13 repeats](benchmarks/m13/final-repeat.csv), and
[environment/commands](benchmarks/m13/environment.json) retain the evidence.
[Before profile](benchmarks/m13/m12-profile.txt), [after profile](benchmarks/m13/final-profile.txt)
and [frame inspection](benchmarks/m13/frames.txt) support the traversal review.
`just all`, `just build`, all 568 classified conformance cases and 37,746 upstream
differential comparisons pass. Exact validation-error mutation tests cover malformed
captured/unselected fields, duplicate parents, UTF-8, trailing bytes and depth limits.

## Milestone 14: scoped path composition

`@`/`#` paths carry streamed values and evaluated bindings through the existing
scalar, filter, ordering and grouping semantics. Ordinary paths do not create rows
or scope frames; eligible scalar leaves still use M13 demand capture. No scanner,
plan instructions, input index or JIT machinery changes in this milestone.

On the same Apple M4 / Rust 1.98.1 release configuration, compare committed M13
`7279177` with M14: **679 → 723 workloads**, including 43 new evaluation cases and
one scoped-path compilation case. Full runs use seven 50 ms samples; 59 existing
workloads also have seven 150 ms repeats. All **665 existing evaluation cases keep
identical allocation calls and requested bytes**; 456 remain allocation-free.
Compilation of the ordinary path remains eight allocations, 578 → 618 requested
bytes from added step metadata. Scoped-path compilation is measured separately.

Representative full-run medians (records/s):

| Workload | Bytes | M13 | M14 | M14 allocations / requested bytes |
| --- | ---: | ---: | ---: | ---: |
| Shallow path | 500 | 3.19M | 3.20M | 0 / 0 |
| Planned arithmetic | 500 | 2.74M | 2.74M | 0 / 0 |
| Varying 4K static lookup | 501 | 1.69M | 1.71M | 0 / 0 |
| Planned invoice sum, 128 rows | 4,488 | 60.8k | 60.3k | 0 / 0 |
| Planned mapped objects, 128 rows | 4,488 | 29.5k | 29.3k | 256 / 17,408 |
| Scoped indices, two rows | 500 | — | 1.05M | 11 / 1,104 |
| Scoped filter + sum, two rows | 500 | — | 793k | 13 / 1,648 |
| Scoped constructor with plan leaves | 500 | — | 670k | 17 / 1,696 |

Wide scoped sums reach 20.9k records/s at 128 rows (6,666 B) and 2,625 at 1,024
rows (56,043 B), requesting 454 / 57,088 B and 3,590 / 451,328 B respectively.
Bindings and short-lived lexical frames allocate per candidate; streaming bounds
live rows but does not mean zero allocation. Sorting, grouping and predicates that
may index from the end retain rows. Padded 1 MiB scoped fixtures reach about 943
records/s; requested storage stays the same as their two-row 500 B versions.
These byte counts measure total allocation requests, not peak resident memory.

The unbound join `rows@$r.bands[kind=$r.kind]` repeats the root-field scan per row:
its aggregate reaches 1,055 records/s at 128 rows and 19 at 1,024. Binding `bands`
once before the path gives 6,678 and 833 respectively, with the same result and
borrowed leaves. This is an explicit workload-level reuse opportunity, not an
automatic optimizer claim. Nested tuple traversal and lexical-frame costs remain
measurable; no dynamic indexing or tuple-specific plan lowering is added.

Regressions and negative evidence:

- Full-run tiny-filter/retention losses of 7–11% shrink in longer repeats: wide
  predicates lose about 2–3%, 100 B singleton retention about 4%, and computed-last
  selection about 1%. The apparent 9% bound-static-lookup loss does not repeat.
- Unplanned sequence sums lose 3–7% in repeats (24,936 → 23,306 records/s at 4,017 B;
  1,496 → 1,389 at 87,201 B). Tiny raw-member construction loses about 5%; one wide
  last-position case also loses about 5%. Their allocations are unchanged. Shared
  tree dispatch/consumption changed, but the precise code-generation cause is not
  isolated; these paths do not initialize tuple bindings or change JSON scanning.
- Repeated planned arithmetic/conditionals and invoice sums stay within about 1%;
  mapped objects lose about 1% at 128 rows. An isolated generic filter callback
  experiment gives no consistent improvement across 27 paired workloads and is
  not retained. No annotations or specialized fast paths are added to chase noise.

[Full comparison](benchmarks/m14/comparison.csv), [M13 samples](benchmarks/m14/m13-full.csv),
[M14 samples](benchmarks/m14/final-full.csv), [longer repeats](benchmarks/m14/repeat-comparison.csv)
and [environment/commands](benchmarks/m14/environment.json) retain the measurements.
`just all`, `just build`, all 611 classified upstream cases and 38,686 differential
comparisons pass. The exact engine/test/harness hashes are in
[source.json](benchmarks/m14/source.json); callback experiment samples and its small
patch preserve the discarded result separately from final timings.

## Milestone 15 — standard-library expansion

Same machine/compiler/profile; committed M14 `0d5aa9e` is the baseline. **800
workloads** now include 16 compilation cases and 784 evaluations. All **708 existing
evaluation cases preserve allocation calls and requested bytes**, including their
456 allocation-free cases. Of 76 new evaluation cases, 23 allocate nothing, bringing
the total to **479**. Full runs take seven 50 ms samples; longer sequential repeats
cover 73 existing and 14 new workloads. A second paired run checks 11 regression
controls. Baseline overlap/interruption details are recorded in the environment file.

Selected new full-run medians include validation and complete consumption:

| Workload | Bytes | Records/s | Allocations / requested bytes |
| --- | ---: | ---: | ---: |
| String length | 500 | 1,440,651 | 0 / 0 |
| Trim + uppercase | 500 | 827,957 | 23 / 387 |
| Map native `$abs`, three items | 500 | 1,151,208 | 3 / 224 |
| Map captured closure, three items | 500 | 675,195 | 11 / 1,208 |
| Reduce numeric array, three items | 500 | 935,340 | 8 / 904 |
| Planned filter/map/average, 128 rows | 4,537 | 75,741 | 0 / 0 |
| Higher-order filter/reduce, 128 rows | 4,537 | 17,328 | 396 / 52,680 |
| Map constructors + merge, 1,024 rows | 37,737 | 212 | 17,438 / 1,107,312 |

`$average` reuses the existing primitive fold and eligible numeric plan. Direct
string length, numeric helpers and type names need no heap storage. Transforming
strings uses UTF-16 work buffers and shared encoded output. Higher-order calls
retain evaluated sequences once and invoke the existing closure runtime; raw array
inputs and output leaves still borrow. Callback scope frames allocate, and requesting
the original collection can require a singleton wrapper. Reduce holds an accumulator
rather than a second mapped collection. Padded 1 MiB fixtures retain the same
allocation counts/bytes as their 500 B versions; record padding is never copied.

Relevant limitations and regression evidence:

- Distinct/key deduplication and merge use linear searches. Wide distinct-key merging
  is quadratic: the same mixed workload reaches 6,238 records/s at 128 rows and 212
  at 1,024. These include callback, string and constructor work. Requested bytes
  above are cumulative allocator requests, not peak memory.
- Existing planned arithmetic is effectively unchanged in repeats (2.721M → 2.723M
  records/s at 500 B); invoice sums and mapped objects remain close to M14. The
  apparent full-run 21% lexical lookup and 9% grouping losses at 64 KiB disappear
  in repeats. Nested filtered sums remain within 1%; unplanned sequence sums gain
  about 1–2% on these runs. No general speedup follows from these controls.
- Tiny validation/path controls lose about **2–3%** across two paired repeats.
  A 4K static last-key hit loses **4–7%**; the 128-row last-position filter loses
  **1.5–7.2%**, showing significant run variation. Allocations are unchanged, and
  these controls execute no new library calls, scope frames or scanning passes.
  Scanner and plan sources are unchanged; the precise code-generation cause remains
  unisolated. No special-case paths or annotations are added to chase these losses.
- Compilation also marks lambda tail-call boundaries to preserve observable native
  sequence shape. Plain-path compilation is 4.89M → 4.42M/s in the full runs, with
  unchanged eight allocations / 618 requested bytes. Compile cost remains separate
  from repeated execution.

[Full comparison](benchmarks/m15/comparison.csv), [M14 samples](benchmarks/m15/m14-full.csv),
[M15 samples](benchmarks/m15/final-full.csv), [longer repeats](benchmarks/m15/repeat-comparison.csv),
[second controls](benchmarks/m15/control-comparison.csv), and
[environment/commands](benchmarks/m15/environment.json) retain the evidence.
[Source hashes](benchmarks/m15/source.json), [checks](benchmarks/m15/checks.log) and
[differential results](benchmarks/m15/differential.txt) identify the tested build.
`just all`, `just build`, all 820 classified upstream cases and 40,012 differential
comparisons pass. Regex, transforms and the remaining library gaps stay explicit in CONFORMANCE.

## Milestone 16 — conversion and function composition

Same machine/compiler/profile; committed M15 `1d6f3bf` is the baseline. **910
workloads** include 18 compilation cases and 892 evaluations. All **784 existing
evaluations preserve allocation calls and requested bytes**; 41 new allocation-free
cases bring the harness total to **520**, including Rust controls. Full runs use
seven 50 ms samples. Longer sequential comparisons cover 78 existing workloads,
19 additional controls, 17 conversion cases and 18 mixed pipelines. Baseline overlap
and experimental-run boundaries are recorded in the environment file.

Selected new full-run medians include validation and complete consumption:

| Workload | Bytes | Records/s | Allocations / requested bytes |
| --- | ---: | ---: | ---: |
| Borrowed string conversion | 500 | 2,893,820 | 0 / 0 |
| Numeric string conversion | 500 | 2,711,038 | 0 / 0 |
| Number stringification | 500 | 1,587,544 | 8 / 105 |
| Concatenation | 500 | 699,591 | 14 / 251 |
| Direct string pipeline | 500 | 938,387 | 16 / 242 |
| Function-value composition | 500 | 668,834 | 34 / 1,254 |
| Repeated partial use | 500 | 506,519 | 35 / 2,218 |
| Conversion/filter/map/sum, 128 rows | 3,886 | 22,624 | 145 / 22,856 |
| Chained numeric filter/map/sum, 128 rows | 1,180 | 130,640 | 0 / 0 |

Two small changes have measurable value:

- A root conversion of one static path captures its argument during full validation.
  The matched 150 ms before/after measurements improve numeric text **1.525M →
  2.807M records/s (1.84×)** and borrowed strings **1.565M → 2.987M (1.91×)** at
  500 B, with no allocation change. Object stringification improves 1.43×; ordinary
  composition controls remain close. The fixed-layout Rust numeric-text control uses
  the same validating selector plus Rust parsing and reaches 2.933M/s in the full run.
- Joining validated string bodies removes a redundant decode/encode pass. Focused
  concatenation improves **618k → 708k records/s**, with **20 / 300 B → 14 / 251 B**.
  Its pre-change samples come from an experimental harness with a subsequently fixed,
  unrelated fixture failure; final checks and all authoritative timings pass.

Known builtin call chains lower to ordinary calls. Direct and chained numeric folds
remain allocation-free and within about 1% in longer repeats at 8, 128 and 1,024 rows.
No string/function plan instructions are added. Partial calls fill small argument lists
on the stack; repeated partial creation merges bound slots once. Borrowed strings,
numeric text and retained input leaves are not copied. Padded 1 MiB conversions keep
their 500 B allocation counts/bytes; root conversions reach about 1,854–1,859 records/s.
Compilation stays separate: ordinary paths retain eight allocations / 618 B.

Regression investigation and remaining costs:

- Larger full-run closure losses do not repeat: 1,692 B mapped calls are 42,209 →
  42,584 records/s; the 1 KiB closure control is 485,233 → 487,545. Padded 10/64 KiB
  validation/path controls also converge. Repeated 500 B arithmetic is 2.746M →
  2.730M/s; invoice sums and nested filtered sums remain close to M15.
- A 4,017 B array count loses about **3.8%** and a 2,736 B last-position filter about
  **2.6%** in repeats; tiny mapped calls lose about 2%. Allocations and traversal
  passes are unchanged. Scanner and plan source are untouched. The recursive tree
  frame grows **32 B (1,040 → 1,072 B)**; conversion/composition helpers stay outside
  it. Remaining code-generation causality is unisolated; no speculative fast paths
  or annotations are added for these small differences.
- General conversions inside larger tree regions still use separate validation and
  field scans. JSONata stringification owns formatted output and temporarily retains
  each object's members for duplicate/order rules. It is deliberately separate from
  token-preserving output. Fractional formatting and escaped numeric input also need
  temporary storage; no general DOM is built.
- Dynamic calls/composed functions retain evaluated arguments and intermediate sequences.
  Higher-order pipelines therefore pay callback/frame and collection costs, unlike
  streamed path predicates and known aggregate chains. At 1,024 rows, the numeric
  conversion pipeline requests 1,047 allocations / 180,552 B; label/filter/sort/join
  requests 3,636 / 341,457 B. These are cumulative requests, not peak memory. Longer
  allocating mixed runs are 5–10% below full-run medians; both datasets are retained.

[Full comparison](benchmarks/m16/comparison.csv), [longer repeats](benchmarks/m16/repeat-comparison.csv),
[focused controls](benchmarks/m16/control-comparison.csv),
[capture experiment](benchmarks/m16/path-capture-comparison.csv),
[mixed repeats](benchmarks/m16/mixed-repeat.csv) and
[environment/commands](benchmarks/m16/environment.json) retain the evidence.
[Source hashes](benchmarks/m16/source.json), [frames](benchmarks/m16/frames.txt),
[checks](benchmarks/m16/checks.log) and [differential results](benchmarks/m16/differential.txt)
identify the tested build. `just all`, `just build`, all **924** classified upstream cases
and **42,754** differential comparisons pass. Related limitations remain in CONFORMANCE.
