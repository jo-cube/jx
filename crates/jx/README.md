# jx

A Rust JSONata engine for compiling once and evaluating independent borrowed JSON
records. Rust 1.98.1; MIT licensed; APIs are pre-release.

Use a reviewed Git revision; crates.io publication will come later:

```toml
[dependencies]
jx = { git = "https://github.com/jo-cube/jx", rev = "REVIEWED_COMMIT" }
```

The library target/import is `jx`. The crates.io package currently named `jx` is a
different project; APIs here can change before a stable release.

```rust
fn main() -> Result<(), jx::Error> {
    let expression = jx::compile("price * quantity")?;
    let value = expression.evaluate(br#"{"price":2.5,"quantity":3}"#)?.single()?.unwrap();
    assert_eq!(value.as_number(), Some(7.5));
    Ok(())
}
```

`Expression` is `Send + Sync`; each evaluation has local state. Results preserve
borrowing where possible. `for_each`/`try_for_each` stream, `single` checks cardinality,
and `collect_owned` detaches snapshots. The complete input is validated before effects.

Paths, sequences, operators, functions/closures, standard helpers, grouping/sorting,
regex, pictures, transforms and dynamic evaluation are supported. See
[compatibility](https://github.com/jo-cube/jx/blob/dev/docs/compatibility.md) for precise boundaries.

External bindings, synchronous effectful host callbacks and cooperative resource
controls are described in the [embedding guide](https://github.com/jo-cube/jx/blob/dev/docs/embedding.md).
Default features are empty; optional `jit` plus `enable_native()` accelerates eligible
numeric regions. Normal execution remains available for unsupported regions.

[Performance](https://github.com/jo-cube/jx/blob/dev/docs/performance.md) describes allocation,
traversal and benchmark trade-offs. Public `stream`/`embedding` examples are included;
repository-only corpus tests and benchmarks stay outside source packages.
