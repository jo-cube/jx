# Architecture

`crates/jx` owns the language and evaluation. `crates/jx-cli` owns NDJSON framing,
files and output policy. The optional `crates/jx-native` owns executable code.
Compiled expressions are immutable `Send + Sync`; evaluation state is local to a caller.

```text
JSONata source
      |
      v
compiled semantic tree + static data
      |
      +--> general tree evaluator
      |
      +--> bounded optimized plan
                 |
                 +--> plan interpreter
                 +--> optional native numeric kernels

JSON bytes --> complete validation + demand capture --> execution --> results
```

## Compilation and traversal

Compilation parses source, resolves lexical/builtin references, derives requested
ancestry, identifies effects and specializes constants. Static constructors, indexed
objects, regexes, pictures and known `$eval` programs become immutable expression data.
Constant failures stay conditional runtime failures; compilation does not move errors
into unselected branches or bypass input validation.

Every input byte is validated, including undemanded fields and untaken branches.
Compact demand metadata captures required raw spans during validation where possible.
Repeated planned loads, pure call arguments and bounded pure root regions reuse those
captures. Root string/conversion and fixed constructor regions use one stack capture
frame, sharing scalar/constructor operations with tree execution and existing numeric
plans. Dynamic contexts, effects and controls stay on their original paths; intermediate
arrays fall back before region execution. An explicit `InputPlan` can union the same
bounded pure root demands across compiled expressions. Its borrowed `PreparedInput`
contains only validated spans; each evaluation runs independently, and deferred demands
fall back per expression. Existing expression
entry points keep their own acquisition fast paths. Runtime contexts and values carry
no capture metadata. Nested demanded objects can share a scan; arrays and dynamic
navigation use ordinary traversal.
Traversal of validated spans locates token boundaries without repeating grammar
validation. The validator and traversal cursor share path/demand handling; only the
validator accepts unchecked input. There is no universal input index or per-record cache.

## Plans and fallback

Plans cover useful pure regions: scalar loads and lookups, arithmetic/comparisons,
truth tests, forward branches/moves, selected filter/map/numeric folds and fixed
constructors with computed primitive members. Eligible callback bodies use the same
operations. Compilation shares repeated demands and computations within these regions.
Registers and captures are bounded; ordinary scalar execution uses stack storage.

A plan retains its tree source. Unsupported shapes or failed guards fall back before
publishing output, preserving type errors and evaluation order. Only replay-safe work
can be retried. Strings, general ownership, provenance, dynamic calls, host functions
and effects remain in normal Rust evaluation. Controlled execution uses tree regions
where needed to keep cancellation and work checks effective.

Lowering is deliberately bounded: representing every language feature would duplicate
the general evaluator without a demonstrated benefit. Native execution has a narrower
boundary still.

## Values, sequences and retention

Values can be raw borrowed JSON, expression-owned constants, primitive scalars,
encoded strings, constructed containers or functions. Constructed containers own
member lists and share immutable structure; their leaves can still borrow input.
Decoded unescaped strings borrow, while escape decoding may allocate. JSONata string
conversion is separate from token-preserving JSON output.

Missing, null, arrays and result sequences are distinct. Navigation applies JSONata's
flattening/cardinality rules; pure sequences can stream or replay where safe. Lexical
bindings, arguments and captured results retain evaluated values once, never recipes
that could re-run a call or random draw. Filters and aggregates share this sequence
model. Sorting and grouping retain candidates because their semantics require it.
Replay-safe sort keys are computed lazily once; effectful comparisons preserve call order.

An optional lexical arena holds frames. Closures capture focus and frame indices without
owning back-edges. Uncaptured frames recycle; completed regions release frames only when
no returned closure/container or older-frame write can reach them. Tail calls trampoline
with bounded Rust stack; non-tail calls remain guarded.

`$eval` confines dynamic program ownership to its scope bridge and escaping values.
It does not make ordinary values universally owned. Parent provenance is derived from
requested paths, rather than attached to every value. Clone/transform views share
untouched structure and rebuild changed containers/ancestors. Owned snapshots explicitly
detach JSON-compatible results; JSONata functions remain evaluation-local.

## Runtime boundaries

External names are declared before optimization. Immutable owned external data is shared
by compile options/expressions and substituted only when scope analysis proves the name
unchanged. Static navigation folds through ordinary semantics; dynamic evaluation,
rebinding and identity-sensitive transforms retain a borrowed immutable environment
beneath evaluation-local frames. Host functions are always effectful,
synchronous and excluded from pure plans/replay. Randomness initializes lazily;
timestamps are fixed within an evaluation. Optional controls live in evaluation state,
not on each value. Diagnostics carry phase, source, offset and nested causes.
See [embedding](docs/embedding.md) for the public ownership and control contracts.

The CLI compiles once, bounds record/result buffers and applies synchronous backpressure.
It serializes a result completely before writing its line. Earlier completed results
remain visible if later work fails; I/O errors can still interrupt a write.

## Optional native backend

The `jit` feature and explicit runtime enablement compile eligible numeric/boolean
register programs and selected fold bodies with Cranelift. Validation, capture,
traversal, strings, allocation, normalization and general JSONata semantics stay in Rust.
Failed compilation/guards keep interpreter/tree fallback available.

Engine and CLI Rust forbid unsafe code. Executable pointer conversion, invocation and
release are confined to `jx-native/src/executable.rs`. A fixed C ABI uses bounded stack
buffers; an `Arc` owner keeps immutable code alive through calls/clones. No evaluator
lock or unsafe sharing trait is needed. Default builds do not depend on Cranelift.
See [performance](docs/performance.md#optional-native-execution) and [feature/platform policy](docs/releases.md).
