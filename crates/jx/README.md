# jx

Compile-once JSONata evaluation over borrowed UTF-8 JSON records. Rust 1.98.1;
MIT licensed; APIs remain unstable. No general input DOM is required.

```rust
let expression = jx::compile("price * quantity")?;
let value = expression.evaluate(br#"{"price":2.5,"quantity":3}"#)?.single()?;
assert_eq!(value.unwrap().as_number(), Some(7.5));
# Ok::<(), jx::Error>(())
```

Compilation is separate from evaluation. `Expression` is `Send + Sync` and cloneable;
each evaluation has independent local state. Results borrow input/expression data
when possible. Missing emits no values, null is a value, arrays remain values, and
sequences emit items in order. Use `for_each` / `try_for_each` to consume without
collection, `single` for at most one item, or `collect_owned` to detach snapshots.
Snapshots reject JSONata functions; closures are local to their evaluation.

The entire JSON input is validated before any result/effect. Errors during streamed
consumption may follow earlier values. `Value::write_compact` writes JSON, preserving
raw spellings where possible; `$string` follows JSONata conversion semantics separately.

`CompileOptions` declares external lexical bindings before compilation.
`EvaluationOptions` supplies their values, optional focus/absent input, synchronous
host functions, deterministic randomness, limits, cancellation and deadlines.
Host calls are effectful. Controls are cooperative and cannot interrupt an individual
regex or noncooperating host call; bound source/input sizes for untrusted workloads.

Default features are empty and have no native dependencies. Optional `jit` builds
Cranelift numeric kernels on x86_64 Linux/macOS/Windows and aarch64 Linux/macOS.
Call `enable_native()` once before sharing the expression to opt into execution.
Guards/unsupported regions keep interpreter fallback; scanning-heavy records rarely
benefit. General traversal, strings, ownership and dynamic effects remain in Rust.

The pinned JSONata 2.2.0 language corpus has 1,679 asserted outcomes: 1,390 results,
288 mapped errors and one explicit non-tail recursion guard. This is not complete
JavaScript host compatibility. See the [compatibility guide](https://github.com/jo-cube/jx/blob/dev/docs/compatibility.md),
[embedding guide](https://github.com/jo-cube/jx/blob/dev/docs/embedding.md) and
[performance evidence](https://github.com/jo-cube/jx/blob/dev/docs/performance.md).

Runnable `stream` and `embedding` examples use only the public API. Repository tests
and benchmarks depend on the pinned corpus and stay outside the source package.
No crates.io publication is performed by repository workflows.
