# Embedding jx

Compile once and evaluate independent borrowed byte records. `Expression` is cloneable
and `Send + Sync`, so callers can share it directly or through `Arc`. Evaluation state
and ordinary runtime values stay local to the caller; the engine does not spawn workers.

Use a Git dependency pinned to a reviewed revision:

```toml
[dependencies]
jx = { git = "https://github.com/jo-cube/jx", rev = "REVIEWED_COMMIT" }
```

Registry publication will come later; the crates.io package named `jx` is a different
project. The library target/import remains `jx`, and APIs are pre-release and can change.
For checkout development, use `jx = { path = "/path/to/jx/crates/jx" }` instead.
Default features are empty; [native support](releases.md#optional-native-acceleration)
is optional and also requires `enable_native()` once before sharing an expression.

## Compile and consume

```rust
fn main() -> Result<(), jx::Error> {
    let expression = jx::compile("items[price>10].price")?;
    let input = br#"{"items":[{"price":5},{"price":12},{"price":20}]}"#;
    expression.evaluate(input)?.for_each(|value| {
        println!("{}", value.as_number().unwrap());
    })?;
    Ok(())
}
```

`for_each` consumes a sequence without collecting it. `try_for_each` stops on consumer
failure and distinguishes `ConsumeError::Evaluation` from your original consumer error.
`single()` returns `None` for missing and reports `CardinalityError` at the second item.
`collect_owned()` deliberately collects detached snapshots. Scalar computation and
lexical retention can happen before consumption; lazy navigation can fail during it.
Earlier delivered values remain visible after a later failure.

## Borrowed access and owned snapshots

Use `as_number`, `as_bool`, `as_str`, `get`, `array_items` and `object_entries` rather
than inspecting storage variants. Container access does not map/flatten values.
`as_str()` returns `Result<Option<Cow<str>>, Error>`: mismatch is `None`, unescaped text
borrows, and escape decoding may allocate. Isolated UTF-16 surrogates have no Rust `str`
representation; `string_units` preserves them. Object entries expose storage order and
raw duplicate keys; `get` selects the last decoded matching key.

```rust
fn main() -> Result<(), jx::Error> {
    let expression = jx::compile("customer")?;
    let snapshot = {
        let input = br#"{"customer":{"name":"Ada","id":42}}"#.to_vec();
        let value = expression.evaluate(&input)?.single()?.unwrap();
        assert_eq!(value.get("name").unwrap().as_str()?.unwrap(), "Ada");
        value.to_owned()?
    }; // input is gone; snapshot remains valid
    assert_eq!(snapshot.as_value().get("id").unwrap().as_number(), Some(42.0));
    Ok(())
}
```

When several expressions share one input, call `jx::validate` once and pass its
`RawJson` to `Expression::evaluate_validated`. `evaluate_validated_with` accepts
`Option<RawJson>` and the same `EvaluationOptions` as `evaluate_with`. These methods
skip validation, retain specialized capture/execution, and preserve result borrowing
and lazy failures. For a single expression, `evaluate` can fuse validation with capture.

For repeated static paths, `InputPlan::new(&expressions)` unions bounded demands from
independently compiled expressions. `plan.prepare(bytes)` validates and captures in
one traversal; `prepare_validated(raw)` captures without validating again. The returned
`PreparedInput` evaluates expressions by their original index, in any order, with
`evaluate(index)` or `evaluate_with(index, options)`. Results do not borrow the capture
frame; `as_raw()` exposes the validated root without copying it. Unsupported
expressions, excess demands and intermediate arrays use ordinary
validated evaluation. Bindings, focus, randomness and controls retain their existing
per-expression execution. Preparation never runs expressions or effects.

Ordinary results may borrow both the input and compiled expression. Do not recycle the
input buffer while results remain borrowed. New containers own their structure but can
retain borrowed leaves. `OwnedValue` is independent of both lifetimes and `Send + Sync`;
`to_owned()` preserves missing/null, sequence shape and UTF-16 units without a serialization
round-trip. `OwnedValue::from_json` validates and detaches an input value.

Snapshots reject functions anywhere inside them. JSONata closures carry evaluation-local
frame indices; they can be returned/used inside that evaluation but cannot be injected
into another. Independently owned host functions can cross evaluations.

## External bindings and host callbacks

Declare names without `$` before compilation so builtin resolution, folding and plans
respect external shadowing. Bindings store already-evaluated values once; undeclared
injection raises `BindingError`. An unsupplied declared name can fall back to a builtin;
explicit `Value::Undefined` shadows it with missing. Local variables/parameters shadow
external names lexically.

```rust
fn main() -> Result<(), jx::Error> {
    let expression = jx::CompileOptions::default()
        .binding("scale").binding("add")
        .compile("$add(price*$scale, quantity)")?;
    let add = jx::HostFunction::new(2, |args, _context| {
        let a = args[0].as_ref().unwrap().as_number().unwrap();
        let b = args[1].as_ref().unwrap().as_number().unwrap();
        Ok(Some(jx::Value::Number(a+b)))
    }).with_signature("<nn:n>")?;
    let add_value = add.value(); // reuse this wrapper across records
    let options = jx::EvaluationOptions {
        bindings: vec![("scale", jx::Value::Number(2.0)), ("add", add_value.clone())],
        ..Default::default()
    };
    let value = expression.evaluate_with(Some(br#"{"price":2.5,"quantity":3}"#), options)?
        .single()?.unwrap();
    assert_eq!(value.as_number(), Some(8.0));
    Ok(())
}
```

Fixed JSON-compatible data can be bound before compilation with
`CompileOptions::constant_binding(name, owned_value)`. It accepts an `OwnedValue` or
`Arc<OwnedValue>`; reuse the Arc or compile options across expressions to share storage.
No per-record binding injection is needed. Static reads fold into existing constants,
lookups and plans where safe. Local assignments/parameters still shadow the name;
`$eval` inherits the constant environment. Rebinding and dynamic semantics conservatively
retain ordinary lexical evaluation. A constant name cannot also be declared for runtime
binding or overridden through `EvaluationOptions`.

`Value::from_json` supplies validated borrowed input; `from_string`, `from_array` and
`from_object` construct owned structure. Names/values need only live through evaluation.

Host implementations are synchronous `Send + Sync` closures. Every call is effectful,
including calls reading mutable state; no host purity promise enters plans or key reuse.
Optional signatures compile once and validate arguments using JSONata's rules; callbacks
without signatures validate their own arguments. Arity controls higher-order argument
selection, and return signature types are descriptive.

`HostContext` exposes focus/root, `checkpoint()` and synchronous `invoke()` for JSONata
function arguments. Re-entry shares the lexical arena, controls and random stream.
Returned argument/focus clones preserve borrowing; newly built values own their new
storage. `Error::user` becomes a nested cause of `HostError`. Panics follow Rust's normal
policy. Host code is trusted and must cooperate with cancellation; there is no async/FFI
framework or detached JSONata function runtime.

## Root, focus and effects

`evaluate_with(None, options)` means absent input; `Some(b"null")` means JSON null.
`options.focus` replaces `$`, while `$$` retains the original input or absent root:

```rust
fn main() -> Result<(), jx::Error> {
    let expression = jx::compile("[$$.id, $.price]")?;
    let options = jx::EvaluationOptions {
        focus: Some(jx::Value::from_json(br#"{"price":12}"#)?),
        ..Default::default()
    };
    let value = expression.evaluate_with(Some(br#"{"id":42}"#), options)?.single()?.unwrap();
    let numbers: Vec<_> = value.array_items().unwrap().map(|v| v.as_number().unwrap()).collect();
    assert_eq!(numbers, [42.0, 12.0]);
    Ok(())
}
```

`$eval` inherits lexical bindings and controls; escaping dynamic programs own what their
results need. `options.random` or `evaluate_with_random` accepts a `Random` stream;
`Random::seeded` provides deterministic draws. Defaults initialize randomness lazily.
Empty options with present input use ordinary evaluation.

## Limits and cancellation

```rust
fn main() -> Result<(), jx::Error> {
    let expression = jx::compile("$sum(items.price)")?;
    let cancellation = jx::Cancellation::default(); // clone to a cancelling thread
    let options = jx::EvaluationOptions {
        limits: Some(jx::Limits { max_work: 100_000, ..Default::default() }),
        cancellation: Some(cancellation.clone()),
        deadline: Some(std::time::Instant::now() + std::time::Duration::from_secs(1)),
        ..Default::default()
    };
    expression.evaluate_with(Some(br#"{"items":[{"price":12}]}"#), options)?
        .for_each(|value| assert_eq!(value.as_number(), Some(12.0)))?;
    Ok(())
}
```

Controls are optional; no control state allocates unless requested. Work units count
semantic checkpoints, not instructions. Result counts and compact byte totals are checked
before delivery; JSONata function results remain inspectable without a JSON encoding.
Controls can fail during evaluation or consumption. Controlled planned regions use tree
execution where needed; empty options preserve the ordinary fast path.

Tokens stay cancelled after `cancel()`. Deadlines/cancellation are cooperative, not
process timeouts or memory quotas. Complete input validation runs first. Regex/formatting
and noncooperating host calls cannot be interrupted mid-call; output preflight cannot
undo construction already performed. Bound input/source sizes and use process isolation
where hostile code requires it. [Compatibility and limits](compatibility.md#resource-policy)
list hard/default ceilings.

## Diagnostics and examples

`Error` exposes kind/message, byte offset, `Phase`, `Source`, point `span()` and optional
`cause()` (also `std::error::Error::source`). `location(source)` gives one-based line and
Unicode-scalar column at valid offsets. Dynamic/host failures preserve nested causes.
Exact upstream messages, call traces and parser recovery are not API contracts.
`Value::write_compact` uses `io::Error` for serialization/I/O failures; raw output and
language `$string` conversion have different contracts.

Runnable [embedding](../crates/jx/examples/embedding.rs) and
[stream](../crates/jx/examples/stream.rs) examples use only public APIs:

```sh
cargo run -p jx --example embedding --locked
printf '%s\n' '{"price":2.5,"quantity":3}' |
  cargo run -p jx --example stream --locked -- 'price * quantity'
```

The stream example expects trusted bounded input. Use the CLI's reader or your own
byte limits for untrusted streams.
