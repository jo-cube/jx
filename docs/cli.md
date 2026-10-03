# CLI reference

```text
jx [OPTIONS] [--] EXPRESSION [FILE ...]
jx [OPTIONS] -f EXPRESSION_FILE [FILE ...]
```

The expression compiles once before reading records. No files, or a `-` file argument,
reads stdin. Files and stdin are processed in argument order. Options precede the
expression or `-f`; later arguments are file paths. Use `--` for a leading `-` expression
or a path beginning with `-`. Source must be UTF-8; file names use native platform paths.

| Option | Behavior |
| --- | --- |
| `-f`, `--expression-file PATH` | Read JSONata source, including newlines/comments, from a file |
| `--max-record-bytes N` | Positive byte limit per input record; default 1,048,576 |
| `--max-output-bytes N` | Positive byte limit per output result; default 16,777,216 |
| `--max-work N` | Enable cooperative evaluation controls with this work budget and other default limits |
| `--jit` | Enable eligible numeric kernels; only available in a `jit` feature build |
| `-h`, `--help` | Print usage/options |
| `--version` | Print package version |

Input is UTF-8 NDJSON, one complete JSON value per line. Empty/whitespace-only lines are
ignored. LF, CRLF and a final record without LF work. The input limit excludes LF but
includes CR and whitespace; oversized lines are rejected during reading. Entire JSON
records are validated before evaluation. The CLI stops on the first error.

Output is compact JSON plus LF: missing emits nothing, null emits `null`, sequences emit
separate lines and arrays remain one line. To package a sequence as one array, use an
array constructor (`[items.id]`) or JSONata retention (`items.id[]`). Byte-preserving raw
serialization can retain numeric/escape spelling; `$string` follows separate JSONata rules.

Each result is fully serialized into a reused bounded buffer before writing. A failing
result leaves no partial NDJSON line; earlier complete results from the same record or
previous records remain. This is result atomicity, not transactionality over a record,
file, or OS write. An underlying I/O failure may interrupt a write. Neither entire input
files nor all outputs are collected. Retained engine values can exceed I/O buffer sizes.

Diagnostics go to stderr with expression location or file/line and phase/kind. JSON goes
to stdout. Exit status is **0** for success/broken pipe, **1** for input, evaluation,
serialization or I/O failure, **2** for usage, expression-file or compilation failure.
Earlier complete lines are flushed when a later result fails. See
[compatibility/limits](https://github.com/jo-cube/jx/blob/dev/docs/compatibility.md) for resource policy and numeric/Unicode boundaries.

POSIX shells protect `$` and other operators with single quotes:

```sh
jx '{"id":id,"total":$sum(items[price>10].price)}' records.ndjson > results.ndjson
jx -f transform.jsonata first.ndjson - second.ndjson
jx --max-record-bytes 10485760 --max-output-bytes 33554432 '$' large.ndjson
jx --max-work 100000 '[1..100].($*$)' records.ndjson
```

PowerShell also supports single-quoted expressions; `-f` avoids native-command quoting
pitfalls for complex programs. `cmd.exe` expands `%` and uses different quoting rules,
so expression files are preferable there. The shell's redirected output encoding is a
separate concern; `jx` writes UTF-8 bytes directly.
