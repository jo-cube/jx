# jx

A Rust JSONata engine for compiling once and evaluating independent JSON records.
It supports the large majority of practical JSONata: navigation, sequences, operators,
filters, aggregates, constructors, lexical functions, grouping/sorting, regex, pictures,
parent navigation, transforms and dynamic evaluation. The pinned 2.2.0 language corpus
has 1,679 asserted outcomes, including one documented recursion guard. See
[compatibility boundaries](CONFORMANCE.md); passing this corpus is not a claim of
complete compatibility with every JavaScript host behavior.

Rust **1.98.1**, edition 2024. APIs are still unstable. The library and CLI use safe Rust;
optional native numeric kernels have a separate, bounded executable-code boundary.

```sh
just all
just build
printf '%s\n' '{"customer":{"id":42}}' | target/release/jx 'customer.id'
target/release/jx '{"total":$sum(orders[price>10].price),"ids":[orders.id]}' records.ndjson
target/release/jx -f transform.jsonata records.ndjson
target/release/jx --max-record-bytes 1048576 --max-output-bytes 16777216 '$' records.ndjson
```

```rust
let expression = jx::compile("price * quantity")?;
let value = expression.evaluate(br#"{"price":2.5,"quantity":3}"#)?.single()?;
assert_eq!(value.unwrap().as_number(), Some(7.5));
# Ok::<(), jx::Error>(())
```

`evaluate` validates the entire UTF-8 JSON record before exposing any result. Missing
emits nothing; null is a value; arrays are one value; sequences stream their items.
`for_each` and `try_for_each` avoid collection, and consumer failure stops later work.
Scalar/lexical expressions may finish before consumption; streamed evaluation failures
can follow earlier values. `single` requires at most one result. `collect_owned` is
an explicit materialization boundary for results that must outlive input/expression.

Use `value_type`, `as_number`, `as_bool`, `as_str`, `get`, `array_items` and
`object_entries` across all storage forms. Unescaped strings borrow; decoded escapes
allocate. Isolated UTF-16 surrogates use `string_units` rather than Rust `str`.
`write_compact` preserves raw number/escape spelling; JSONata `$string` has its own
conversion rules. Constructed containers retain borrowed leaves. No input DOM is
required, and ordinary paths/scalar plans retain zero-allocation execution.

[Embedding guide](docs/embedding.md) covers external bindings, synchronous host
callbacks, focus/absent input, ownership, diagnostics and cooperative resource limits.
Compiled expressions are `Send + Sync`; each evaluation owns its lexical/effect state.
Declare external names with `CompileOptions` before compilation so builtin shadowing,
constant folding, plans and native execution remain correct. Host calls are always
effectful; there is no host purity override or async framework.

The CLI compiles once and streams stdin/files (`-` means stdin) with synchronous
backpressure. Blank lines are ignored; the final line may omit LF. Options precede the
expression or `-f`; `--` allows expressions beginning with `-`. `--version` prints the
version. Input defaults to 1 MiB per record, excluding LF and including CR/whitespace;
output defaults to 16 MiB per result, excluding LF. Both buffers are reused across files.
Each result is serialized completely before its NDJSON line is published. A failed
result leaves no partial line; earlier complete results remain visible. An underlying
I/O failure can still interrupt a write. `--max-work N` enables cooperative controls
with the other default library limits. Engine retention/construction can require more
memory than the I/O buffers. Usage/compilation errors exit 2; record/I/O errors exit 1;
broken pipes exit 0. Diagnostics include file/record context and error phase.

Native acceleration stays opt-in: build with `cargo build -p jx-cli --release --features jit --locked`, then pass `--jit`, or call `enable_native()` on a compiled expression.
Unsupported regions and guards retain interpreter/tree fallback. Numeric loops benefit;
scanning-dominated records generally do not. The core library has no Cranelift dependency
unless `jit` is enabled. The workspace build/check also exercises the native crate.

`just` lists developer commands. [Architecture](ARCHITECTURE.md),
[conformance](CONFORMANCE.md), [performance](PERFORMANCE.md) and
[agent guidance](AGENTS.md) describe implementation boundaries and evidence.
