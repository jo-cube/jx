# Embedding jx

From a checkout, run `cargo run -p jx --example embedding --locked`, or pipe NDJSON
to `cargo run -p jx --example stream --locked -- 'price*quantity'`. The examples use
only public APIs; the stream example expects bounded trusted input, while the CLI
enforces byte limits during reading. Add `jx` as a path/git dependency pinned to your
reviewed revision until registry publication is deliberately chosen. Default features
are empty; enable `jit` and call `enable_native()` once for optional numeric acceleration.

For a local checkout in an external Rust application:

```toml
[dependencies]
jx = { path = "/path/to/jx/crates/jx" }
```

A Git dependency can instead name this repository and a reviewed `rev`. The crates.io
name `jx` belongs to a different project; do not use a registry dependency until this
engine deliberately publishes under its chosen package name.

Compile once, share `Expression` (or `Arc<Expression>`) across callers, and evaluate
independent borrowed byte records. Evaluations and runtime values are local to a caller;
`OwnedValue` snapshots are `Send + Sync` and independent of input and expression storage.

## Values and results

`Evaluation::for_each` streams without collecting. `try_for_each` returns either
`ConsumeError::Evaluation` or the original `ConsumeError::Consumer`; consumer failure
stops later traversal. Scalar computations and lexical retention can run before the
consumer is called. `single()` stops at the second item with `CardinalityError`.
`collect_owned()` explicitly collects detached snapshots, preserving result cardinality.

`Value::get` accesses a container field without mapping/flattening; raw duplicate fields
use the last decoded key. `array_items` and `object_entries` return optional iterators,
not copied collections. Keys decode to `Cow<str>`; entries expose raw duplicate keys in
storage order. `as_str()` returns `Result<Option<Cow<str>>, Error>`: a type mismatch is
`None`, escapes allocate, and isolated surrogates raise `EncodingError`. `string_units`
preserves those units losslessly. Numbers remain binary64, including computed NaN/infinity;
JSON serialization encodes nonfinite numbers as null.

`Value::to_owned()` copies only when explicitly requested, preserving missing, null,
array/sequence shape and encoded UTF-16 string units. It does not use CLI serialization
or round-trip through a parser. Snapshot access uses `OwnedValue::as_value()`;
`OwnedValue::from_json()` validates and detaches JSON input. Functions anywhere in a
snapshot are rejected: JSONata closures carry evaluation-local frame indices. They can
be inspected as function values, or invoked during a host callback in that same evaluation,
but cannot be injected into a different evaluation. Host functions have independent ownership.

## Bindings and host functions

External names omit `$` and must be declared before compilation. This keeps static
builtin resolution and purity analysis correct, including declarations that shadow
builtin names. Supplying an undeclared name raises `BindingError`, rather than executing
already-folded or planned code with stale assumptions. Missing declarations fall back
to the builtin (if any); an explicit `Value::Undefined` shadows it with missing.
Local bindings, parameters and tuple bindings shadow external bindings lexically.

```rust
let expression = jx::CompileOptions::default()
    .binding("scale").binding("add")
    .compile("$add(price*$scale, quantity)")?;
let add = jx::HostFunction::new(2, |args, _context| {
    let a = args[0].as_ref().unwrap().as_number().unwrap();
    let b = args[1].as_ref().unwrap().as_number().unwrap();
    Ok(Some(jx::Value::Number(a+b)))
}).with_signature("<nn:n>")?;
expression.evaluate_with(Some(br#"{"price":2.5,"quantity":3}"#), jx::EvaluationOptions {
    bindings: vec![("scale", jx::Value::Number(2.0)), ("add", add.value())],
    ..Default::default()
})?.for_each(|value| assert_eq!(value.as_number(), Some(8.0)))?;
# Ok::<(), jx::Error>(())
```

Bindings retain evaluated values once. `Value::from_json`, `from_string`, `from_array`
and `from_object` construct embedding inputs; raw values remain borrowed. Names and
values need only live through the evaluation. Reuse a host `value()` by cloning it to
avoid rebuilding its callable wrapper per record. Host implementations are synchronous
`Send + Sync` closures; mutable state may use normal Rust synchronization. Every host
call is effectful and is excluded from purity-based replay, key caching and lowering.
The declared arity describes callback argument selection; optional signatures validate
arguments once per call using the same compiled rules as JSONata lambdas. Return types
are descriptive, matching JSONata. Unsigned callbacks validate their own arguments.

`HostContext` exposes focus/root, a cooperative checkpoint and synchronous `invoke` for
JSONata function arguments. Returned argument/focus clones preserve borrowing; newly
constructed values own only their new storage. Callback re-entry shares the lexical
arena, random source and controls. `Error::user` becomes a structured cause of `HostError`;
Rust panics retain normal Rust panic policy. Host code is trusted and must cooperate
with cancellation. No FFI/plugin registry, async protocol or detached closure API exists.

## Context and effects

`evaluate_with(None, options)` means absent input; `Some(b"null")` means JSON null.
`options.focus` replaces current `$`, while `$$` remains the validated input/absent root.
Root arrays retain top-level focus semantics. `$eval` inherits the same bindings and
resource controls, including dynamically owned programs and escaping callbacks.
`options.random` or `evaluate_with_random` accepts a caller-owned `Random` stream;
`Random::seeded` is deterministic. Defaults initialize randomness lazily.
An empty options value with present input uses the ordinary evaluation fast path.

## Limits and cancellation

Existing hard ceilings always apply: JSON/expression depth 128, at most 64 active
calls/512 accumulated expression levels, and one million iterations per tail chain.
`EvaluationOptions` can supply `Limits`, `Cancellation` and/or a monotonic `deadline`.
No configurable control state is allocated unless one is requested.

`Limits::default()` allows one million work checkpoints, inspected/emitted intermediate
items and emitted results, 16 MiB of compact result JSON across an evaluation, 64 calls,
and one million tail calls per chain. Stack/tail settings can tighten hard ceilings.
Work units are semantic checkpoints, not CPU instructions: calls, loop/traversal
boundaries and retained collection arguments count; exact counts may evolve.
Result bytes exclude NDJSON delimiters; function-bearing results remain available to
non-serializing consumers and do not accrue completed JSON bytes. Limits are checked before
delivering each result. Calls/traversal can fail before `evaluate_with` returns or during consumption.
Controlled planned regions use their tree source to keep checkpoints effective.

Cancellation tokens are cloneable across threads and remain cancelled once set.
Deadlines/cancellation are cooperative, not hard process timeouts or memory quotas.
Complete JSON validation runs before effects/control failures; callers should bound
input/source sizes. Regex calls and noncooperating host code cannot be interrupted
mid-call. String/picture helpers retain their existing local growth guards. Output
preflight rejects oversized results but cannot undo construction already completed.
Applications handling hostile code still need their own process-level isolation.

## Diagnostics

`Error` exposes kind, message, byte offset, `Phase`, `Source`, point `span()` and optional
nested `cause()` (also standard `std::error::Error::source`). `location(source)` returns
one-based line/Unicode-scalar column when the offset is a valid boundary. Point spans
are deliberate where a complete token range is unavailable. Input-validation errors,
compile errors, dynamic source errors, host causes and output-limit errors have distinct
sources/phases. Dynamic errors preserve the outer call offset and inner failure.
Exact upstream messages/codes, call traces and parser recovery are not API contracts.
`Value::write_compact` uses ordinary `io::Error` for serialization/I/O failures.
