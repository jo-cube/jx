# jx

`jx` is a Rust [JSONata](https://docs.jsonata.org/overview.html) engine and a streaming
CLI. Compile an expression once, then evaluate independent JSON records. The complete
input is validated; values stay borrowed where possible without an input DOM.

## Install the CLI

Install a published release on Linux or macOS:

```sh
curl -fsSL https://raw.githubusercontent.com/jo-cube/jx/main/scripts/install.sh | sh
```

Select a specific published version:

```sh
curl -fsSL https://raw.githubusercontent.com/jo-cube/jx/main/scripts/install.sh | VERSION=v0.2.0 sh
```

The default destination is `~/.local/bin`. See [installation and builds](docs/releases.md)
for a custom directory, supported platforms, Windows downloads and source installation
with Rust **1.98.1** or newer. Use a source build when the requested release is unavailable.
License: MIT.

## Use it

```sh
printf '%s\n' '{"items":[{"price":2.5,"quantity":3}]}' |
  jx '{"total":$sum(items.(price * quantity))}'
# {"total":7.5}
jx 'customer.id' records.ndjson
jx -f transform.jsonata first.ndjson - second.ndjson > results.ndjson
```

The CLI streams NDJSON from stdin/files in order with bounded reusable buffers.
Missing emits no line; sequences emit one line per item; arrays remain one value.
Each result finishes serialization before writing its line. See [CLI usage](docs/cli.md)
for quoting, result shape, resource limits and exit behavior.

## Rust library

Depend on a reviewed Git revision today:

```toml
[dependencies]
jx = { git = "https://github.com/jo-cube/jx", rev = "REVIEWED_COMMIT" }
```

Registry publication is deferred; the API is pre-release and can change. The library
import and CLI remain `jx`. The crates.io package currently named `jx` is a different project.

```rust
fn main() -> Result<(), jx::Error> {
    let expression = jx::compile("price * quantity")?;
    for record in [br#"{"price":2.5,"quantity":3}"#.as_slice(),
                   br#"{"price":4,"quantity":2}"#.as_slice()] {
        expression.evaluate(record)?.for_each(|value| {
            println!("{}", value.as_number().unwrap());
        })?;
    }
    Ok(())
}
```

Compiled expressions are `Send + Sync`. Results can borrow input/expression storage;
owned snapshots detach them explicitly. [Embedding](docs/embedding.md) covers external
bindings, focus, synchronous host functions, cancellation and ownership.

## Language and execution

Paths/sequences, filters, operators, constructors, lexical functions/closures, partials,
sorting/grouping, parent navigation, transforms, regex, numeric/date pictures, `$eval`,
randomness and higher-order helpers are supported. The pinned JSONata 2.2.0 corpus has
1,679 asserted outcomes, including one documented non-tail recursion limit. See
[compatibility and limits](docs/compatibility.md) and [conformance](CONFORMANCE.md).

All public evaluation APIs and the CLI use one common semantic IR, with automatic
input acquisition, borrowed results and streaming where semantics allow. There is
no engine-selection flag, native feature or Cranelift dependency.

## Performance

Measure the actual expression, input distribution and output obligations. Compilation,
validation, acquisition, transformation and serialization have separate costs.
[Performance guide](docs/performance.md) describes reproducible controls;
[architecture](ARCHITECTURE.md) explains execution and ownership.

## Development

`just all` checks formatting, strict Clippy, workspace tests, conformance, archive
tooling, guide examples and allocation budgets. `just build`, `just bench` and
`just robustness` offer focused workflows. Start with [contributing/testing](docs/development.md);
[AGENTS.md](AGENTS.md) contains the durable implementation rules.
