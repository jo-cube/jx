# Architecture

## Current execution

`source → small expression tree → validating selection or expression execution → result stream`

- `parse/lex.rs` and `parse.rs` own tokenization, precedence and grouping. Field
  names and encoded string literals are owned once; operators retain source offsets.
  Parser nesting and tree depth are capped at 128, including flat operator chains.
  `expression.rs` holds static paths, mapped steps with predicates, groups, scalar
  operations, builtin calls, array/object constructors, bindings, blocks, conditionals and lambdas. Plain paths retain
  their specialized representation. Array-led path focus propagates through groups
  once during parsing, so execution need not rediscover it for each candidate.
- A top-level path still uses `json/scan.rs` to capture object paths during full
  validation. The first array needing navigation retains its raw range and remaining
  fields. Last decoded duplicate keys win before traversal.
- `path.rs` maps fields through arrays and normalizes sequence cardinality. Each
  lookup retains at most one pending value. A sole final raw array is preserved;
  combined results flatten one level. Leading `$` matters for root-array mapping.
- Scalar trees validate the record first, then execute directly in `evaluate.rs`.
  Path operands use borrowed field/element cursors over validated input. Known
  missing/single-value selections return directly; array navigation and retained
  sequences normalize through a stream. A path stream is inspected up to its second
  item to distinguish missing, singleton and multiple;
  multiple items remain a replayable stream, not an eagerly collected vector.
  Filtered operands are inspected completely before scalar use to preserve errors.
- `sequence.rs` composes mapped steps and `filter.rs` applies predicates using the
  same `Node::run` and scalar operands. Stack-borrowed stage views retain array versus
  sequence shape across chained predicates; groups normalize at expression boundaries.
  Candidate context explicitly distinguishes top-level array wrapping from local
  mapping. Numeric filters count only when a negative index needs the length.
  Each live stage retains that length across replays, avoiding recursive recounts.
- `aggregate.rs` folds `$count`, `$sum`, `$min` and `$max` over the same deferred
  path/filter streams. `Node::stream` exposes them without scalar cardinality
  preflight; other argument expressions use `Node::run`. One pending value resolves
  missing/singleton/multiple shape before interpreting a sole raw array as the
  argument array. The fold walks its argument stream once without collecting it;
  source-stage normalization and negative positions can still replay.
  Numeric type errors are retained until argument evaluation succeeds, preserving
  upstream error precedence. Counts and numeric accumulators are primitives.
- `retain.rs` is the shared retention boundary for constructors and lexical values.
  Already retained values pass through without copying. `construct.rs` consumes
  member streams directly, collecting sequences only when stored in a container. Arrays distinguish ordinary
  arrays, path-preserved arrays and retained sequences. Direct nested array syntax
  retains its result; other array members append the normalized result one level.
  Objects finish evaluating keys before values, compare decoded UTF-16 keys, omit
  missing members and reject duplicate keys from different member expressions.
  Local array contexts group matching keys before evaluating each group's value.
  Key groups use a linear search, suitable for small constructors; wide dynamic
  grouping has quadratic key-comparison cost and is not yet specialized.
- `container.rs` owns immutable array/object member lists behind `Rc`. Leaves are
  existing `Value` items: raw input, compiled strings, primitives or nested containers.
  Cloning containers shares structure rather than copying leaves or serializing them.
  The same path, filter, comparison and aggregate code accepts raw and constructed
  values; no second evaluator or serialization/reparse boundary exists.
- Numbers use binary64, booleans/null are primitives, and strings retain validated
  JSON encodings. Escapes and ordering compare as UTF-16 units without allocating
  decoded strings. Raw paths preserve all original number/string tokens.
- `compare.rs` handles typed ordering and structural equality. Sequence-to-array
  equality streams both sides. Comparing two push streams retains one side's borrowed
  values. Object equality uses a temporary map of borrowed left members and applies
  last-key-wins on both sides. These allocations are confined to equality.
- `Value` is the small public output union: raw input, primitive scalars or a
  compiled string literal, constructed container, opaque function, or undefined inside
  multi-item sequences. Containers retain their internal sequence/array shape. Its
  two lifetimes distinguish input from expression storage. `as_raw()` extracts input
  slices that can outlive the expression. Values are `Clone`, no longer `Copy`;
  constructed results use single-threaded shared ownership. Compiled expressions
  remain shareable between independent evaluator threads.
- `Expression` is immutable/shareable. `evaluate(&[u8])` validates JSON before returning.
  `for_each`/`try_for_each` return errors encountered during streamed evaluation;
  `ConsumeError` separates evaluation from consumer failure. Consumer errors stop
  immediately. Earlier items may be delivered before a mapped expression fails. Missing emits
  nothing, null emits once, and arrays remain values.
  The CLI owns NDJSON, limits, reused buffers and synchronous I/O, one line per item.

## Lexical evaluation

`analysis.rs` marks expressions and path stages that read/change lexical state or
create/call closures. It lowers direct builtin calls only when no binding or parameter
in the expression can shadow their name. Pure expressions allocate no scope arena;
static aggregates retain their streaming folds. There is no per-record name analysis.

`runtime.rs` owns an evaluation-local arena of parent-linked frames with small linear
binding lists. Bindings store missing/values/retained sequences, never replayable streams.
Blocks and calls create frames; assignment replaces a binding in the current frame.
Closures hold their body, captured current value/wrapping, and frame index. Captures
observe subsequent rebinding in that frame, supporting forward and recursive references.
Frames own values, but closures do not own the arena: recursion creates no `Rc` cycle.
Uncaptured terminal frames are popped; captured frames and their ancestors live until
that record's evaluation ends. Creating many closures can retain many frames per record.

`Context` carries an optional scope alongside current value and wrapping. `$$` is
initialized in the root frame; mapped items change `$` while preserving that scope.
Calls evaluate the callee and arguments in caller order, retain arguments once, bind
parameters in a child of the captured frame, and run with the captured current value.
Results are retained before leaving a call/block. Missing parameters are undefined;
extra arguments are evaluated then ignored by lambdas without signatures. Native calls check arity.

Stateful stages are retained before downstream traversal can replay them for cardinality,
negative indexes or error precedence. Pure stages still use existing stack-borrowed views.
This conservative boundary favors correctness over streaming lexical pipelines. It does
not turn ordinary paths or filters into collections. Retained output can still be cancelled,
but lexical evaluation may finish before the first callback. Input validation remains first.

`builtin.rs` registers four aggregates plus `$boolean`, `$not`, and `$exists`. Builtins are
first-class, shadowable values; unimplemented standard names are recognizable functions
whose calls fail explicitly. `function.rs` handles closures and dynamic calls. Functions
have identity equality, false effective-boolean value, and no host JSON serialization or
public host invocation API. Escaped JSON results retain their ordinary borrowing lifetimes;
opaque function results cannot be invoked after their evaluation's arena is gone.

Calls are bounded by 64 active invocations and 512 accumulated body-tree levels, in
addition to the parser's 128-level limit. Tail-call elimination and function signatures
remain deferred. Direct evaluation still expresses scope/control flow cleanly; an IR
would not remove frame retention or traversal costs and is not justified by this milestone.

## Decisions and measured limits

The tree exists because precedence, short-circuiting and typed operators now need
structure. Constructors add owned containers only when they are requested. There is
no execution IR, JIT or input DOM. Function execution uses the same tree and values.
Only unary minus on numeric literals is folded: JSONata treats literal and computed
positions differently. Groups must survive parsing for the same reason. No other
constant folding or field-demand fusion yet: neither is needed for correctness,
and the benchmark suite now exposes their potential value.

The custom safe scanner combines validation and selective capture for standalone
paths. Element/member cursors reuse its grammar instead of adding a trusted second
parser. Validation covers every byte, including unselected fields, before any output.
JSON container depth is capped at 128; all production Rust forbids unsafe.

Callbacks keep traversal state on the bounded native stack. Suspending two callbacks
for ordered equality would require iterator machinery; retaining one side only in
that operation is simpler. Object equality initially rescanned objects per key;
measuring 128-field objects justified a borrowed-member map confined to that case.
Neither choice imposes allocation on ordinary paths, filters, aggregates or scalar
operators without lexical features or construction. Routes, predicates and recursive lookups borrow
scoped contexts and values; stage views borrow operands. A predicate owns its candidate
once and moves it to output on a boolean match. Values clone where a deferred stream,
retained member or repeated positional match actually needs ownership.

Filters fuse with navigation and later filters. If a downstream stage fails, earlier
stages finish checking for errors; an error can replay its input to preserve upstream
error precedence. Successful forward traversal needs no preflight evaluation pass.
Raw arrays, explicit stage sequences and normalized operands are separate states;
collapsing them would break chained positions and last-step array preservation.

Array traversal can rescan a subtree at each nesting/path level: worst-case
O(bytes × input depth). Scalar operands also rescan demanded paths separately after
validation. The benchmarks retain deep arrays, tiny records, multi-field scalars
and structural equality so these costs remain visible. Add capture fusion, indexes,
trusted skipping or specialization only for a measured benefit with a simple design.

## Consolidation review

Keep direct tree evaluation. Parsing already resolves precedence, static paths and
array focus; execution separates scalar operands, replayable streams and retained
containers. Those distinctions encode observable shape and error behavior. Flattening
them into one owned result or one generic iterator would reintroduce collection or
move complexity elsewhere. Modules remain small and cohesive; no new layer is needed.

Profiles locate the current costs in scanning and the scalar/stream ownership boundary,
not instruction decoding. Returning known selections directly removes a redundant
stream walk; borrowing removes temporary ownership within live stages. Neither needs
an IR. An IR should next earn its place through concrete field-demand fusion or
function/control-flow requirements, with a measured benefit over this tree. It would
not by itself eliminate subtree rescanning, negative-position replays or member-wise
constructor evaluation. Those costs remain explicit; no cache or scanner index was added.

## Milestones

1. **Complete:** identity/object paths, UTF-8 JSON validation, borrowed evaluation,
   bounded NDJSON CLI, pinned conformance and initial benchmarks.
2. **Complete:** array navigation, normalized sequences, raw-array preservation,
   borrowed callbacks and flattening benchmarks.
3. **Complete:** scalar literals, arithmetic, equality/ordering, boolean logic,
   unary minus and grouping; primitive output, explicit runtime errors, scalar
   differential/conformance tests and performance evidence.
4. **Complete:** predicate and positional filters, candidate context, grouped
   paths, stage-preserving sequences, fallible streams and filter benchmarks.
5. **Complete:** direct `$count`/`$sum`/`$min`/`$max` calls, streaming folds,
   cardinality/type/error rules, upstream cases and allocation/traversal benchmarks.
   Later extended to first-class function references and dynamic calls in milestone 8.
6. **Complete:** array/object constructors, computed keys and values, nested output,
   borrowed leaves, retained sequence shape, composed navigation/filtering/aggregates,
   differential coverage and allocation benchmarks. Postfix grouping and `expr[]`
   remain deferred; see CONFORMANCE for reference implementation boundary policies.
7. **Complete: architecture/performance consolidation.** Scalar paths consume known
   selections directly; scoped traversal borrows instead of copying ownership through
   each stage. No language or public API change. See PERFORMANCE for repeated results.
8. **Complete: lexical/function runtime.** Bindings, blocks, root context, conditionals,
   lambdas, captured environments, dynamic/higher-order calls and minimal builtins.
   Evaluated retention and effect-aware stages preserve single execution.
9. **Next: sequence/path completion and function library.** Singleton retention,
   ranges and common higher-order builtins can build on this runtime. Tail calls and
   host invocation need explicit lifetime/resource contracts before broader embedding.
   Introduce IR only for a demonstrated semantic or measured execution benefit.

Each milestone updates conformance, tests and representative benchmarks. Full
language support does not require every expression to use the same execution path.
