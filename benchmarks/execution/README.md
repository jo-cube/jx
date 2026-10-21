# Experimental execution controls

The `experimental` feature adds the shared semantic execution foundation. Legacy
`Expression`, CLI and native routing remain the default. Unsupported syntax, callee
domains and input shapes fail explicitly; there is no compatibility replay. See
[the execution model](../../docs/execution.md) and [reproduction](reproduce.md).

The pinned gate admits 1,426 of 1,679 cases: 1,227 successful outcomes, 198 expected
errors and one limit. There are 250 compile rejections and three parser errors. Of
1,390 legacy-success cases, 163 remain unsupported. First missing mechanisms are
tuple/index/ancestry contexts (68), matchers (37), formatted library calls (29),
copy/update identity (12), dynamic program ownership (10) and invocation adapters
(7). Groups have overlapping dependencies. Generated upstream controls also find
valid fused path-fold array shapes that remain runtime rejections. These counts
do not establish complete library or public API parity.

## Retained fixtures

| Source | Responsibility |
| --- | --- |
| `crates/jx/benches/architecture.rs` and modules | Matched validation, acquisition, execution and serialization; varied record sizes and field distributions |
| `architecture/calls.rs` | Captured/parameter/dynamic calls, callbacks, lexical forms and deferred delivery |
| `architecture/sequences.rs` | Candidate stages, positions, grouping, order, ranges and numeric controls |
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

Wide projections and shared acquisition are the strongest measured areas. Compact
numeric folds and generic sequence stages can trail legacy substantially. Dynamic
calls, conservative scope promotion and aliases retain acquisition/materialization
cliffs. Constructors add compilation cost; grouping/order retain candidates. Warm
state and link layout can materially affect rates. Preserve raw samples and binary
identities; investigate broad regressions before default adoption.

Semantic convergence and migration remain necessary. No legacy subsystem can yet
be retired wholesale. Historical reports, profiles, trial variants and raw evidence
bundles are review artifacts, not build/CI inputs. New measurements belong under
ignored `target/` or external review storage.
