# jx CLI

Compile a JSONata expression once and stream independent NDJSON records from stdin
or files. The executable is `jx`; binaries need no Rust toolchain. Source builds
require Rust 1.98.1 or newer. MIT licensed.

No GitHub Release is published yet. See the
[installation guide](https://github.com/jo-cube/jx/blob/main/docs/releases.md) for source
builds now and the verified Unix installer, Windows ZIPs and platform requirements
when releases are available. Archive names use `jx-vVERSION-TARGET.tar.gz` on Unix
and `.zip` on Windows, with adjacent `.sha256` checksums.

```sh
printf '%s\n' '{"customer":{"id":42}}' | jx 'customer.id'
jx -f transform.jsonata records.ndjson
```

Missing emits no line, sequences emit one line per item, and arrays remain one value.
Each result finishes serialization before writing its line; earlier complete lines
remain after a later failure. Diagnostics go to stderr.

The [CLI guide](https://github.com/jo-cube/jx/blob/main/docs/cli.md) covers expression
files, quoting, limits and exit codes. See
[compatibility](https://github.com/jo-cube/jx/blob/main/docs/compatibility.md) for language
and resource boundaries.
