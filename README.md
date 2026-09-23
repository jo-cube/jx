# jx

A Rust JSONata engine designed for compiling an expression once and evaluating
millions of independent JSON records. Early development: **identity and object
field paths only**. Full JSONata is the semantic target; see [coverage](CONFORMANCE.md).

Requires Rust **1.98.1** and [just](https://just.systems). The workspace contains
`crates/jx` (library) and `crates/jx-cli` (binary `jx`). APIs may change freely.

```sh
just all
just build
printf '%s\n' '{"customer":{"id":42}}' | target/release/jx 'customer.id'
target/release/jx --max-record-bytes 1048576 '$' records.ndjson
```

```rust
let expression = jx::compile("customer.id")?;
for record in [br#"{"customer":{"id":42}}"#.as_slice(), b"{}"] {
    for value in expression.evaluate(record)? {
        println!("{}", value.as_str());
    }
}
```

Evaluation validates the entire UTF-8 record before returning borrowed results.
No general JSON tree or per-record heap allocation is needed for this subset.
Compilation owns field names; results borrow only the input. There are no engine
runtime dependencies. `serde_json` is a test-only oracle and fixture reader.

The CLI compiles once, accepts stdin or files (`-` means stdin), and writes compact
NDJSON synchronously. Missing produces no line; null produces `null`; a selected
array is one JSON value on one line. Traversal **through** arrays is explicitly
unsupported, including empty and singleton arrays. Blank lines are ignored; the
last line may omit LF. The default 1 MiB record limit excludes LF but includes
CR and whitespace. Memory is bounded by the largest accepted record plus I/O
buffers. Invalid records stop processing; earlier records remain written.
Usage/compilation errors exit 2; record/I/O errors exit 1; broken pipes exit 0.

`just` lists commands. [Architecture and milestones](ARCHITECTURE.md),
[conformance](CONFORMANCE.md), [benchmarking](PERFORMANCE.md), and
[agent guidance](AGENTS.md) describe the development contract.
