# Execution foundation

The `experimental` feature exposes `jx::execution`. The default `Expression`, CLI
and native backend retain legacy routing. Foundation selection is explicit at
compilation; unsupported semantics never cause a tree retry.

## Compile, prepare and consume

`CompileOptions::compile_program` shares parsing, provenance, binding analysis and
constant preparation, then lowers an owned register program. Constant preparation
can still evaluate AST nodes during compilation. It is not a runtime fallback.
Unsupported instructions fail compilation; some admitted expressions still reject
valid input shapes. Both boundaries prevent default adoption.

Programs are immutable, cloneable and `Send + Sync`. Clones copy code/operand arrays
and share immutable constants, schemas and acquisition tokens. Evaluation state is
independent. Prefer `Arc<Program>` when copying code is unnecessary.

`Acquisition::new` unions root demands from multiple programs. Each worker owns its
span buffer; clones share metadata. `prepare(bytes)` validates every input byte and
captures demanded spans together. `prepare_validated(raw)` acquires spans without
repeating grammar validation. It still walks token boundaries. Preparations replace
old spans, including decoded duplicate-key subtrees. There is no mandatory input DOM
or record-result cache.

`evaluate(prepared)`, `evaluate_bytes(bytes)`, `evaluate_validated(raw)` and
`evaluate_without_input()` return the existing `Evaluation` interface. Byte evaluation
validates without shared acquisition; use prepared input for shared field access.
Absent focus/root are missing, distinct from JSON null.

Pure terminal paths defer execution until consumption. A one-shot continuation owns
query-sized span copies and shares metadata when its preparation can be reused or
dropped. Capture identities survive this ownership boundary. Input/code leaves stay
borrowed. Raw/validated evaluation needs no span snapshot. Eager lexical and grouping
boundaries still complete before returning results; parentheses are not universally
interchangeable with terminal streaming.

`try_for_each(prepared, consumer)` avoids an escaping continuation and span copy.
Eligible terminal fields/paths stream directly, and a consumer failure stops later
work. Required eager barriers remain. `write_json(prepared, buffer)` completes output
and restores the original buffer length on failure. Its rollback contract can permit
streaming a terminal retained path which externally cancellable consumption must
finish eagerly. Scope guards remain live through delivery and unwind on failure.
A serialization failure does not cancel complete evaluation: a later language error
takes precedence. Compare interfaces only with matching output/ownership obligations.

`evaluate_with`, `evaluate_validated_with` and `evaluate_prepared_with` accept
`EvaluationOptions`: declared bindings, independent focus/root, host functions,
limits, deadlines and cooperative cancellation. Validation precedes binding failures,
controls and effects. Controls share the lexical runtime and result wrapper. Work
charges follow executed instructions/items; their counts need not equal AST work
charges. They are cooperative budgets, not a process memory quota. Terminal
`try_for_each`/`write_json` do not have settings overloads; consume a controlled
`Evaluation` when settings are needed.

Returned values may outlive acquisition buffers while borrowing input/program.
`to_owned` detaches JSON-compatible data. Functions have no JSON encoding and remain
evaluation-local; only host functions may enter a different evaluation through
bindings or external focus.

## Calls and lexical lifetime

Function definitions contain programs, parameter names, signatures and semantic tail
annotations. General invocation uses the shared value ABI, argument storage, lexical
arena, signature checks and tail trampoline. Builtin callbacks, partial application,
composition and host reentry call the same program bodies. No AST body is embedded
in a compiled definition.

Closures capture definition focus/wrapping and a live arena frame index. Reads observe
later assignments and forward/recursive bindings. Values own no reference back to
the arena, so recursive bindings create no ownership cycle. Activation regions reclaim
temporary captures unless returned values or older-frame writes reach them. Uncaptured
frames recycle; tail calls reuse the existing guarded trampoline. Non-tail calls do
not spend the tail-call budget.

A program execution domain checks value-call targets, including callback, partial,
owned-definition and tail transitions. AST-backed callees and unsupported builtin
implementations fail explicitly. This also guards indirect resolution of a declared
but unbound builtin name. Direct reserved intrinsic syntax such as `$eval` remains
unsupported even when a host override could handle it; ordinary declared function
names use the general call ABI.

Straight-line locals and proven immediate callbacks can retain SSA ports. A source
scope analysis selects retained cells for escaping/callable values and enclosing
writes. This is conservative promotion, not a complete closure conversion or liveness
analysis. Proven locals and parameters keep register ports even inside retained scopes.
Read-only branches preserve those ports; calls invalidate reusable expressions but
cannot write their caller's lexical frame. Root rebinding and escaping captures still
use live cells. Immediate callbacks enforce the same call/stack/tail quotas as lexical
calls, with inline depth state when no frame is needed.

Definitions carry deduplicated root, definition-focus and parameter demands, and at
most one proven return origin. Statically resolved calls substitute these summaries
into the common acquisition analysis; they do not inline or duplicate bodies. Direct
IR calls forward prepared input through the shared ABI and restore borrowed spans by
byte identity. Unknown returned values and lexical reads can use the existing local
acquisition operation when independent fields are co-used. Already acquired values
reuse a covering schema. Dynamic calls retain their normal checks and effects.

A row capture identifies its active Prepared view, separately from the immutable
schema used for read-layout hints. Nested activations can reuse one schema without
confusing their rows. Restoration searches active views by borrowed byte identity;
no token is dereferenced. Values become borrowed bytes before a row view ends, so
returned values and closures retain no reference to its temporary span buffer.

Generic higher-order adapters, partial application and composition still pass values
without forwarding prepared-input metadata. Row capture tokens are activation-local; only borrowed language values escape. These boundaries can lose indexing and remain optimization
gaps. Scope selection is still conservative across an entire program: an unused
function can promote otherwise immediate callbacks to retained invocation.

## Compiler and physical execution

The semantic program represents focus/root, arguments/captures, live lexical reads
and writes, scopes, functions/calls, ordered operations, branches, constructors,
paths, aggregation and materialization. Registers have no fixed eligibility bound.
Joins can write multiple definitions; this is structured register dataflow, not
strict SSA. Identity-producing values are not commoned. Unknown builtin-name reads
also retain fresh function identity when lexical lookup falls back to a builtin.

Native call results retain their sequence shape until the consumer's normalization
boundary. Postfix array retention is explicit. Retained field lookup observes matches
before ordinary path flattening; keeping an already normalized value would lose
nested-array cardinality. General routes compile each step and predicate into the same IR. Stack-scoped
producer views share the reference engine's mapping, literal-position, candidate
selection and error-order kernels. Arbitrarily composed non-tuple routes, array
focus, later numeric predicates, ranges, wildcards and descendants can therefore
execute without an AST body or another evaluator. Negative positions obtain length
lazily. Replay-safe views may revisit preceding pure work for cardinality/error
precedence; effectful stages retain borrowed candidates before downstream replay.
Consumer cancellation propagates immediately through the shared view contract.

Static unique-key constructors execute one compiled focus region. Object members
receive unwrapped focus, even when their enclosing constructor saw a wrapped array.
Array grouping establishes the grouped focus before values execute. Nested members
are compiled once, and known lexical versions cross this non-iterating region as
ports. Prepared cells and row-read layouts survive that region when their guarded
input origin is unchanged. Repeated operators own reusable register workspaces for
wide regions; values are cleared between iterations and only capacity is retained.
One-shot and small regions use stack frames. Dynamic/duplicate keys use the shared grouping storage with compiled key and
value programs. Key collection precedes value evaluation. Group order, UTF-16 key
equality, duplicate errors and grouped container identity are common kernels.
Order-by uses the existing stable fallible merge order and lazy replay-safe key
cache, invoked with compiled terms; effectful keys are evaluated in comparison order.

Local deterministic CSE, copy propagation, infallible DCE, field composition and
single-use aggregate fusion precede physical preparation. Callback-capable kernels
are not commoned as pure scalars. Stage fusion proves that neither side can call
functions or write lexical state; delaying errors alone cannot preserve effects.
One structural child-program visitor serves preparation, capture traversal/remapping,
local-acquisition traversal and lexical-write summaries. One structured branch
lowerer owns lexical-version joins and dominance for conditional, default/coalescing
and short-circuit operations. Context-sensitive acquisition facts remain explicit.
Recursive preparation builds acquisition bindings, scalar layouts, load transfers
and delivery. Lexical-write facts are computed bottom-up from the IR, separately from
source effects that govern scheduling. Definition summaries have one proven origin,
never combinations of branch alternatives. This is not a general pass framework.

Two dispatch loops consume the same instructions. The straight-line scalar kernel
reuses slots; the general executor uses logical slots. Checked numeric/value kernels
are shared. Inline frames spill dynamically without changing admission. There is no
physical guard replay. Compact typed instructions and general frame liveness remain
unimplemented.

Root/row captures, owned indexed aliases and raw navigation are input strategies.
Failed read guards navigate the same value once. Separate origin analyses,
`Fold`/`Reduce`, and immediate `Each` versus generic higher-order value kernels remain
migration debt. General function calls do not yet share all acquisition/fusion benefits
of immediate callbacks.

## Dynamic coverage and boundaries

Constant preparation can remove unsupported syntax. A constant-only example does not
establish dynamic support.

| Area | Available | Remaining legacy capability |
| --- | --- | --- |
| Values/operators | Borrowed JSON, missing/null, scalars, arrays, static and dynamic objects, grouping, ranges, scalar operators, membership, postfix retention | Tuple-aware grouping; complete input-shape support in fused path folds |
| Navigation | Static fields, general non-tuple routes and predicates, later positions, array focus, wildcard/descendants, order-by, conditional acquisition | Tuple bindings and ancestry; tuple-aware ordering |
| Scope/calls | Lexical blocks/writes, live/escaping closures, recursion/tail calls, signatures, general arity, first-class supported builtins, partials/composition | Direct reserved-intrinsic host adaptation; general producer/consumer call results |
| Library | Lookup, truth/existence, aggregates, common string/numeric/collection kernels, map/filter/reduce/each/sift/single and sort callbacks, error/assert | Regex literals/library surface, formatted numbers/dates, eval, transform/clone, random/time |
| Embedding | Compile/clone/reuse, immutable/runtime bindings, host calls/reentry, external focus, absent/validated/shared input, controlled borrowed results | Complete eligibility, terminal settings overloads, common-IR native execution |

Fused paths still reject some array source/result shapes. Unsupported syntax in an
unselected branch is rejected at compilation. A runtime shape/callee-domain rejection
can follow preceding streamed output. Neither is a hidden compatibility fallback.

The full pinned corpus has an explicit admission entry in
[`tests/conformance/execution.json`](../tests/conformance/execution.json). Feature
checks assert admitted outcomes in raw, validated and prepared value modes against
legacy and upstream expectations. Optional JS differential tests also check complete
byte output. See the [decision and measurements](../benchmarks/execution/README.md)
and [reproduction](../benchmarks/execution/reproduce.md).
