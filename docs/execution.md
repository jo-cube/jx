# Execution foundation

The ordinary public compiler and CLI use the common semantic IR. See
[architecture](../ARCHITECTURE.md) for lowering, acquisition, calls and delivery,
and [embedding](embedding.md) for public APIs.

`Expression` shares immutable code. `InputPlan` shares preparation across programs.
`execution::InputTemplate` and `execution::Workspace` separate immutable demand
metadata from reusable mutable offsets for callers that need explicit scratch
ownership. Offsets retain no input bytes, lexical environment or result graph.
A workspace may be reused after an evaluation returns; results borrow input/code,
not its capture frame. Use separate workspaces concurrently.

General functions, closures, partials, callbacks, dynamic code, correlated paths,
constructors and folds execute the same compiled programs. Scalar layouts and
fused complete filter/map regions are physical strategies of that IR. General
callbacks, ordering/grouping, effects and retained lexical values can require
materialization. Uncached indirect/tail dynamic compilation remains an efficiency
limit. There is no legacy retry or executable AST body.

`CompileOptions::compile_program` exposes the same compiler without the expression
wrapper. `Expression::execution_program()` borrows its immutable code. There is no
alternative execution engine, native feature or executable-memory dependency.

The pinned corpus has an explicit outcome for every case. Compilation failures,
evaluation failures, missing results and serialized values are distinct contracts.
Existing immutable transform-alias snapshots, regex/coercion and formatting
boundaries are documented in [compatibility](compatibility.md). Admission alone is
not proof of full upstream or embedding equivalence.

Run `just all` and `just build`; see the [performance workflow](performance.md)
for measurements and [benchmark fixtures](../benchmarks/execution/README.md).
