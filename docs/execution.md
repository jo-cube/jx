# Execution foundation

The `experimental` feature exposes `jx::execution` and `compile_execution` (also on
`CompileOptions`). The latter returns the ordinary `Expression` facade backed by
the common IR. Default `compile` and the CLI retain legacy routing. Selection is
explicit at compilation; unsupported semantics never cause a tree retry.

## Compile, prepare and consume

`CompileOptions::compile_program` shares parsing, provenance, binding analysis and
constant preparation, then lowers an owned register program. Constant preparation
can still evaluate AST nodes during compilation. It is not a runtime fallback.
Unsupported instructions fail compilation. Inherited host/coercion boundaries remain
explicit runtime errors; admission alone does not establish API compatibility.

Programs are immutable, cloneable and `Send + Sync`. Clones copy code/operand arrays
and share immutable constants, schemas and acquisition tokens. Evaluation state is
independent. Prefer `Arc<Program>` when copying code is unnecessary.

Each public program stores an immutable input template compiled once from its IR.
Ordinary byte evaluation validates every input byte and captures demanded spans in
one traversal. Validated evaluation acquires token boundaries without repeating
grammar validation. Both automatically use the same acquisition as prepared input;
`evaluate_scan(raw)` is a separately named diagnostic control.

`InputTemplate::new` unions demands and read bindings from multiple programs.
Templates are immutable, cloneable and `Send + Sync`. `Workspace` contains only
query-sized offsets: eight inline pairs plus a reusable heap spill. It retains no
JSON bytes, values, environments or program pointers. `retained_bytes` reports heap
capacity; `clear` releases it. Capacity follows the widest query used, not record
size or array cardinality. Concurrent evaluations need separate mutable workspaces;
programs/templates may be shared freely. No global or thread-local cache is used.

Ordinary calls use a temporary workspace. `evaluate_with_workspace` and
`evaluate_validated_with_workspace` on `Expression` and `Program` reuse caller-owned
storage with the same settings and result lifetimes. A returned `Evaluation` never
borrows that workspace. `Acquisition` remains a convenience bundle of template and
workspace. A one-program template reuses the program's precompiled metadata.
Preparations replace old spans, including duplicate-key subtrees. Invalid input
clears partial offsets before returning. There is no mandatory input DOM or index.

The existing `InputPlan::prepare(&self)` and indexed `PreparedInput` APIs also support
IR expressions. Each preparation owns independent offsets, so multiple preparations
and results may coexist. IR demands have no fixed capture limit. A mixed IR/legacy
plan validates once and uses a separate validated capture traversal for legacy
entries; it never evaluates an IR expression through legacy. For allocation reuse
across multiple programs, use their shared template and a worker-local workspace.

`evaluate(prepared)`, `evaluate_bytes(bytes)`, `evaluate_validated(raw)` and
`evaluate_without_input()` return the existing `Evaluation` interface. Absent
focus/root are missing, distinct from JSON null.

Terminal borrowed fields with an infallible reference-only prefix resolve directly
through the ordinary field kernel to a value or a lazy path cursor. Neither needs an
escaping activation or span snapshot. Controlled and general sequence producers
retain a one-shot continuation owning query-sized span copies and shared metadata.
Preparations can be reused or dropped while results remain live; input/code leaves
and capture identities survive these ownership boundaries. Eager lexical and grouping
boundaries still complete before returning results; parentheses are not universally
interchangeable with terminal streaming. Deferred root delivery can expose a terminal
producer while its caller remains cancellable. It does not authorize crossing inner
eager barriers. Complete serialization has a separate consumption contract.

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
charges. They are cooperative budgets, not a process memory quota.
`try_for_each_with` and `write_json_with` accept the same settings for prepared input.
Their settings-aware evaluation preserves the reference's eager effect boundaries.
IR-proven total, read-only contextual producers can defer without retaining a result
graph when controls are absent. Unknown/fallible operations and controlled execution
keep their existing delivery boundary;
empty/seeded options preserve ordinary delivery. Complete serialization shares one
rollback/error-order kernel with `write_json`.

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
but unbound builtin name. `$eval` resolves its lexical target before arguments and honors host shadowing.
Builtin dynamic source uses the semantic parser and lowers into this same IR. The
scope lifetime bridge retains escaping compiled definitions, matchers and transforms;
it does not execute a borrowed AST. Non-tail `$eval` call sites with literal or
compile-time immutable string sources own shared compiled IR code (or a lazy syntax
error). Arguments still execute in order; the resolved target must be the actual
builtin before prepared code is used. Code shares the caller's lexical arena and
retains no evaluated results or environments. Input-derived sources, indirect calls
and genuine tail transitions retain dynamic compilation and the lifetime bridge.
There is no source-keyed/global cache. Unsupported dynamic lowering remains explicit.

Immutable invocation regions expose their incoming/replacement focus and root demands
to the existing acquisition and read-binding analysis. Their code is already physically
prepared and is not inlined or rewritten as an ordinary child region. Invocation
forwards the active input view through the same region/call runtime used by functions.
Byte-identity and base guards keep external focus, rebinding and separate objects
distinct. Unknown input origins navigate normally; acquisition hints never authorize
execution before target resolution, argument evaluation or lazy-error checks.

Straight-line locals and proven immediate callbacks can retain SSA ports. A source
scope analysis selects retained cells for escaping/callable values and enclosing
writes. This is conservative promotion, not a complete closure conversion or liveness
analysis. Proven locals and parameters keep register ports even inside retained scopes.
Read-only branches preserve those ports; call-effect summaries invalidate ports when
a target can write an older lexical frame. Conditional aliases join invocation effects
in one forward dataflow scan, so repeated read-only calls retain their lexical ports
without recursively expanding alias diamonds. Root rebinding and escaping captures still
use live cells. Immediate callbacks enforce the same call/stack/tail quotas as lexical
calls, with inline depth state when no frame is needed.

Definitions carry deduplicated root, definition-focus, parameter and free-name demands, and at
most one proven return origin. Statically resolved calls substitute these summaries
into the common acquisition analysis; they do not inline or duplicate bodies.
A conditional callee joins demanded paths, with an unknown arm invalidating the proof.
Summaries are shared immutable metadata; joins copy and deduplicate facts instead of
retaining combinations of call targets. A result origin survives only when all arms
agree. Nested IR regions carry value origins and resolved callees together, so
constructor and argument boundaries preserve the same demand facts. Lexical bindings
and resolved function aliases propagate acquisition hints, including returned captures. These definition-time hints never substitute for live reads: rebinding
and shadowing still resolve the actual arena value, and byte-identity guards prevent
using another object's spans. Unresolved/changed targets can lose hints without changing
execution. Direct IR calls forward prepared input through the shared ABI and restore borrowed spans by
byte identity. Unknown returned values and lexical reads can use the existing local
acquisition operation when independent fields are co-used. Already acquired values
reuse a covering schema. Dynamic calls retain their normal checks and effects.

A row capture identifies its active Prepared view, separately from the immutable
schema used for read-layout hints. Nested activations can reuse one schema without
confusing their rows. Restoration searches active views by borrowed byte identity;
no token is dereferenced. Temporary row spans never escape by reference. Demanded ancestors and explicit local
acquisition can promote into `Value::InputView`, an experimental opaque borrowed input
handle. It owns query-sized spans, shares the immutable schema, and preserves byte
identity through bindings, arguments, closures, dynamic regions and returned values.
A covered preparation copies only demanded spans; otherwise acquisition scans that
object once. Ordinary cells remain compact; expressions without these demands do
not allocate input views. `Value::as_raw`, access, signatures, transforms and owned
conversion treat a view as the original input. Array elements without static spans
still navigate normally; no full-document index or ancestry graph is built.

Matchers use the existing function ABI and stateful continuation kernels. Literal
execution creates fresh state per evaluation; compiled pictures own no executable
AST. Prepared-format argument shifting and picture-error timing share the reference
value kernel. Clock-using programs initialize evaluation-fixed time at entry, and
seeded random settings are honored by byte, validated and prepared evaluation.

Transforms own compiled selection/update/delete programs. A small body interface
lets both backends use the same copy identity, membership, merge, deletion and
ordered rewrite kernels. Captured frames and owned dynamic transform definitions
use the existing lexical arena and lifetime bridge. Dynamic roots and transform
parts share a non-tail region adapter that retires discarded closure environments
before the next invocation; returned captures and older-frame writes remain live.
No independent transform
interpreter is introduced. Generic calls, callback parameters and compositions may
resolve to `$eval` or a transform that writes the caller's frame; one conservative
call-effect fact drives both port invalidation and region write summaries. Read-only
definition bodies retain their register ports. Unknown calls inside a known function
remain in its conservative write summary, including calls to captured transforms.

Generic higher-order adapters, partial application and composition use the value ABI.
Owned borrowed-input metadata survives that ABI. Plain raw values and activation-local
row tokens are still different lifetime choices: arbitrary captured focus/argument values
are not all promoted automatically. A missing static demand or a view ending before
promotion can therefore lose indexing, but cannot change the value or trigger replay
into another evaluator. Scope selection remains conservative; retained enclosing scopes
do not discard proven immediate callback register ports.

## Compiler and physical execution

The semantic program represents focus/root, arguments/captures, live lexical reads
and writes, scopes, functions/calls, ordered operations, branches, constructors,
paths, aggregation and materialization. Registers have no fixed eligibility bound.
Joins can write multiple definitions; this is structured register dataflow, not
strict SSA. Acquisition analysis restores lexical scopes with a journal of changed
bindings instead of copying every live lexical/path map at each nested body. This
bounds scope restoration work by actual mutations; path construction and schema
binding still have their own compilation costs. Identity-producing values are not commoned. Unknown builtin-name reads
also retain fresh function identity when lexical lookup falls back to a builtin.

Native call results retain their sequence shape until the consumer's normalization
boundary. Postfix array retention is explicit. `Collect` consumes a sequence producer
before missing/singleton normalization when candidate positions must be preserved;
it reuses the ordinary route/selection/context kernels. It is an eager cardinality
boundary, not a new execution strategy. Contextual builtin tail calls enter the same
tail ABI as general calls, evaluating arguments in definition focus and using caller
focus for context substitution. Retained field lookup observes matches
before ordinary path flattening; keeping an already normalized value would lose
nested-array cardinality. General routes compile each step and predicate into the same IR. Stack-scoped
producer views share the reference engine's mapping, literal-position, candidate
selection and error-order kernels. Arbitrarily composed non-tuple routes, array
focus, later numeric predicates, ranges, wildcards and descendants can therefore
execute without an AST body or another evaluator. Negative positions obtain length
lazily. Lowering tracks known wrapping at root, stage, constructor and closure
entries. Collapsing a route source requires preserving that focus/iteration fact;
unknown dynamic focus executes the general IR route. General stages acquire their
requested row schema, including when a compact source collapse is not valid.
Predicates over retained operands acquire fields in the element scan and preserve
earlier predicate failures ahead of downstream failures; consumer cancellation
still propagates immediately. Replay-safe views may revisit preceding pure work for cardinality/error
precedence; effectful stages retain borrowed candidates before downstream replay.
Consumer cancellation propagates immediately through the shared view contract.

Correlated paths are explicit `Correlate` / `CorrelatedGroup` IR regions. Their
structural operands describe stages, lexical boundaries, filtering and ordering;
every executable body is an ordinary compiled program. A borrowed semantic view lets
AST and IR operands use the same tuple, grouping and ordering kernels during migration.
There is no AST body or reference retry inside a compiled context region.

Rows carry a value and copy-on-write bindings only in correlated regions. Explicit
focus/index bindings and statically demanded ancestry slots preserve their correlation
through nested stages. Indexes before and after filtering have distinct stage positions.
A focus binding preserves the incoming focus while binding the selected value. Parent
slots denote expression-path ancestry, not a parent pointer on generic JSON values.
Lexical blocks discard local names while preserving demanded ancestry; retained
closures use the same frame/reachability rules as ordinary calls.

Boolean and nonnegative literal filters stream. A predicate that can select from the
end retains correlated rows once, without replaying effects. Sort/group consumers
retain rows with their bindings; ordinary scalar/field values gain no metadata. Rows
borrow original input fragments, and each IR region reuses one register workspace.
The acquisition analysis follows generated immutable ancestry slots and route return
origins through the same demand collector as lexical/call ports. Body acquisition
reuses a covering root or active ancestor port by byte identity, exports language
values before reusing spans, and never builds an input graph. Demanded ancestor slots receive sparse parent schemas from this same collector.
Before a correlated stage exports its parent, it promotes that borrowed value when
needed. Descendant stages share this metadata even after the producing row view ends.
Dynamic field names without static demands still navigate borrowed bytes. Uncorrelated path prefixes compile
once into ordinary IR rather than retaining duplicate contextual bodies.

An eager selection is also a cardinality boundary: its sole selected array becomes
the following mapper's input sequence. Fusion must preserve this rule. Complete consumers can pass a replay-safe IR producer
through a cardinality-aware boundary view. It holds one candidate until a second item
settles native-sequence cardinality; a sole array retains its distinct normalization.
The existing route/filter kernels defer downstream failures behind earlier-stage errors.
Externally cancellable delivery and effectful stages retain their eager boundaries.
This avoids an intermediate sequence graph without creating a separate fold evaluator;
separate predicate/mapper activations, numeric conversion and generic dispatch still
limit throughput. Callback and replay boundaries can still require materialization. Valid
array shapes execute normally rather than triggering an optimization-domain rejection.

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
functions or write lexical state, and preserves cardinality boundaries; delaying
errors alone cannot preserve effects or sequence normalization.
One structural child-program visitor serves preparation, capture traversal/remapping,
local-acquisition traversal and lexical-write summaries. One structured branch
lowerer owns lexical-version joins and dominance for conditional, default/coalescing
and short-circuit operations. Context-sensitive acquisition facts remain explicit.
Recursive preparation builds acquisition bindings, scalar layouts, load transfers
and delivery. Lexical-write facts are computed bottom-up from the IR, separately from
source effects that govern scheduling. Definition summaries have one proven origin,
never combinations of branch alternatives. This is not a general pass framework.

Two dispatch loops consume the same instructions and physical storage layout.
Conservative live intervals assign logical values to reusable cells, including
forward branch joins and synchronous child captures. Read layouts still name
logical IDs; physical slot reuse does not change acquisition identities. Scalar
and general activations use the same allocation analysis. Inline frames spill
dynamically without changing admission; no register-count threshold selects another
evaluator. Backedges would require CFG liveness before reusing their storage.

Complete, uncontrolled consumption can use a fused source/filter/map region when
its IR bodies fit the scalar kernel, its single predicate is Boolean-like, and its
mapper cannot produce an array. A derived row program shares field reads, scalar
work and acquisition across the predicate and mapper. It has a fresh acquisition
identity and reuses the common instruction, slot and checked-value machinery.
The region stores a locator for its original source and retains only the fused row
code. It does not recognize expression text or an aggregate builtin.

Array/undefined candidates whose cardinality is ambiguous remain borrowed until
that boundary is resolved. Predicate failures precede deferred mapper failures,
which precede downstream failures. Cancellable/controlled delivery, array root
focus, retained scopes, captured arguments, multiple predicates and incompatible
bodies use their existing semantic operations. Strategy selection happens before
language execution; no guard retries completed expressions. This bounded physical
strategy does not yet fuse general callbacks or replace all sequence operations.

Root/row captures, owned indexed aliases and raw navigation are input strategies.
Failed read guards navigate the same value once. Separate origin analyses,
`Fold`/`Reduce`, and immediate `Each` versus generic higher-order value kernels remain
migration debt. General function calls do not yet share all acquisition/fusion benefits
of immediate callbacks.

## Dynamic coverage and boundaries

Constant preparation can remove unsupported syntax. A constant-only example does not
establish dynamic support.

| Area | Available | Remaining boundaries |
| --- | --- | --- |
| Values/operators | Borrowed JSON, missing/null, scalars, arrays, static and dynamic objects, grouping, ranges, scalar operators, membership, postfix retention | Inherited host/coercion boundaries |
| Navigation | Static fields, general routes and predicates, positions, focus/index tuples, ancestry, tuple grouping/order, array focus, wildcard/descendants, conditional acquisition | General producer/consumer call boundaries |
| Scope/calls | Lexical blocks/writes, live/escaping closures, recursion/tail calls, signatures, general arity, first-class builtins, partials/composition, dynamic IR with retained definitions | General producer/consumer call results; indirect and tail dynamic-code reuse |
| Library | Lookup, truth/existence, aggregates, common string/numeric/collection kernels, map/filter/reduce/each/sift/single and sort callbacks, error/assert, matcher values, formatted numbers/dates, eval, transform/clone, random/time | Inherited regex/coercion and formatting boundaries in `compatibility.md` |
| Embedding | Compile/clone/reuse, immutable/runtime bindings, host calls/reentry, external focus, absent/validated/shared input, controlled borrowed results and terminal settings | Escaping-result allocation, conservative settings delivery, interpreter-only native requests |

The pinned legacy-success inventory is admitted, but this is not a proof of complete
language or API parity. Unsupported syntax in an unselected branch is rejected at compilation. A runtime shape/callee-domain rejection
can follow preceding streamed output. Neither is a hidden compatibility fallback.

The full pinned corpus has an explicit admission entry in
[`tests/conformance/execution.json`](../tests/conformance/execution.json). Feature
checks assert admitted outcomes in raw, validated and prepared value modes against
legacy and upstream expectations. Optional JS differential tests also check complete
byte output. See the [decision and measurements](../benchmarks/execution/README.md)
and [reproduction](../benchmarks/execution/reproduce.md).

## Default migration boundary

The opt-in `compile_execution` facade routes byte, validated, absent-input, random,
settings, workspace and shared-preparation evaluation through IR. `Expression::clone`
shares its immutable program. The semantic AST remains stored for compilation and
reference infrastructure; it is not a runtime fallback.

Under `jit`, `enable_native()` on an IR expression returns zero kernels, zero code
bytes and zero failures. It does not traverse legacy plans or change execution.
Repeated calls are harmless; clones share the same interpreted code. Default legacy
expressions retain their existing native installation behavior. Native acceleration
is optional and is not a prerequisite for an IR default.

Before default adoption, keep public contract replays, validation/error phase and
source checks, shared-input ownership, cancellation/prefix tests, concurrent reuse
and actual-worker controls mandatory. Work quotas count executed IR work rather
than AST instructions; retained builtin arguments and sequence consumption still
charge their item/work budgets. Indirect/tail `$eval` recompilation and conservative
callback retention remain workload caveats. CLI/default compilation
have not migrated. No legacy subsystem can yet be removed wholesale.

The pinned admission inventory covers every legacy-success outcome. The remaining
41 non-admitted cases are compilation errors in both inventories, not missing
successful language features. Generated checks distinguish immutable-transform alias
snapshots, upstream empty-tuple grouping exceptions and inherited regex coercion
rejections. Those existing boundaries are documented in `compatibility.md`; they
are not new IR regressions and do not require recreating JavaScript mutability.

General callbacks, lexical sequence values, sort/group and escaping closures can
retain evaluated values. Their general path is correct on the retained conformance
and embedding suites; scalar-only physical fusion is an optimization coverage
boundary. Materialization required for replay, identity, lexical lifetime or effect
ordering must remain. Avoidable callback result graphs and per-invocation dynamic
compilation are follow-up efficiency work, not reasons to retain a second evaluator.

After facade, CLI and shared-input parity, retire the legacy bounded `plan` family,
`acquire::Region` / `Acquired`, `function::acquire`, and their guard/retry routing.
Remove `InputPlan`'s remaining legacy capture storage; its IR entries already use
shared acquisition. Retire AST-backed callable and
stream adapters only after their reference callers and compile-time uses are gone.
`compile::prepare` still uses AST constant evaluation; removing the complete tree
implementation requires replacing that dependency as a separate step.

Retain the parser, semantic provenance/binding preparation, validator/scanner,
borrowed public values, lookup and scalar kernels, lexical arena, controls,
signature/callback ABI, sequence/cardinality, tuple/group/order and transform kernels.
Files containing both shared kernels and AST adapters cannot be deleted wholesale.
The legacy native adapter may retire with bounded plans; preserve the auditable
`jx-native` executable boundary only if an IR-native backend is later justified.
No native parity project or additional execution framework is a migration prerequisite.
