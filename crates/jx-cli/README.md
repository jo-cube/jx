# jx CLI

Compile a JSONata expression once and stream independent NDJSON records from stdin
or files. The executable is `jx`. Rust 1.98.1; MIT licensed.

Use the normal binary from [GitHub Releases](https://github.com/jo-cube/jx/releases)
when published. Archives include a `.sha256` checksum; names use Rust platform targets,
for example `jx-vVERSION-x86_64-unknown-linux-gnu.tar.gz`. Windows archives are `.zip`.
The [installation guide](https://github.com/jo-cube/jx/blob/dev/docs/releases.md)
has curl download/verification examples and a platform table. No release is published yet;
a reviewed source checkout can use `cargo install --path crates/jx-cli --locked`.

```sh
printf '%s\n' '{"customer":{"id":42}}' | jx 'customer.id'
jx -f transform.jsonata records.ndjson
```

Missing emits no line, sequences emit one line per item, and arrays remain one value.
Each result finishes serialization before writing its line; earlier complete lines
remain after a later failure. Buffers are reused and backpressure is synchronous.
Default limits are 1 MiB per input record and 16 MiB per output result, excluding LF.

Use `--help` for options. Diagnostics go to stderr. Exit status: 0 success/broken pipe,
1 input/evaluation/output failure, 2 usage/expression-file/compilation failure.

The normal build is recommended and has no Cranelift dependency. Optional archives add
`-native` before the extension; both variants contain `jx`/`jx.exe`. Pass `--jit` to enable
numeric/boolean acceleration and selected folds in that variant. Unsupported regions
continue through the complete normal runtime; scanning-heavy workloads may gain little.
Source builds need `--features jit` as well as runtime `--jit`.

See the [CLI guide](https://github.com/jo-cube/jx/blob/dev/docs/cli.md),
[compatibility/limits](https://github.com/jo-cube/jx/blob/dev/docs/compatibility.md) and
[build/release guide](https://github.com/jo-cube/jx/blob/dev/docs/releases.md).
