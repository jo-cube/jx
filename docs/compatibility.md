# Compatibility and resource policy

`jx` supports the large majority of practical JSONata: paths/sequences, filters,
operators, constructors, lexical functions/signatures/partials/tail calls, grouping,
sorting, higher-order helpers, conversion, regex, numeric/date pictures, parent
navigation, transforms, `$eval` and randomness. Semantic authority is the
[official documentation](https://docs.jsonata.org/overview.html) and JSONata 2.2.0.

The complete pinned language corpus has 1,679 classified, asserted outcomes: **1,390
results, 288 mapped errors, zero blockers and one non-tail recursion guard**. Passing
that inventory does not establish compatibility with all JavaScript host behavior.
Detailed rules and regression fixtures remain in [CONFORMANCE.md](https://github.com/jo-cube/jx/blob/dev/CONFORMANCE.md).

Important boundaries beyond the corpus:

- Rust sources/field names are UTF-8; JSON string values preserve lone UTF-16 surrogate
  units, but decoded Rust `str` access rejects them. JSONata strings use UTF-16 indexing.
  Some legacy Unicode case mappings, surrogate regex patterns and picture rules remain
  explicitly deferred in the inventory.
- Numeric values use binary64. Raw input/output can preserve number spelling;
  constructed nonfinite numbers serialize as null. Duplicate decoded input keys use
  the last value. JavaScript prototype-related names are ordinary JSON keys.
- Picture/date parsing has documented legacy/locale/Unicode edge boundaries. Node's
  lossy base64 coercions and some untyped native partial coercions are not reproduced.
- Parent paths require statically derivable ancestry. Transforms use immutable selective
  copies; custom clone overrides and advanced JavaScript mutable identity are deferred.
- Host functions are synchronous and effectful. No async integration, parser recovery,
  detached JSONata closure transfer or exact upstream diagnostic-string contract exists.

Every record is completely validated, even if only one field is demanded. Missing,
null, arrays and sequences remain distinct. Streaming consumption may fail after
previous values; callers requiring atomic output should buffer each result as the CLI does.

## Limits

Always-on ceilings include JSON/expression depth **128**, at most **64** active calls
and **512** accumulated expression levels, and **one million** iterations per tail chain.
Ranges have a ten-million-item bound and finite safe-integer endpoints. Individual
string/regex/picture helpers apply local growth guards. Non-tail recursion is deliberately
bounded rather than risking Rust stack overflow; one factorial corpus case asserts this.

The ordinary library API has no input byte quota. Opt-in `EvaluationOptions` accepts
`Limits`, `Cancellation` and a monotonic deadline. Default `Limits` allow one million
work checkpoints, intermediate inspected/emitted items and results, **16 MiB** compact
result bytes across an evaluation, 64 calls and one million tail calls per chain.
Controls can tighten stack/tail ceilings. Work units are semantic checkpoints, not
stable CPU-instruction counts. Controlled planned regions use tree execution so checks
remain effective. No optional control state is allocated unless requested.

Cancellation/deadlines are cooperative. Validation completes first; a regex invocation,
format helper or noncooperating host callback cannot be interrupted mid-call. Output
preflight cannot undo allocations already made. Limits are not hard memory/CPU quotas;
applications accepting hostile code should bound input/source sizes and use process
isolation where necessary. [Embedding controls](https://github.com/jo-cube/jx/blob/dev/docs/embedding.md#limits-and-cancellation)
provide the precise public API contract.

The CLI separately bounds input records and serialized result lines. `--max-work`
enables the other default evaluation limits; these are per input record.
