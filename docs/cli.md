# Using the CLI

`jx` compiles one expression, then processes independent NDJSON records in order.
Diagnostics go to stderr; compact JSON results go to stdout.
Install the recommended normal [release binary](https://github.com/jo-cube/jx/blob/dev/docs/releases.md#download-and-install),
or use a source build while releases are being prepared.

```sh
printf '%s\n' '{"customer":{"id":42}}' | jx 'customer.id'
jx '{"id":id,"total":$sum(items[price>10].price)}' records.ndjson > results.ndjson
jx -f transform.jsonata first.ndjson - second.ndjson
```

No files, or a `-` argument, reads stdin. Files and stdin are processed in argument
order. The CLI stops on the first error and uses synchronous backpressure; it never
reads a complete file into memory.

## Expressions and input

```text
jx [OPTIONS] [--] EXPRESSION [FILE ...]
jx [OPTIONS] -f EXPRESSION_FILE [FILE ...]
```

Options precede the expression or `-f`; later arguments are file paths. Use `--` before
a leading `-` expression. File arguments after the expression, and the path after `-f`,
are taken literally. Source is UTF-8;
file names use native platform paths. Expression files can contain newlines and comments:

```text
/* transform.jsonata */
{
  "customer": customer.id,
  "total": $sum(items.(price * quantity))
}
```

Input contains one complete UTF-8 JSON value per line. Blank/whitespace-only lines are
ignored. LF, CRLF and a final line without LF are accepted. The byte limit excludes LF
but includes CR and whitespace; oversized records are rejected while reading. Every
record is completely validated, including fields the expression does not use.

POSIX shells and PowerShell generally use single quotes around JSONata to protect `$`
and other operators. Complex programs are easier with `-f`, especially in `cmd.exe`
where `%` expands and quoting differs. `jx` writes UTF-8 directly; a shell's redirection
encoding is a separate concern.

## Result shape

| Result | Output |
| --- | --- |
| Missing | No line |
| Null or scalar | One JSON line |
| JSONata sequence | One line per item, in order |
| Array or object | One JSON line, including empty containers |

`jx 'items.id'` can emit several lines per record. Use `[items.id]` to construct one
array, or `items.id[]` to retain a sequence's singleton shape; missing still stays missing
with retention. These forms have different JSONata flattening rules.

Raw output can preserve numeric/escape spelling and duplicate keys. `$string` is a
separate language conversion, not a way to obtain byte-preserving CLI output.
Function values have no JSON encoding and cause serialization failure.

Each result finishes serialization in a reused bounded buffer before its line is
written. Failure leaves no partial result line; earlier complete results from the same
record or previous records remain. This is not a transaction over a record/file.
An underlying I/O error can interrupt a write. Input/result buffers are bounded, but
retained engine values can require additional memory.

## Options and limits

| Option | Behavior |
| --- | --- |
| `-f`, `--expression-file PATH` | Read the expression from a file |
| `--max-record-bytes N` | Positive per-record input limit; default 1,048,576 |
| `--max-output-bytes N` | Positive per-result serialized byte limit; default 16,777,216 |
| `--max-work N` | Cooperative work budget plus other default evaluation limits, per input record |
| `--jit` | Enable eligible native kernels; available only in a `jit` feature build |
| `-h`, `--help` | Print usage and options |
| `--version` | Print package version |

```sh
jx --max-record-bytes 10485760 --max-output-bytes 33554432 '$' large.ndjson
jx --max-work 100000 '$sum(items.price)' records.ndjson
```

Work units are semantic checkpoints, not CPU instructions. Controls are cooperative;
validation finishes first and regex/host calls cannot be interrupted mid-call.
See [compatibility and resource limits](https://github.com/jo-cube/jx/blob/dev/docs/compatibility.md).

The recommended normal binary has no native backend. An optional `-native` release
adds eligible numeric/boolean acceleration and selected folds; pass `--jit` to enable it.
Source builds use `--features jit`. Unsupported regions continue through normal execution;
scanning-heavy work may gain little. See [native installation](https://github.com/jo-cube/jx/blob/dev/docs/releases.md#optional-native-acceleration).

## Errors and exit status

Diagnostics include expression location or file/line, phase and error kind.

| Status | Meaning |
| --- | --- |
| 0 | Success, including a downstream broken pipe |
| 1 | Input, evaluation, serialization or I/O failure |
| 2 | Usage, expression-file or compilation failure |

Earlier complete lines are flushed when later work fails. Inspect the exit status when
using output from a partial run.
