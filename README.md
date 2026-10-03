# jx

A Rust JSONata engine and streaming CLI for compiling expressions once and evaluating
independent JSON records. Input stays borrowed where possible; validation covers the
entire record before any result or effect. No general input DOM is required.

The pinned JSONata 2.2.0 language corpus has **1,679 asserted outcomes**: 1,390 results,
288 mapped errors and one documented non-tail recursion guard. Compatibility beyond
that corpus is explicit. Rust **1.98.1**, edition 2024; MIT licensed. APIs remain unstable.

## Install and use

From a checkout of this repository:

```sh
cargo install --path crates/jx-cli --locked
printf '%s\n' '{"customer":{"id":42}}' | jx 'customer.id'
jx '{"total":$sum(orders[price>10].price),"ids":[orders.id]}' records.ndjson
jx -f transform.jsonata records.ndjson
```

The CLI streams NDJSON from stdin and files with bounded reusable buffers and synchronous
backpressure. Missing emits no line; sequences emit one line per item; arrays remain one
value. Each result is serialized completely before its line is published. See the
[CLI reference](docs/cli.md) for limits, exit codes, quoting and failure behavior.

Binary archives/checksums can be prepared for Linux, macOS and Windows with the
[release workflow](docs/releases.md). Publication is a separate maintainer action;
this repository's workflows never publish crates or create GitHub Releases.

## Embed

```rust
let expression = jx::compile("price * quantity")?;
let value = expression.evaluate(br#"{"price":2.5,"quantity":3}"#)?.single()?;
assert_eq!(value.unwrap().as_number(), Some(7.5));
# Ok::<(), jx::Error>(())
```

Compiled expressions are `Send + Sync`; evaluations have independent local state.
`for_each` / `try_for_each` consume streamed results, decoded accessors preserve borrowing,
and `collect_owned` explicitly detaches snapshots. Declare external bindings before
compilation; synchronous host callbacks remain effectful. The [embedding guide](docs/embedding.md)
and runnable [examples](crates/jx/examples) cover ownership, contexts, diagnostics and
cooperative resource controls. See [compatibility and limits](docs/compatibility.md)
before running untrusted expressions.

## Features and performance

Default features are empty. Ordinary Cargo builds and `just build` do not compile
Cranelift. The optional `jit` feature builds a bounded native backend; execution also
requires `--jit` or `Expression::enable_native()`. Interpreter fallback remains in place.
Supported native targets are x86_64 Linux/macOS/Windows and aarch64 Linux/macOS.

Numeric loops benefit most; large scanning-dominated records generally do not. Allocation
depends on the expression: ordinary paths/scalar plans can allocate nothing per record,
while constructors, callbacks and retained results may allocate. The [current performance
summary](docs/performance.md) gives scoped measurements and links to reproducible evidence.

## Develop

`just all` runs formatting, strict Clippy, default/native tests, conformance, packaging
tests and allocation-asserting benchmark smoke. `just build`, `just bench`, `just robustness`
and `just package` provide focused checks. `just package` assembles/verifies source
archives only. CI separates default/native builds on Linux, macOS and Windows.

[Architecture](ARCHITECTURE.md), [conformance inventory](CONFORMANCE.md),
[detailed performance evidence](PERFORMANCE.md) and [agent guidance](AGENTS.md) describe
the implementation. Coverage-guided fuzzing is documented [separately](fuzz/README.md).
