# jx

A Rust JSONata engine designed for compiling an expression once and evaluating
millions of independent JSON records. Early development: **paths through objects and arrays,
result sequences, scalar operators, filters, aggregates, constructors, lexical
variables, conditionals, closures, wildcard navigation, grouping, ordering, indexed/joined paths,
common string/collection/higher-order functions (including round, pad, sort/zip/single and encoding), conversions, function pipelines, regex/matcher text processing, parent navigation and structural transforms**.
Full JSONata is the semantic target; see
[coverage](CONFORMANCE.md).

Requires Rust **1.98.1** and [just](https://just.systems). The workspace contains
`crates/jx` (library) and `crates/jx-cli` (binary `jx`). APIs may change freely.

```sh
just all
just build
printf '%s\n' '{"customer":{"id":42}}' | target/release/jx 'customer.id'
target/release/jx 'price * quantity' records.ndjson
target/release/jx 'orders[price > 10].id' records.ndjson
target/release/jx '$sum(orders[price > 10].price)' records.ndjson
target/release/jx '{"total":$sum(orders[price > 10].price),"ids":[orders.id]}' records.ndjson
target/release/jx '($prices:=orders.price; {"total":$sum($prices),"count":$count($prices)})' records.ndjson
target/release/jx 'orders[price > 10]^(>price){kind:{"ids":id[],"total":$sum(price)}}' records.ndjson
target/release/jx 'orders#$i.{"index":$i,"id":id}' records.ndjson
target/release/jx '$join($map(orders,function($r){$uppercase($trim($r.name))}),", ")' records.ndjson
target/release/jx 'orders ~> $map(function($r){$r.name & "=" & $number($r.price)}) ~> $join(", ")' records.ndjson
target/release/jx 'orders[name ~> /hat/i].{"name":$replace(name,/hat/i,"cap")}' records.ndjson
target/release/jx 'orders.items[price > %.limit].{"order":%.id,"price":price}' records.ndjson
target/release/jx '$ ~> |orders[price > 10]|{"price":price*1.2},"obsolete"|' records.ndjson
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
serializes both, plus constructed objects and arrays. Containers own their member
lists and retain borrowed leaves; cloning a constructed value shares its storage. Input slices extracted with `as_raw()` can outlive the expression;
string literals borrow compiled storage; computed strings own shared encoded bytes. Constant constructors also borrow immutable
compiled data, and static object lookups use a prebuilt key index. `try_for_each` propagates consumer errors
immediately, distinguishing `ConsumeError::Consumer` from `ConsumeError::Evaluation`.
Both callback APIs return evaluation failures. Lexical bindings and function arguments
retain evaluated sequences once; repeated variable use does not re-run expressions.
Function values are opaque and `write_compact` rejects them as non-JSON.

Ordinary field paths and scalar/filter/aggregate workloads on borrowed or primitive
values allocate no per-record heap storage. Dynamic constructors allocate their structure and retained
member sequences; constant containers allocate only a fresh identity token; mapped constructors can emit one container at a time. Structural
equality may retain borrowed members or one sequence. Sorting, grouping and `[]`
retention store their output; scoped paths allocate binding/frame storage and can stream
boolean filters and aggregates; wildcard/descendant object enumeration retains one
object’s members to resolve duplicate keys and ordering. Higher-order functions retain
arguments and output once, keeping borrowed leaves; string transformations own their results.
`$string` performs JSONata conversion separately from token-preserving output; string inputs
keep borrowing. Static builtin chains retain their ordinary streaming execution; partial
functions store evaluated arguments. Parent access captures only statically demanded
path contexts. `$clone` gives input/compiled containers fresh identities through immutable
views; transforms rebuild changed containers and their ancestors while sharing untouched
structure and borrowed leaves. Input is never parsed into a general JSON DOM. Regex literals compile once with `regress`; matcher cursors are local to
one evaluation, and continuations retain their subject. ASCII subjects stay borrowed;
escaped/non-ASCII subjects decode once to UTF-16. `base64` supplies the binary codecs; `serde_json` is a test-only oracle and fixture reader.

The CLI compiles once, accepts stdin or files (`-` means stdin), and writes compact
NDJSON synchronously. Missing produces no line; null produces `null`; sequences
emit one line per item. An array value or explicitly kept sequence (`expr[]`) stays on one line. JSONata mapping and singleton rules
determine which is returned; see the examples in [coverage](CONFORMANCE.md).
Blank lines are ignored; the last line may omit LF. The default 1 MiB record limit excludes LF but includes
CR and whitespace. Memory is bounded by the largest accepted record plus I/O
buffers, lexical frames/retained values and constructed output for that record, and equality/grouping
storage where needed. Invalid records or runtime
errors stop processing. JSON validation precedes all output; a later mapped expression error
can leave earlier items from that record written. Each aggregate consumes its whole
argument before emitting its scalar result. A constructor finishes its members before
emitting its container; mapped constructors can emit earlier complete containers.
Usage/compilation errors exit 2; record/I/O errors exit 1; broken pipes exit 0.

`just` lists commands. [Architecture and milestones](ARCHITECTURE.md),
[conformance](CONFORMANCE.md), [benchmarking](PERFORMANCE.md), and
[agent guidance](AGENTS.md) describe the development contract.
