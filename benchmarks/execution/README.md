# Common execution controls

Ordinary `compile`, evaluation, prepared inputs and the CLI use the same semantic
IR; there is no engine-selection API.
[Execution contracts](../../docs/execution.md) and [reproduction](reproduce.md)
describe current acquisition, delivery, workspace and ownership behavior.

The pinned gate admits 1,638 of 1,679 cases: all 1,390 successful outcomes from the migration reference,
247 expected errors and one limit. The remaining 38 compile rejections and three
parser errors are expected-error cases, not missing successful tuple semantics.
Generated controls exercise correlated stages, ancestry, retained callbacks and
valid fold-array shapes. They separately assert inherited transform-alias differences,
upstream empty-tuple grouping exceptions and the direct-regex coercion boundary.
Admission is distinct from complete public API and upstream compatibility.

## Retained fixtures

| Source | Responsibility |
| --- | --- |
| `crates/jx/benches/architecture.rs` and modules | Matched validation, acquisition, execution and serialization; varied record sizes and field distributions |
| `architecture/calls.rs` | Captured/parameter/dynamic calls, callbacks, lexical forms and deferred delivery |
| `architecture/sequences.rs` | Candidate stages, positions, grouping, order, ranges and numeric controls |
| `pipeline-cases.json` | Sequence length, selection distribution and direct/grouped/alias/call/callback forms with matched entry fixtures |
| `context-cases.json` | Index/focus/ancestry, contextual callbacks, predicates, ordering and grouping at 8/1,024 rows |
| `semantic-cases.json` | Matchers, prepared formats, dynamic IR and compiled copy/update controls |
| `architecture/scaling.rs` | Compilation time, allocations, retained/peak heap and program size by complexity |
| `architecture/memory.rs` | Separate compilation, acquisition and execution heap probes |
| `architecture/jkq.rs` | Actual-worker driver for an isolated read-only sibling experiment |
| `throughput.rs` and `workloads/realistic_memory.rs` | Compact public-API throughput and retained-heap controls |

JSON selectors keep ordinary, call, sequence and worker controls separate. Fixtures
are generated deterministically before timing; no generated input corpus is needed.
The larger matrix is optional. The Cargo target `architecture` is a benchmark, not
another execution engine.

## Measurement contracts and limitations

Compile once and reuse code. Match validation, consumption, serialization and
ownership. Already-validated acquisition is a separately labelled control. Core
timings serialize every emitted value to a reused buffer; workers complete JSON
output and match payload/output ownership. Never compare rejection time with
successful execution, or cancellation with eager delivery obligations.

Timing disables allocation counters. Separate warmed probes count alloc/realloc,
requested bytes and incremental requested peak live heap. Input, code, schemas,
lookup storage, stack and existing output capacity are excluded. This is not RSS.
Samples use two reversed process orders; spread is not a confidence interval.

Wide projections and shared acquisition are the strongest measured areas. Complete
scalar filter/map regions also share row acquisition and execution; grouped folds
can approach legacy throughput. Compact numeric folds and generic callbacks can
still trail legacy. Eager cardinality barriers remain explicit even when fusing
them would improve timings. Dynamic
calls, conservative scope promotion and aliases retain acquisition/materialization
cliffs. Constructors add compilation cost; grouping/order retain candidates. Warm
state and link layout can materially affect rates. Preserve raw samples and binary
identities; investigate broad regressions before release.

Ordinary compilation automatically uses acquisition. Public contract tests cover
workspaces, cloning/concurrency, borrowed and owned delivery, immutable bindings,
errors and cancellation. Historical engine labels in the diagnostic harness now
name entry points to the same engine; compare separate source-revision binaries
for a real before/after measurement. See [absolute public controls](../absolute/README.md)
for a small release comparison without backend-selection instrumentation.
