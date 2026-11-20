# Using the CLI

`jx` compiles one expression, then processes independent JSON values in order.
Input may be NDJSON, pretty-printed multi-line JSON, or concatenated JSON
values. Diagnostics go to stderr; JSON (or raw UTF-8 with `-r`) results go to
stdout. See [installation and releases](https://github.com/jo-cube/jx/blob/main/docs/releases.md)
for the verified Unix installer, Windows downloads and source builds.

```sh
printf '%s\n' '{"customer":{"id":42}}' | jx 'customer.id'
jx -r 'database.host' config.json
jx '{"id":id,"total":$sum(items[price>10].price)}' records.ndjson > results.ndjson
jx -f transform.jsonata --max-work 100000 first.ndjson - second.ndjson
jx -s '$sum($.amount)' orders.ndjson
```

No files, or a `-` argument, reads stdin. Files and stdin are processed in argument
order. By default the CLI stops on the first error and uses synchronous backpressure;
without `-s`, it never reads a complete file into memory.

## Expressions and input

```text
jx [OPTIONS] [--] EXPRESSION [FILE ...]
jx [OPTIONS] -f EXPRESSION_FILE [OPTIONS] [FILE ...]
```

Options may appear before the expression, and with `-f` may also follow the
expression file path before input file arguments. Use `--` before a leading `-`
expression or file name. File arguments after the expression are taken
literally. Source is UTF-8; file names use native platform paths. Expression
files can contain newlines and comments:

```text
/* transform.jsonata */
{
  "customer": customer.id,
  "total": $sum(items.(price * quantity))
}
```

Input may contain one or more UTF-8 JSON values separated by arbitrary
whitespace (including NDJSON, CRLF, pretty-printed multi-line documents, or
concatenated values). `--max-record-bytes` bounds each input JSON value (or the
combined array in `-s` mode) without reading unbounded input. Every record is
completely validated, including fields the expression does not use.

POSIX shells and PowerShell generally use single quotes around JSONata to protect `$`
and other operators. Complex programs are easier with `-f`, especially in `cmd.exe`
where `%` expands and quoting differs. `jx` writes UTF-8 directly; a shell's redirection
encoding is a separate concern.

## Result shape

| Result | Output |
| --- | --- |
| Missing | No output |
| Null or scalar | One JSON value (or unquoted UTF-8 string with `-r`) |
| JSONata sequence | One value per item, in order |
| Array or object | One JSON value, including empty containers |

`jx 'items.id'` can emit several values per record. Use `[items.id]` to construct one
array, or `items.id[]` to retain a sequence's singleton shape; missing still stays missing
with retention. These forms have different JSONata flattening rules.

Compact output can preserve numeric/escape spelling and duplicate keys. `$string` is a
separate language conversion, not a way to obtain byte-preserving CLI output.
Function values have no JSON encoding and cause serialization failure.

Each result finishes serialization in a reused bounded buffer before it is
written. Failure leaves no partial result; earlier complete results from the same
record or previous records remain. This is not a transaction over a record/file.
An underlying I/O error can interrupt a write. Input/result buffers are bounded, but
retained engine values can require additional memory.

## Options and limits

| Option | Behavior |
| --- | --- |
| `-f`, `--expression-file PATH` | Read the expression from a file |
| `--vars OBJECT` | Strict JSON object bound as `$vars` |
| `--vars-file PATH` | Read the strict JSON `$vars` object from a UTF-8 file |
| `-r`, `--raw-output` | Emit string results as unquoted UTF-8 text |
| `-p`, `--pretty` | Pretty-print JSON output with 2-space indentation |
| `-n`, `--null-input` | Evaluate once with absent input instead of reading input |
| `-s`, `--slurp` | Read all input JSON values into a single JSON array |
| `--check` | Validate CLI options, `$vars`, and expression, then exit |
| `--on-error MODE` | Record error policy: `fail` (default) or `skip` |
| `--max-record-bytes N` | Positive per-value (or slurped array) byte limit; default 1,048,576 |
| `--max-output-bytes N` | Positive per-result serialized byte limit; default 16,777,216 |
| `--max-work N` | Cooperative work budget plus other default evaluation limits, per input record |
| `-h`, `--help` | Print usage and options |
| `--version` | Print package version |

```sh
jx --max-record-bytes 10485760 --max-output-bytes 33554432 '$' large.ndjson
jx --max-work 100000 '$sum(items.price)' records.ndjson
jx -n --vars '{"tax":0.08}' '100 * (1 + $vars.tax)'
```

Work units are semantic checkpoints, not CPU instructions. Controls are cooperative;
validation finishes first and regex/host calls cannot be interrupted mid-call.
See [compatibility and resource limits](https://github.com/jo-cube/jx/blob/main/docs/compatibility.md).

## Errors and exit status

Diagnostics include expression location or file/line, phase and error kind.

| Status | Meaning |
| --- | --- |
| 0 | Success, including a downstream broken pipe |
| 1 | Input, evaluation, serialization or I/O failure |
| 2 | Usage, expression-file, `$vars`, or compilation failure |

Earlier complete outputs are flushed when later work fails. Inspect the exit status when
using output from a partial run. With `--on-error skip`, record validation, evaluation,
and serialization failures are logged to stderr and processing continues.
