# Architecture

## Current execution

`source → expression tree → demanded ancestry → effect analysis and specialization → bounded region lowering → validating selection/capture → execution → result stream`

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
- `route.rs` composes mapped steps over `sequence.rs` scoped views and `filter.rs` applies predicates using the
  same `Node::run` and scalar operands. Stack-borrowed stage views retain array versus
  sequence shape across chained predicates; groups normalize at expression boundaries.
  Candidate context explicitly distinguishes top-level array wrapping from local
  mapping. Numeric filters count only when a negative index needs the length.
  Each live stage retains that length across replays, avoiding recursive recounts.
- `aggregate.rs` folds `$count`, `$sum`, `$min`, `$max` and `$average` over the same deferred
  path/filter streams. `Node::stream` exposes them without scalar cardinality
  preflight; other argument expressions use `Node::run`. One pending value resolves
  missing/singleton/multiple shape before interpreting a sole raw array as the
  argument array. The fold walks its argument stream once without collecting it;
  source-stage normalization and negative positions can still replay.
  Numeric type errors are retained until argument evaluation succeeds, preserving
  upstream error precedence. Counts and numeric accumulators are primitives. Exhaustive
  array folds dispatch on raw/dynamic/compiled storage once through the iterator’s
  `for_each`, avoiding a representation branch for each item.
- `retain.rs` is the shared retention boundary for constructors and lexical values.
  Already retained values pass through without copying. `construct.rs` consumes
  member streams directly, collecting sequences only when stored in a container. Arrays have four explicit shapes: ordinary
  arrays, path-preserved arrays, normalized sequences and kept sequences (`[]`). Direct nested array syntax
  retains its result; other array members append the normalized result one level.
  Objects finish evaluating keys before values, compare decoded UTF-16 keys, omit
  missing members and reject duplicate keys from different member expressions.
  Local array contexts group matching keys before evaluating each group's value.
  Key groups stay linear below 32 distinct keys, then build a construction-local
  hash index comparing decoded UTF-16 units. First-seen groups and reference integer-key
  ordering are preserved; the index is dropped before group values are evaluated.
- `navigate.rs` adds wildcard/descendant traversal, range emission and singleton
  retention. Kept sequences are recognized before scalar normalization; they emit
  as one array but flatten as sequences during mapping. Parser annotations preserve
  both the marked stage and the complete path boundary. Pure wildcard/descendant
  streams stop cardinality lookahead after two items, as static paths already do.
- `members.rs` enumerates one object's borrowed members with last-key-wins and
  reference integer-key ordering. Descendants stream depth-first; an array-valued
  wildcard member requires an array result and therefore retention. Neither builds
  a whole input tree. Ranges emit primitive numbers into the array constructor;
  the explicit output array still materializes, including when passed to an aggregate.
- Postfix grouping feeds the existing constructor groups directly, without a second
  candidate collection. `ordering.rs` retains candidates and stable merge-sort indices;
  comparator expressions use the normal context/evaluator and may fail or have lexical
  effects. For more than two candidates, replay-safe keys are retained lazily on their
  first comparison, including primitive numeric decoding. Secondary terms remain
  short-circuited; effectful terms and tuple comparison scopes preserve reference order.
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
  compiled or owned string, dynamic or compiled container, immutable clone view, opaque function, or undefined inside
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
in the expression can shadow their name. Compilation separates replay safety from runtime-storage demand. Expressions that
need no scope, call state or clock allocate no scope arena;
static aggregates retain their streaming folds. There is no per-record name analysis.

`runtime.rs` owns an evaluation-local arena of parent-linked frames with small linear
binding lists. Tuple frames share immutable row bindings; assignment copies them only
when needed. Ordinary lexical frames still own their lists. Bindings store missing/values/retained sequences, never replayable streams.
Blocks and calls create frames; assignment replaces a binding in the current frame.
Closures hold their body, captured current value/wrapping, and frame index. Captures
observe subsequent rebinding in that frame, supporting forward and recursive references.
Frames own values, but closures do not own the arena: recursion creates no `Rc` cycle.
An uncaptured terminal slot and its binding capacity are recycled. Immediately invoked
literal callees borrow their definition and caller frame without allocating a function
value or marking that frame captured; escaped body closures still capture their own
ancestry. Captured frames and their ancestors live until that record's evaluation ends;
creating many escaping closures can retain many frames per record.

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

`builtin.rs` resolves static names and keeps streaming aggregates/boolean helpers and
indexed lookup on their existing paths. `builtin/library.rs` describes fixed builtin
parameters, optional context substitution, arity and constant-fold eligibility;
implementations live in cohesive string, collection, higher-order, numeric, padding,
array, encoding and diagnostic modules. There is no
runtime registry. User signatures have a separate compiled parameter matcher. Direct library calls use three
stack argument slots; `$replace` uses four and variadic `$zip` retains its argument list. Dynamic calls and callbacks share `function::invoke`.

Computed strings use `OwnedString`: shared immutable JSON encoding, including lone
UTF-16 surrogates. Raw strings and literals keep their borrowing; comparisons, keys,
serialization and constant capture use the same encoded-string access as before.
`$length` counts codepoints without copying; transforming strings owns new storage.
UTF-16 iterators expose bounded size hints; computed strings reserve their lower bound.
Type names borrow static literals. No input record becomes an owned tree.
Rounding shifts shortest decimal text in a stack buffer; padding counts/streams
codepoints. Sort reuses `ordering::indices`, zip uses element cursors, and single
retains only its selected candidate. Library arguments still finish before callbacks;
sequences needed as arguments are retained, while raw arrays stay borrowed. URI
conversion implements strict percent/UTF-8 rules; `base64` supplies binary codecs.
Only failing user diagnostics own messages (`Cow<str>`); success adds no ownership
state. Scalar operators have a separate cohesive evaluator method; this and lexical
parser methods keep recursive frames below the existing
parser/function guards' stack budget. Scanner diagnostics stay static and copyable;
conversion to the public owned-message error happens only at validation boundaries.
The JSON validation grammar and execution plan are unchanged.

Higher-order functions evaluate arguments once before invoking callbacks. Raw arrays
stay borrowed; result sequences are retained, and callbacks receive the original
array/object only when their arity requests it. `$reduce` retains its accumulator,
not a second mapped collection. Map/filter/each finish their output before exposing
it; cancellation then stops output consumption. Existing path predicates and numeric
folds still stream, including `$average` through eligible execution plans.

Calls normalize native sequence results at the expression boundary, after postfix
`[]` can retain them. Lambda tail calls preserve native sequence shape for their
caller, matching observable nesting in higher-order results. Analysis marks these
call sites; `function/tail.rs` executes tail calls through a loop. Folding preserves
call boundaries. Builtins remain first-class and shadowable; unimplemented standard names
fail explicitly when called. Opaque functions have no JSON encoding or public host
invocation API. Escaped JSON values keep their ordinary borrowing lifetimes.

Non-tail calls remain bounded by 64 active invocations and 512 accumulated body-tree
levels, alongside the parser's 128-level limit. A tail chain has a separate deterministic
one-million-iteration budget (`EvaluationLimit`), so infinite calls do not run forever.
Scope and function control flow stay in tree execution; numeric lowering is unchanged.

Function definitions hold formal parameters, body, optional compiled signature and a
tail-control flag. Closures borrow that definition and retain only focus/frame metadata.
`function/signature.rs` validates masks, optional/variadic matching and shallow array
subtypes once per invocation; pictures and signatures are never parsed per record.
Type checks borrow value tags, including validated input tokens; they do not decode
numbers just to classify them. Fixed signatures need no matching program scratch. Ambiguous signatures use bounded
suffix reachability, with stack scratch for small calls and heap scratch only for wide
ones. Retained argument slots use the stack through three values, then spill.

Only tail positions descend through blocks/conditionals to a pending call. Tests,
arguments and terminal values use the existing evaluator. Arguments finish before the
old frame is reset; captured frames cannot reset. Partial and composed callable targets
are unwrapped by the same loop. Signatures validate against invocation focus while the
body/arguments retain lexical focus; native contextual tails share builtin evaluation
with a distinct caller focus. Context-free native/scalar bodies use ordinary calls.
Released block slots and parameter capacity can be reused across tail iterations and
callbacks, without new state in `Value`, `Context`, non-lexical paths or execution plans.

### Dynamic evaluation and effects

`dynamic.rs` prepares constant direct `$eval` source with the normal compiler once, without
folding its execution. Dynamic source compiles per call. Dynamic programs leave
builtin names shadowable so they inherit the caller's bindings; enclosing programs
that can call `$eval` also conservatively retain builtin lookup. Replacement focus
changes `$`, not `$$` or the lexical frame. Native tail calls use invocation context,
including its environment, as the pinned reference does.

Static programs borrow their enclosing expression's storage. Truly dynamic programs
use `runtime/bridge.rs` for a shorter compilation lifetime: copy scope slots, share
input/constructed leaves, execute, then export changed frames and results. Only this
boundary owns dynamic binding names, literal bytes, container member lists and escaped
lambda/transform definitions. Unchanged imported values preserve their identities;
escaped matcher patterns retain compiled regex state. Ordinary `Value` lifetimes and
borrowed definitions do not change. Captures remain frame indices without arena back-edges.

`dynamic/borrow.rs` loans owned callable definitions for an entire invocation region,
so recursive references enter the existing tail loop. `dynamic/retention.rs` memoizes
aliases only while exporting that region. No per-record input cache or replayable
sequence becomes a stored value. Dynamic closure callbacks currently copy scope slots
per invocation; that containment has a measurable allocation cost.

`random.rs` is a shared single-threaded draw stream, seeded lazily on first use.
`evaluate_with_random` accepts an optional caller-owned deterministic stream. Nested
programs share it; errors preserve consumed draws. `$random` returns a binary64 value
in `[0,1)`. `$shuffle` uses inside-out Fisher–Yates and retains its explicit output
array with borrowed/shared leaves. Missing/zero/single inputs draw nothing. Both are
effectful and cannot fold or replay. Non-effect plans keep their existing instructions
and allocation behavior; runtime arenas add one optional random handle, not a universal
value/context field. No dependency or host-call registry is added.

### Conversion and composition

`convert.rs` implements JSONata stringification, independently of token-preserving
`write_compact`. It visits borrowed/constructed members, resolves duplicate/order rules,
canonicalizes escaping and numbers, and optionally indents. Strings pass through;
booleans/null/functions use static literals. `convert/number.rs` checks decimal/radix
text without allocating for unescaped input. Computed conversion/concatenation strings
use the existing immutable encoded storage. Concatenation joins validated string bodies
without a redundant decode/encode pass; it preserves UTF-16 units across the boundary.

`function/composition.rs` adds retained partial arguments and pairs of callable values
to the existing `FunctionKind`. Bound arguments evaluate once before target resolution;
missing and holes stay distinct. Repeated partial application merges bound slots once.
Invocation fills small argument lists on the stack and uses the existing lambda/builtin
call path. Closures keep their original focus/frame; there are no owning arena back-edges.
Composed functions normalize the intermediate result, then invoke the second function.
Lambda partials bypass their original signature, matching the reference. Native
count/zip partials retain their own length/array rules; other JavaScript coercions and
native default-parameter partials remain explicit boundaries in CONFORMANCE.

Analysis rewrites an unshadowed `lhs ~> $builtin(args)` to its ordinary builtin call
with `lhs` first. This preserves streaming folds, constant folding and existing plan
lowering, without function allocation or a lexical arena. Dynamic invocation and
function-value composition retain tree execution. No string/function plan instructions
or generic boxing are added to unrelated expressions. A root conversion of one static
path reuses the fully validating selector and retains its result once, avoiding the
separate validation and field scan. Other conversion calls use normal argument retention.

### Scoped path rows

`Kind::Tuples` marks paths carrying `@`/`#` bindings at compilation. `tuple.rs`
streams rows containing a value and shared, evaluated bindings; `tuple/stages.rs`
handles map/filter/index composition. Ordinary `Kind::Route`, `Context`, `Value`
and plan instructions do not carry tuple state. Scalars and planned leaves still
execute through `Node::run`; predicates, ordering comparisons and object grouping
share their existing semantic routines. This is path context propagation, not a
second expression evaluator or an input index.

Each row evaluation installs its bindings in a short-lived child of the existing
scope arena. Closures capture that frame through the same arena indices as other
lexical functions. Rows share immutable binding lists until a binding changes.
One-shot consumers (`Node::consume`, used by constructors and aggregates) can walk
scoped paths directly; scalar normalization and lexical storage retain evaluated
results. `Node::stream` still exposes only replay-safe expressions.
Top-level scoped paths stream through the callback API.

Map/boolean-filter/index stages need no complete row collection. Ambiguous numeric
predicates may need negative indexing, so they retain rows once instead of replaying
lexical effects. Sorting and grouping retain rows, preserving their bindings. Group
values merge current context and bindings together. The transition from a plain path
holds at most one pending item to settle array/sequence cardinality. No scanner
changes are needed: demand capture continues inside eligible planned expressions.

## Regex and matcher boundary

`matcher/` owns compiled ECMAScript patterns, per-evaluation cursors, retained
subjects and text consumers. `regress` 0.12 supplies parsing and matching, including
lookarounds/backreferences; only `std`/`utf16` features are enabled. Its only runtime
dependency is `memchr`. `Value` and plan instructions are unchanged.

A regex literal owns its immutable compiled pattern. Evaluating it creates a callable
matcher with a shared position cell; `next` functions retain that same cursor and their
own subject. Calling the matcher resets the cursor, including on another subject;
failed searches reset it to zero. Continuations observe this shared state, matching the
reference. Neither compiled expressions nor unrelated functions own matcher state.
ASCII subjects borrow encoded input/expression bodies. Other subjects decode once to
shared UTF-16 units, preserving lone surrogates, legacy code-unit matching and indexes.
Escaped pattern surrogate units are exposed individually at compile time because the
engine otherwise joins them. Legacy `/i` compatibility restrictions are in CONFORMANCE.

Native text consumers keep only one capture record at a time. `$contains` needs no
match object; replacement templates decode once and append capture ranges directly.
Callbacks and direct matcher calls create the ordinary object with `match/start/end/
groups/next`; custom matchers use the same invocation and object model. `$match` retains
its output sequence; split retains its output array. Output strings own new storage;
unchanged replacements and whole-source matches keep borrowing. Existing function
serialization rules still apply to matcher objects containing `next`.
There is no regex execution-plan lowering or per-record pattern compilation.

## Compile-time specialization

`compile.rs` runs bottom-up after scope/effect analysis. A conservative whitelist
identifies context-independent, effect-free scalar expressions, constructors and
implemented builtin calls with explicit arguments. The existing evaluator computes
successful constants; errors stay in the tree and retain runtime phase, source offsets,
short-circuiting and input-validation precedence. Paths, ranges, bindings, lambdas,
dynamic calls and effectful subtrees remain direct evaluation. This avoids both a
second semantic implementation and speculative evaluation of large implicit ranges.

`constant.rs` separates immutable compiled `Data` from per-record `Value`. Strings
and container contents borrow expression storage; scalars remain unboxed. Containers
keep their array/sequence shape and receive a fresh shared identity token per
construction. Descendants share the token but have distinct data addresses. This
preserves observable identity under `in`, repeated calls and retained bindings without
rebuilding members. Dynamic constructors use the same container access operations and
can contain compiled subtrees. Compiled expressions remain `Send + Sync`.

Static objects preserve member order and add a sorted index over encoded keys. Lookup
binary-searches decoded UTF-16 units without allocating decoded keys. `$lookup` uses
this index for compiled objects and the ordinary member traversal for dynamic/raw
objects. A direct call with a static object lowers to a small `StaticLookup` tree node:
primitive results need no temporary object identity. At the expression root, a plain
path key reuses validating path capture, avoiding a second scan of the record. Other
keys use the existing evaluator; numeric regions can embed the indexed lookup. Generic array lookup
retains its normalized result; it is not a second navigation engine.

Builtin references resolve once when no declaration anywhere in the expression can
shadow them. Ordinary references then need neither name lookup nor a lexical arena.
The conservative whole-expression shadowing rule is unchanged. Lexical constant
propagation is deferred: captured frames observe later rebinding. Repeated numeric
paths share loads within a lowered region (below). Nested object paths and static
lookup keys share demand capture; capture across independent regions remains deferred.
Static constructor grouping still has quadratic compile cost for distinct keys, paid
once rather than per record. No general IR, JIT, scanner cache or input DOM is needed
for these specializations.

## Execution plans

`plan/` lowers pure regions after constant folding. Programs still fit in 32 slots;
operands and forward branch targets use byte indexes. Instructions load paths or
indexed static lookups, create primitive constants, negate, perform numeric binary
operations, test effective boolean values, copy/merge results and jump. Scalar
regions need at least three operations. Calls, lexical effects, general sequences,
string operations and unsupported/oversized regions retain tree evaluation; their
eligible children can still lower. There is no general IR or JIT.

Numbers, booleans and missing stay in stack registers. Repeated path loads share a
slot only when that load dominates its use. Conditional joins restore the preceding
load set; skipped branches neither evaluate arithmetic nor trigger guards.

`json/demand.rs` stores a small tree of compiled path prefixes, with bit masks naming
at most 32 load destinations. A root plan captures borrowed spans while validating
the entire record. Matching nested prefixes share traversal; static lookup keys use
the same capture. A duplicate parent clears all its descendant destinations before
replacement, including missing fields. Intermediate arrays mark affected loads for
tree fallback, preserving JSONata normalization. No input index or record cache exists.

Captured spans and primitive registers are separate stack storage. Number conversion,
lookup and type guards run only when the corresponding instruction executes; untaken
branches still incur key matching/span capture, but no conversion. Plans inside tree
execution use the same scanner over their already validated raw context. Constructed
contexts retain ordinary path selection. Standalone paths and indexed lookups keep
their existing validating selector and need no multi-demand scratch storage.

Two enclosing operations reuse this same primitive program:

- A streaming fold accepts a plain object path prefix, optional boolean predicates,
  and one numeric mapped stage. It visits candidates once, sharing surviving field
  loads between predicates and mapping, and reuses `aggregate::Fold` for count/sum/
  min/max/average. Nested source arrays, positional predicates and general sequence boundaries
  stay with the tree. Source selection shares object-prefix capture, and the raw array
  cursor captures candidate demands while locating each element boundary. Validation
  still finishes before folding begins. No candidate collection, stage views or lexical
  frames are needed on the accepted path.
- A fixed object compiles distinct constant keys and their reference ordering once.
  Computed primitive members share one program; prepared constant subtrees retain
  expression storage and fresh identity. Existing owned object storage holds the
  output, without the generic constructor's temporary key groups. Dynamic keys,
  grouped array contexts and directly borrowed member values keep tree construction.

Each plan retains its original pure tree. A type/shape miss retries that entire region
before publishing output, preserving exact errors and upstream-stage precedence.
No lexical effects can replay. Outputs that directly select input tokens stay on the
tree, preserving borrowing and large numeric tokens. Consumer cancellation between
mapped outputs continues through the existing stream. Whole-record validation precedes
all execution. Register execution stays out of line to avoid enlarging recursive tree
frames; construction has a separate instantiation of the same instruction loop.

These plans are a useful primitive lowering boundary, not a second general evaluator.
The numeric control flow could feed a future JIT, but traversal guards and general
sequence ownership remain external semantic operations. Measurements still identify
validation/scanning and normalization as larger opportunities than native arithmetic.
See PERFORMANCE for gains, fallback/compile costs and unchanged workloads.

## Numeric and date pictures

`format.rs` dispatches existing builtin signatures to separate number, integer/English
word, and Gregorian date modules. Direct calls with static pictures/options retain an
immutable `Program` beside their arguments; dynamic and first-class calls use the same
parsers/renderers. Preparation preserves contextual argument substitution, evaluation
order, missing propagation and runtime error timing. Invalid constant pictures remain
stored evaluation errors, not compilation failures. Fully constant calls use ordinary
folding. No new plan instructions, dependencies or value variants are needed.

Decimal rendering uses bounded stack text buffers; renderers build only their result.
Date parsing retains its `regress` matcher for static pictures. Shared signature
validation writes directly into caller-owned stack slots instead of returning/moving
a value array; prepared and ordinary calls use the same rules. Ordinary unescaped
subjects borrow UTF-8; escaped subjects decode only where needed. `OwnedString::text`
takes owned plain UTF-8, reusing its buffer when no escaping is needed; `body` accepts
an already encoded JSON body. Picture literals never bypass JSON escaping or use CLI token preservation as conversion.

A compile-time clock flag is separate from lexical effects. `$now`, `$millis` and
incomplete pictured dates read one timestamp in the existing evaluation-local scope;
immutable clock reads do not force sequence materialization. Default ISO parsing and
static pictures with a complete leading year need no clock/scope. Variables referencing
clock builtins conservatively request it, including before a shadowing assignment.
Unrelated trees/plans keep their existing value, context and allocation behavior.

## Decisions and measured limits

The tree exists because precedence, short-circuiting and typed operators now need
structure. Constructors add owned containers only when they are requested. There is
no general execution IR, JIT or input DOM. Function execution uses the same tree and values.
Fallback operators reuse scalar evaluation and calls. Coalescing stores its left
expression once as the argument of a shadowable `$exists` call, then re-evaluates
that argument when selected. This follows reference execution without exponentially
duplicating nested fallback trees at compile time.
Compilation preserves JSONata’s literal-versus-computed position rules and direct
nested-array syntax, even when their values are constant. See compile-time specialization above.

The safe scanner combines validation and selective capture for paths and root plans.
Path selection, demand capture and cursors share the object grammar and scalar scanner;
there is no trusted second parser. Validation covers every byte, including unselected
fields, before any semantic execution or output.
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

Nested raw-array lookup, wildcard flattening and descendant traversal now skip array
delimiters in one pass, scanning non-array leaves with the existing grammar. They do
not acquire a universal array index or flatten object-valued members recursively.
Multi-step paths starting on a raw object share a selective scan outside plan regions.
Identity and single-field context loads stay direct; multi-field scanner scratch stays
in an outlined helper. Constructed/copied values keep their existing field walks.
Different unplanned operands still scan their demanded paths separately after validation.
The benchmarks retain deep arrays, tiny records, multi-field scalars
and structural equality so these costs remain visible. Region-local demand capture
removes repeated object-prefix scans; general sequence traversal still uses cursors.
Further traversal machinery needs a measured benefit and a simple design.

## Consolidation review

Keep direct tree evaluation. Parsing already resolves precedence, static paths and
array focus; execution separates scalar operands, replayable streams and retained
containers. Those distinctions encode observable shape and error behavior. Flattening
them into one owned result or one generic iterator would reintroduce collection or
move complexity elsewhere. Specialization retains this tree, with bounded regions
where measurement justifies a smaller execution representation.

Milestone 7 profiles located costs in scanning and the scalar/stream ownership boundary. Returning known selections directly removes a redundant
stream walk; borrowing removes temporary ownership within live stages. Neither needs
an IR. Milestone 11 earns bounded numeric lowering through repeated-load sharing and lower
scalar overhead inside streams. Milestone 12 broadens this to primitive branches,
fixed objects and numeric folds. Milestone 13 fuses root validation with nested-demand
capture and fuses numeric-fold candidate capture with element scanning. These reuse
the existing grammar and preserve the fallback tree. Milestone 23 removes depth-multiplied
scans from recursive raw-array lookup, retains pure sort keys only when first requested, indexes wide groups locally, and avoids
pinning frames for immediate literal callees. Object/array transitions, negative-position
replays and distinct unplanned consumers remain explicit traversal costs.

Milestone 9 profiles and repeated runs find no lexical frame creation or variable
lookup on ordinary filters/folds. M8's larger context/operand layouts remain; the
new navigation features do not enlarge them. Stopping infallible traversal lookahead
after two items removes a measured redundant pass. Sorting/grouping retain only
where their semantics require it. See PERFORMANCE for residual costs and variation.

## Parent contexts and structural updates

`provenance.rs` resolves `%` at compilation into an immutable, compiler-generated
binding of a demanded path stage's incoming context. Parent means expression-path
ancestry, not physical JSON containment. Only affected paths become tuple streams;
ordinary values carry no ancestry, and ordinary paths/plans keep their existing
execution. Rows carry these bindings through filters, sorting and grouping. Parent
reads are replay-safe; user-variable reads retain their existing effect rules.
Parenthesized paths preserve ancestry while ending user `@`/`#` bindings.
Normalized path depths are refreshed for compile/call resource budgets. Closures
can retain an explicitly bound parent through the existing lexical frame model.

`transform.rs` represents `|pattern|update[,delete]|` as another captured callable.
`$clone` applies JSONata conversion rules, rather than raw token serialization.
`CopiedValue` is a fresh-identity view over validated input or compiled containers;
access lazily wraps containers and converts scalar leaves. Dynamic containers copy
member structure to break aliases, retaining borrowed strings. Cloning checks all
numeric leaves before returning, including unselected infinity. Clone views expose
ordinary container operations. Type predicates borrow direct storage once rather than
recursing through views; ordinary paths/plans allocate no clone storage.

Transforms select locations once, then apply merges/deletions in order. A local
change map keys cloned container identities; raw byte ranges prune unchanged
subtrees when rebuilding ancestors. It indexes updates, not the input document.
Updated containers own member lists; untouched subtrees keep immutable sharing and
input borrowing. Overlapping/repeated selections observe preceding updates, and
deletion evaluates after merge. The result never mutates the original argument.
Mutable aliases escaping through unscoped assignments, custom cloners, cyclic or
function-valued updates are explicit boundaries in CONFORMANCE. There is no
mutable-document overlay, universal ancestry tracking or second evaluator.

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
   differential coverage and allocation benchmarks. See CONFORMANCE for reference implementation boundary policies.
7. **Complete: architecture/performance consolidation.** Scalar paths consume known
   selections directly; scoped traversal borrows instead of copying ownership through
   each stage. No language or public API change. See PERFORMANCE for repeated results.
8. **Complete: lexical/function runtime.** Bindings, blocks, root context, conditionals,
   lambdas, captured environments, dynamic/higher-order calls and minimal builtins.
   Evaluated retention and effect-aware stages preserve single execution.
9. **Complete: path/sequence expansion.** Singleton retention, array ranges, wildcards,
   descendants, quoted path steps, grouping, stable ordering, membership and fallbacks.
   Direct evaluation remains the foundation; these features need shape and retention
   boundaries, not instruction decoding. Ordinary expressions still create no scope arena.
10. **Complete: compiler specialization.** Fold context-independent expressions, retain
    immutable static constructors, resolve builtin references and index static object
    lookups. Reuse validating path capture for a direct lookup's record-derived key.
11. **Complete: execution-plan evaluation.** Bounded numeric regions share path loads
    and execute primitive operations; unsupported values retry their pure tree. Existing
    traversal, constructors, static lookup and lexical control flow remain direct.
12. **Complete: broader execution regions.** Primitive branches, shared direct-field
    capture, fused numeric map/filter/folds, indexed lookups within numeric regions,
    and fixed objects with computed leaves. General sequence and lexical semantics
    retain their tree boundaries.
13. **Complete: demand-aware validation/traversal.** Root plans capture required
    object paths during validation; raw fold cursors capture candidates in one scan.
    General array normalization remains on the tree; no JIT or input index is added.

14. **Complete: scoped path composition.** Positional/context bindings, streamed
    joins, binding-preserving filtering/sorting/grouping and closure capture.

15. **Complete: standard-library expansion.** String and numeric helpers, collection/
    object operations, type introspection and map/filter/reduce/each/sift callbacks.
    Fixed builtin signatures, owned computed strings and native sequence boundaries
    reuse the existing value, closure and plan machinery.

16. **Complete: conversion and composition.** Deliberate `$string` / `$number`,
    concatenation, chaining, retained partial arguments and callable composition.
    Static builtin chains reuse existing specialization and streaming plans.

17. **Complete: regex and matchers.** Compiled patterns, stateful callable matchers,
    retained continuations and incremental match/split/replace consumption, including
    custom matchers and replacement closures.

18. **Complete: parent provenance and structural updates.** Demand-only ancestry,
    shared row environments, clone views and selective copy/update reconstruction.
    Direct tree and bounded plans remain the execution model.

19. **Complete: corpus classification and everyday helpers.** All 1,679 language
    cases are classified and asserted. Decimal half-even rounding, padding,
    sort/zip/single, encoding, error/assert, comments and Unicode names share
    existing builtin/value/callback machinery. Static helpers use constant folding;
    rounding and default sort/single calls with static inputs can precompute results.

20. **Complete: numeric/date formatting and parsing.** Static picture programs,
    dynamic fallback, integer/word/base conversion, UTC calendar and date matchers,
    evaluation-stable clocks, and explicit compatibility/resource boundaries.
    All 333 previously blocked cases in these families now assert results/errors.

21. **Complete: function-runtime closure.** Compiled user signatures, lambda signature
    bypass for partials, stack-safe direct/mutual/composed tail calls, and recycled
    uncaptured parameter/block slots. Non-tail recursion and infinite execution have
    separate explicit guards; no function lowering or JIT is introduced.

22. **Complete: dynamic evaluation and runtime effects.** Prepared static programs,
    contained ownership for dynamic code and escaped callables, inherited scopes/focus,
    lazy injectable randomness and shuffle. All 16 remaining corpus blockers are promoted.

Remaining picture and native host-coercion boundaries stay in CONFORMANCE.
Full mutable transform aliasing needs a deliberate transient-document model; do not
broaden immutable copy/update by approximating those effects. Host invocation still
needs a lifetime/resource contract.

Each milestone updates conformance, tests and representative benchmarks. Full
language support does not require every expression to use the same execution path.

## Traversal and retention consolidation

M23 keeps the tree and bounded plan unchanged. Full validation still precedes traversal,
effects and output. The flattened raw cursor trusts only validated slices and delegates
leaf boundaries to the same scanner; constructed/copied arrays retain their prior paths.
No result, ancestry, input-location or callback memo is shared across evaluations.

Sort-key retention uses existing replay-safety analysis, never speculative eager evaluation.
A column is allocated on its first comparison; failures occur at that original comparison,
missing keys stay last in both directions, and bindings/calls/eval/randomness remain
uncached. Group indexing changes lookup only, not key/value evaluation order. Literal
callee borrowing applies to ordinary calls; pending tail callees still retain their frame.
Dynamic closure bridges and genuinely captured frames keep their established ownership
rules. See PERFORMANCE for measured gains, added sort/group storage and remaining costs.
