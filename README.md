# jx

`jx` is a Rust [JSONata](https://docs.jsonata.org/overview.html) engine and a streaming
CLI. Compile an expression once, then evaluate independent JSON records. The complete
input is validated; values stay borrowed where possible without an input DOM.

## Install the CLI

Use the normal binary from [GitHub Releases](https://github.com/jo-cube/jx/releases).
Release assets are prepared but **no release is published yet**. The download flow below
applies when assets are available; source builds are available now.

Example for Linux x86-64; choose your target from the [platform table](docs/releases.md#platforms):

```sh
version=0.1.0
target=x86_64-unknown-linux-gnu
archive="jx-v${version}-${target}.tar.gz"
url="https://github.com/jo-cube/jx/releases/download/v${version}"
curl -fLO "$url/$archive" &&
  curl -fLO "$url/$archive.sha256" &&
  sha256sum -c "$archive.sha256" &&
  tar -xzf "$archive" &&
  mkdir -p "$HOME/.local/bin" &&
  install -m 755 "jx-v${version}-${target}/jx" "$HOME/.local/bin/jx"
```

On macOS use `shasum -a 256 -c`; Windows uses `.zip` archives. Ensure the install directory
is on PATH. [Installation and builds](docs/releases.md) covers all platforms, checksum
verification and source installation with Rust **1.98.1** or newer. License: MIT.

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

The **normal build is recommended** and contains the complete general runtime, with no
Cranelift dependency. The optional `-native` release adds acceleration for eligible
numeric/boolean regions and selected folds; pass `--jit` to enable it. Unsupported
regions still use ordinary execution. Heavily reused numeric expressions benefit most;
scanning-heavy workloads may gain little. Library builds use the `jit` Cargo feature
and explicit `enable_native()`.

## Current performance

Compile-time specialization, borrowed leaves, streaming sequences, demand capture,
indexed lookup and fused pure plans reduce repeated work. Retained/constructed values
allocate when ownership requires it.

| Workload | Input bytes | Mode | Records/s | Allocations/record |
| --- | ---: | --- | ---: | ---: |
| Shallow path | 500 | normal | 3.21 M | 0 |
| Scalar arithmetic | 1,024 | normal | 1.61 M | 0 |
| 8,192-key static lookup | 500 | normal | 2.82 M | 0 |
| Filter/map/sum, 1,024 rows | 41,828 | normal | 5.92 k | 0 |
| Constructed object | 500 | normal | 2.45 M | 3 |
| Numeric filter/map/sum, 1,024 rows | 14,794 | native | 10.24 k | 0 |

These are medians from two warmed, single-threaded runs of the current library, including
complete validation and result consumption, excluding compilation, serialization and I/O.
Workload shape, input/output size and execution mode materially affect results.
[Performance guide and raw measurements](docs/performance.md) describe the fixtures,
method and trade-offs; [architecture](ARCHITECTURE.md) explains the execution layers.

## Development

`just all` checks formatting, strict Clippy, default/native tests, conformance, archive
tooling and allocation budgets. `just build`, `just bench` and `just robustness` offer
focused workflows. Start with [contributing/testing](docs/development.md);
[AGENTS.md](AGENTS.md) contains the durable implementation rules.
