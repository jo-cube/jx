# Architecture

`crates/jx` owns JSONata semantics, compilation, acquisition and execution.
`crates/jx-cli` owns NDJSON framing, files and output policy. Both use the same
execution foundation. No runtime engine selection or AST evaluation is involved.

```text
source → parser / lexical and ancestry analysis → constant preparation
       → shared semantic IR → local optimization / acquisition / register layout
JSON bytes → complete validation + query-sized acquisition → execution → delivery
```

## Programs and evaluation state

`compile` and `CompileOptions::compile` return immutable `Expression` values that
share an `Arc<execution::Program>`. The parser tree is discarded after lowering.
Programs are `Send + Sync`; clones share code, immutable bindings and acquisition
templates. An evaluation owns its lexical arena and retained values. It may borrow
input bytes and program constants. There is no global mutable cache or input DOM.

Ordinary byte evaluation validates the complete input and acquires statically
demanded fields automatically. `evaluate_validated` skips only validation.
`InputPlan` joins demands from several expressions and prepares a document once;
independent indexed evaluations share offsets, never lexical writes or results.
There is no fixed capture limit. `execution::Workspace` is optional caller-owned
reusable offset storage. Its exclusive mutable borrow ends before result delivery;
results can outlive the scratch while still borrowing the input and program.

## Semantic and physical execution

The common IR represents focus, root, lexical reads/writes, branches, calls,
constructors and sequence regions. Structured branch lowering preserves dominance
and lexical versions. Local CSE and copy/dead-value elimination preserve effects,
evaluation order and failures. Conservative live intervals assign reusable physical
cells to logical values, including synchronous child captures and branch joins.

General and scalar dispatch use those same instructions, identities and layouts.
Compatible complete sequence consumers share row acquisition through IR-derived
regions. Pure unfiltered maps with a proven non-array result execute their existing
mapper body while acquiring each row; eligible filtered regions compose scalar
operations. Cardinality, effectful, controlled and cancellable boundaries retain
the general semantic path. No guard replays previously executed language work.
Physical strategy is chosen before execution, rather than by recognizing expression
strings or aggregate names.

## Borrowing, contexts and calls

Raw JSON tokens retain their original spelling until computation requires a scalar.
Input views carry borrowed fragments and demand-sized provenance. Byte/base identity
checks guard field offsets across lexical aliases, arguments, closures, nested
programs and ancestor contexts. Unknown origins navigate their actual values.
There is no unconditional full index or permanent input ancestry graph.

Correlated regions carry only demanded tuple/index/ancestry metadata. Ordinary
scalar values have no tuple state. Shared tuple, filter, grouping, ordering,
conversion and transform kernels consume compiled IR bodies. Parent context is
expression-path ancestry, not a generic JSON parent pointer.

Function values refer to compiled definitions and retained lexical arena frames.
Closures observe live assignments, signatures are checked by the common call ABI,
and the tail trampoline reuses frames. Dynamic `$eval` compiles through the same
parser and IR. Invariant source sites retain immutable code or lazy syntax errors;
actual target resolution, arguments, focus and lexical scope still govern invocation.
Nested immutable programs propagate acquisition demands through invocation regions.
Truly dynamic sources have evaluation-local lifetime bridging, not a global cache.

## Delivery and boundaries

Completed results avoid deferred state. Streamed results retain only the state they
need until consumption; replay, identity, lexical lifetime and effects can require
materialization. Validation precedes bindings, controls and host effects. Consumer
cancellation stops delivery immediately. Earlier complete output remains visible
when later execution fails; complete serialization rolls back incomplete values.

Constant preparation executes eligible effect-free nodes through this same IR and
captures immutable data. Failed folds remain runtime expressions. Source nodes and
semantic analysis stay compile-time facilities, never executable result bodies.

Releases contain one interpreted common-IR executable. There are no engine-selection
features, native installation APIs or Cranelift dependencies.

[Embedding](docs/embedding.md), [compatibility](docs/compatibility.md) and
[conformance](CONFORMANCE.md) define public contracts and known reference differences.
