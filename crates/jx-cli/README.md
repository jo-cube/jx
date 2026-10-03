# jx CLI

Compile a JSONata expression once and stream independent NDJSON records from stdin
or files. Rust 1.98.1; MIT licensed. The executable is named `jx`.

From a repository checkout:

```sh
cargo install --path crates/jx-cli --locked
printf '%s\n' '{"customer":{"id":42}}' | jx 'customer.id'
jx -f transform.jsonata records.ndjson
```

Missing emits no line, sequences emit one line per item, and arrays remain one JSON
value. Each result is fully serialized before publishing its line; earlier complete
lines remain when a later result fails. Input/output buffers are reused, backpressure
is synchronous, and files are processed in argument order. Default limits are 1 MiB
per input record and 16 MiB per output result, excluding LF.

Use `--help` for options. Exit codes: 0 success/broken pipe, 1 record/I/O failure,
2 usage/expression compilation failure. Diagnostics go to stderr; JSON goes to stdout.

Default builds contain no native backend. `--features jit` builds optional numeric
acceleration; passing `--jit` then enables execution. Unsupported shapes fall back.
Prebuilt release artifacts use the portable interpreter configuration.

See the [CLI reference](https://github.com/jo-cube/jx/blob/dev/docs/cli.md),
[compatibility/limits](https://github.com/jo-cube/jx/blob/dev/docs/compatibility.md) and
[release guide](https://github.com/jo-cube/jx/blob/dev/docs/releases.md).
