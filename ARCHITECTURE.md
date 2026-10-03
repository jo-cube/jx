# Architecture

## Compilation and execution

`source → owned expression tree → demanded ancestry → effects/builtin resolution →
constant specialization → bounded plan lowering → validating demand capture → execution → results`

`crates/jx` owns semantics; `jx-cli` owns NDJSON/I/O; optional `jx-native` owns executable
code. Rust 1.98.1/edition 2024. The engine and CLI forbid unsafe Rust. Compiled expressions
are immutable `Send + Sync` data; per-record state is local. No input DOM, worker pool,
universal index/cache or general execution IR exists.

Parsing owns field names/string encodings and operator offsets once. Syntax/tree depth
is bounded at 128. Analysis resolves builtins only when source bindings, external name
declarations and dynamic evaluation cannot shadow them. Lexical reads/writes, dynamic
calls, host callbacks and effects prevent unsafe replay/lowering. Compile-time execution
uses existing semantics; failures remain runtime expressions. Static constructors/pictures,
regexes and constant `$eval` programs are prepared once. Static object keys have a sorted
fingerprint index with exact UTF-16 collision checks; dynamic keys are decoded without copying.

The public `compile`/`evaluate` path remains specialized for standalone paths, path
conversions, indexed lookups and literal-call argument acquisition. The bounded plan
contains primitive loads/lookups, arithmetic/comparisons/truth, forward branches/moves,
fused numeric fold bodies and fixed numeric/boolean constructors. Parameter-based pure
callbacks can use those same regions. It deduplicates paths/computations and captures
nested field demands while validating every byte. Guards retry only pure regions through
the retained tree source, preserving error precedence and exposing no partial output.
Strings, provenance, dynamic invocation/effects and general ownership stay in Rust.
Date matchers keep a compiled flag for exact supplementary literals so their UTF-8
matching preserves legacy UTF-16 case rules without a new subject copy.

Validation is complete, including undemanded branches and fields. Scanner internals use
compact static diagnostics. Captures store raw spans or missing/deferred markers on the
stack; demanded nested objects can share one scan. Arrays resume from retained raw ranges.
Array-led scopes, complex sequences and unsuitable shapes use normal traversal rather than
forcing capture metadata onto dynamic operations. Unplanned distinct consumers and
some nested/cardinality traversals can still replay.

## Values, sequences and ownership

`Value` holds borrowed raw input, borrowed compiled constants/literals, binary64/boolean/
null/missing primitives, shared encoded strings, immutable constructed containers,
copy views or function values. Constructed arrays/objects own member lists behind `Rc`
and retain borrowed leaves. Cloning shares structure. Strings retain UTF-16 escape units;
UTF-8 decoded access borrows unescaped text and allocates only when decoding escapes.
`$string` conversion is separate from byte-preserving compact serialization.

Missing has no result; null has one. Internal array shapes distinguish ordinary arrays,
path-preserved arrays, normalized sequences and kept sequences (`[]`). Path mapping flattens
according to JSONata boundaries; scalar consumers determine missing/singleton/multiple
cardinality. Pure multi-item results remain replayable streams when safe. Lexical values,
arguments, constructors and retained results store evaluated values once, never a thunk
that might re-run a call or random draw. `OwnedValue` snapshots deliberately copy into
immutable constant data, retaining shape/UTF-16 units and rejecting functions. They are
independent of both lifetimes and `Send + Sync`; ordinary results never require them.

Scoped sequence views live on the stack. Filters use the same expression/context machinery;
negative positions retain stage length across replays. Aggregates fold streams with one
pending item to settle shape, without a universal vector. Numeric type errors wait for
argument evaluation to preserve upstream error ordering. Scalar operations use primitive
representations. Constructors retain only stored sequences/members; mapped constructors
can emit one finished container at a time.

Grouping builds local key groups, with a decoded-key hash index above 32 distinct keys.
Ordering retains candidates/indices and lazily computes replay-safe keys once at first
comparison; secondary keys remain short-circuited. Effectful/tuple comparators preserve
reference invocation order. Tuple bindings use ordinary lexical frames. A tuple-local
flag preserves the post-sort object context until mapping resumes bindings; it adds
no metadata to ordinary values/paths. Demand-derived
parent slots capture only requested ancestry; ordinary paths have no universal ancestry
metadata. Clone/transform views share untouched structure and rebuild changed containers
and ancestors with borrowed leaves; advanced mutable host aliasing remains unsupported.

## Lexical runtime and embedding

One optional lexical arena owns stable-index frames. Closures contain a definition,
focus and frame index, with no owning back-edge. Uncaptured terminal frames recycle.
Completed retained regions retire new frames only if returned functions/containers cannot
reach them and no older-frame write exports them. Lazy streams, genuine escapes and older
writes retain captures. Tail calls trampoline through existing evaluation, reuse uncaptured
frames and share guards; non-tail calls keep a bounded native Rust stack.

Dynamic code uses a shorter-lifetime scope bridge and retains only escaping definitions/
values. Existing pure dynamic callback plans bypass frame copying: primitives export
directly, fixed objects retain only their result and static members. Signatures and call
guards remain shared; controlled execution, lexical state, effects and unsupported shapes
use the established loan/export path. Random sources are lazy and optional; timestamps
are evaluation-fixed. No ordinary compiled value becomes universally owned for `$eval`.

`CompileOptions` declares external names before optimization. `EvaluationOptions` supplies
borrowed names, already-evaluated values, optional focus/absent input, random stream and
controls. Undeclared binding injection is rejected. External JSONata closures are rejected
because arena indices cannot cross evaluations; independently owned host functions can.
Host callbacks use the existing `FunctionKind`/invocation/partial/dynamic-retention paths,
with `Arc` implementations and optional compiled signatures. They are always effectful.
`HostContext` can invoke a JSONata callback synchronously in the same arena. No host purity
promise, async registry or public detached function runtime exists.

Controls add one optional pointer to the lexical arena, not to every value/context/frame.
Without controls, existing acquisition/plans/native execution remain available and empty
options delegate to ordinary evaluation. Cooperative work/item checks occur at calls,
tail/stream boundaries and retained builtin arguments, rather than every scalar AST node.
Controlled plans/callbacks use tree fallback. Result counts/compact bytes are checked
before consumer delivery. Deadlines/cancellation do not interrupt validation, regex calls,
or noncooperating host code; they are not process memory/time quotas. Existing stack,
range, padding, formatting and transform growth ceilings remain in place.

Diagnostics retain kind/message/byte offset plus phase/source and optional owned causes;
scanner success paths do not build errors. Spans are point locations where token ends
are unknown. Consumer errors remain separate and preserve their original type.

## Native boundary

The opt-in `jit` feature lowers only bounded numeric/boolean register programs and fold
bodies to Cranelift. Validation, demand capture, traversal, normalization, decoding,
allocation, functions and errors remain Rust helpers. Unsupported operations/types/guards
use interpreter/tree fallback; effects are never speculatively executed. Default builds
have neither native checks nor Cranelift dependencies.

Only `jx-native/src/executable.rs` converts the finalized pointer to a fixed C ABI, invokes
it with bounded stack buffers and releases code. Compile-time operand/branch validation,
an `Arc` owner and a mutex-owned non-`Sync` module pin code through calls/clones without
runtime locks or unsafe sharing traits. Failed compilation/final drop release memory.
Native coverage is intentionally bounded; wider lowering/JIT requires measured benefit.

## CLI boundary and future work

The CLI compiles once, validates bounded NDJSON records and uses synchronous backpressure.
Input and result buffers are reused across files. A bounded result buffer finishes JSON
serialization before writing its line; serialization/evaluation failures cannot publish
an incomplete result. Earlier finished results remain visible; I/O write failures can
still interrupt output. Optional work controls use the library API. The CLI owns framing,
filenames, line numbering, exit policy and expression-file loading.

Keep compatibility boundaries and boundary-failure tests current. Seeded properties run
with ordinary tests; optional coverage-guided fuzzing shares their bounded harness.
Default workspace members exclude the native crate; engine/CLI native features and execution
remain separate opt-ins. Versioned path dependencies and independent source packages keep
publication separate from binary artifact preparation. Cross-platform CI tests default/native
builds separately; unsafe invocation remains isolated. See [release policy](docs/releases.md).
Keep pure tree/plan and native layers separate. Remaining string callback projections,
nonprimitive dynamic bridges, genuine captured lifetimes and unplanned traversal need
specific measurements before caching, garbage collection or further lowering.
