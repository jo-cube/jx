# jx

A Rust JSONata engine for compiling once and evaluating independent borrowed JSON
records. Rust 1.98.1; MIT licensed; APIs are pre-release.

Use a versioned Git dependency, replacing `vX.Y.Z` with a published tag:

```toml
[dependencies]
jx = { git = "https://github.com/jo-cube/jx.git", tag = "vX.Y.Z" }
```

No release tag is published yet. Until then, use a reviewed upstream commit with
`rev = "REVIEWED_COMMIT"`, or a local checkout. See the
[dependency guide](https://github.com/jo-cube/jx/blob/main/docs/embedding.md#add-the-dependency)
for versioning and lockfile behavior. The library import is `jx`.

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

See [embedding](https://github.com/jo-cube/jx/blob/main/docs/embedding.md) for shared
input preparation, runtime bindings, complete-result serialization and resource controls;
[compatibility](https://github.com/jo-cube/jx/blob/main/docs/compatibility.md) for language
boundaries; and [performance](https://github.com/jo-cube/jx/blob/main/docs/performance.md)
for measurement guidance. Runnable `stream` and `embedding` examples are included.
