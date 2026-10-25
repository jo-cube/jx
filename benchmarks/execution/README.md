# Experimental execution controls

The `experimental` feature adds the shared semantic execution foundation. Legacy
`Expression`, CLI and native routing remain the default. Unsupported syntax, callee
domains and input shapes fail explicitly; there is no compatibility replay. See
[the execution model](../../docs/execution.md) and [reproduction](reproduce.md).

The pinned gate admits 1,638 of 1,679 cases: all 1,390 legacy-success outcomes,
247 expected errors and one limit. The remaining 38 compile rejections and three
parser errors are expected-error cases, not missing successful tuple semantics.
Generated controls exercise correlated stages, ancestry, retained callbacks and
valid fold-array shapes. They separately assert inherited transform-alias differences,
upstream empty-tuple grouping exceptions and the direct-regex coercion boundary.
Admission does not establish complete public API, host/coercion or native parity.

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
| `workloads/realistic.rs` and `realistic_memory.rs` | Default-engine strings, immutable tables, projections, predicates and equivalent forms |

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
identities; investigate broad regressions before default adoption.

Opt-in `compile_execution` facade controls include automatic acquisition, workspaces
and the immutable shared-input API. `scripts/check-execution-facade.py` replays the
public contracts with documented interpreter-only native requests. Default/CLI
migration and compatibility validation remain necessary. No legacy
subsystem can yet be retired wholesale. Historical reports, profiles, trial variants and raw evidence
bundles are review artifacts, not build/CI inputs. New measurements belong under
ignored `target/` or external review storage.
