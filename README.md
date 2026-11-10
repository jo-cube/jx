# jx

`jx` is a Rust [JSONata](https://docs.jsonata.org/overview.html) engine with a standalone
CLI for newline-delimited JSON (NDJSON). Select fields, filter records and compute
summaries in shell pipelines or Rust applications. Expressions compile once for
repeated evaluation; JSON values borrow their source bytes where possible.

## Install

No GitHub Release is published yet; [build from source](docs/releases.md#source-builds)
for now. Once releases are available, the verified Unix installer installs to `~/.local/bin`:

```sh
curl -fsSL https://raw.githubusercontent.com/jo-cube/jx/main/scripts/install.sh | sh
```

See [installation and releases](docs/releases.md) for Linux/macOS requirements, explicit
versions, custom destinations and Windows ZIP downloads. MIT licensed.

## Use the CLI

Project fields and compute an order total:

```sh
printf '%s\n' '{"id":42,"items":[{"price":2.5,"quantity":3}]}' |
  jx '{"id":id,"total":$sum(items.(price * quantity))}'
# {"id":42,"total":7.5}
```

Keep matching records:

```sh
printf '%s\n' '{"id":1,"status":"paid"}' '{"id":2,"status":"pending"}' |
  jx '$[status = "paid"]'
# {"id":1,"status":"paid"}
```

Read an expression from `transform.jsonata` and process files and stdin in order:

```sh
jx -f transform.jsonata first.ndjson - second.ndjson > results.ndjson
```

Each input line is evaluated independently. Missing produces no line; sequences produce
one line per item; arrays remain one JSON value. `[items.id]` builds one array from a path.
The [CLI guide](docs/cli.md) covers quoting, result shape, limits and errors.

## Embed in Rust

Use a published Git tag, replacing `vX.Y.Z` with the chosen release:

```toml
[dependencies]
jx = { git = "https://github.com/jo-cube/jx.git", tag = "vX.Y.Z" }
```

The library requires Rust **1.98.1** or newer; its API is pre-1.0. Compiled expressions
can be shared across evaluations. The [embedding guide](docs/embedding.md) covers borrowed
values, shared input preparation, runtime bindings and complete-result JSON serialization,
with runnable Rust examples and a Git-revision option before the first release.

## Documentation

- [Language compatibility and resource limits](docs/compatibility.md)
- [Performance and measurement](docs/performance.md)
- [Architecture and ownership](ARCHITECTURE.md)
- [Development and validation](docs/development.md)
