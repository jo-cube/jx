# jx

A Rust JSONata engine designed for compiling an expression once and evaluating
millions of independent JSON records. Early development: **paths through objects and arrays,
result sequences, scalar operators, and filters**. Full JSONata is the semantic target; see
[coverage](CONFORMANCE.md).

Requires Rust **1.98.1** and [just](https://just.systems). The workspace contains
`crates/jx` (library) and `crates/jx-cli` (binary `jx`). APIs may change freely.

```sh
just all
just build
printf '%s\n' '{"customer":{"id":42}}' | target/release/jx 'customer.id'
target/release/jx 'price * quantity' records.ndjson
target/release/jx 'orders[price > 10].id' records.ndjson
target/release/jx --max-record-bytes 1048576 '$' records.ndjson
```

```rust
let expression = jx::compile("price * quantity")?;
expression.evaluate(br#"{"price":2.5,"quantity":3}"#)?.for_each(|value| {
    assert!(matches!(value, jx::Value::Number(7.5)));
})?;
```

Evaluation validates the entire UTF-8 record before returning results. Paths keep
borrowed raw JSON; computed numbers and booleans use primitives. `write_compact`
serializes either. Input slices extracted with `as_raw()` can outlive the expression;
string literals borrow compiled storage. `try_for_each` propagates consumer errors
immediately, distinguishing `ConsumeError::Consumer` from `ConsumeError::Evaluation`.
Both callback APIs return evaluation failures; neither collects results.

Ordinary paths, filters and scalar operators allocate no per-record heap storage. Structural
equality may retain borrowed members or one sequence. There is no general JSON tree
or engine runtime dependency; `serde_json` is a test-only oracle and fixture reader.

The CLI compiles once, accepts stdin or files (`-` means stdin), and writes compact
NDJSON synchronously. Missing produces no line; null produces `null`; sequences
emit one line per item. A raw array value stays on one line. JSONata mapping and singleton rules
determine which is returned; see the examples in [coverage](CONFORMANCE.md).
Blank lines are ignored; the last line may omit LF. The default 1 MiB record limit excludes LF but includes
CR and whitespace. Memory is bounded by the largest accepted record plus I/O
buffers and temporary equality storage where needed. Invalid records or runtime
errors stop processing. JSON validation precedes all output; a later predicate error
can leave earlier items from that record written.
Usage/compilation errors exit 2; record/I/O errors exit 1; broken pipes exit 0.

`just` lists commands. [Architecture and milestones](ARCHITECTURE.md),
[conformance](CONFORMANCE.md), [benchmarking](PERFORMANCE.md), and
[agent guidance](AGENTS.md) describe the development contract.
