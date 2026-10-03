# Compatibility and resource limits

The authority is the [official JSONata language](https://docs.jsonata.org/overview.html)
and JSONata 2.2.0. Supported capabilities include paths/sequences, filters, operators,
constructors, lexical functions/signatures/partials/tail calls, sorting/grouping and
tuples, parent navigation, transforms, regex/matchers, numeric/date pictures, higher-order
helpers, `$eval` and randomness.

The complete pinned language corpus has **1,679 asserted outcomes**: 1,390 results,
288 mapped errors, one non-tail recursion limit and zero blockers. This does not establish
compatibility with every JavaScript host behavior. [Conformance](https://github.com/jo-cube/jx/blob/dev/CONFORMANCE.md)
describes the inventory and regression checks; the boundaries below also cover behavior
outside that corpus.

## JSON, strings and sequences

- Input is exactly one complete UTF-8 JSON value. All bytes are validated; BOM and
  trailing content are rejected. Duplicate decoded keys use the last value for lookup.
- Raw output preserves number/escape spelling and duplicate keys; it is not canonical
  JavaScript parse/stringify output. Demanded numeric operations use binary64. Constructed
  nonfinite numbers serialize as null. `$string` has its own JSONata conversion/rounding rules.
- JSON string values preserve isolated UTF-16 surrogate units. Decoded Rust `str` access
  rejects them; `string_units` and owned snapshots retain them. Expression source/field
  names are UTF-8, so isolated-surrogate selectors and dynamic source are unsupported.
- String comparison and regex indexes use UTF-16 units. `$length` and `$substring` count
  codepoints; empty-separator `$split` uses UTF-16 units.
- Missing emits no item; null is one item; an explicit array is one value. Paths apply
  JSONata sequence flattening, not recursive flattening of every array. Retention (`[]`)
  keeps sequence shape, but cannot restore an already-normalized singleton. Small cases
  in the [sequence corpus](https://github.com/jo-cube/jx/blob/dev/tests/semantics/sequences.json)
  specify the less obvious array/empty boundaries.

Prototype-related names such as `__proto__` are ordinary JSON keys. Construction does
not mutate an empty root array. Grouping an empty tuple stream returns `{}` rather than
an uncoded JavaScript exception. Implicit string iteration after a collapsed leading
array constructor (`["ab"][0].$`) is unsupported; normal parenthesized navigation works.

## Functions, effects and embedding

Bindings retain evaluated values once. Functions capture lexical frames and focus;
finite tail calls use bounded stack execution. Return signatures and function subtype
signatures are descriptive; array subtype checks follow the reference's shallow rule.
The documented primitive `u` signature is supported even though the pinned parser omits it.
Unscoped assignments shared across constructor members are rejected; use member-local
blocks rather than relying on reference asynchronous races.

Ordinary builtin calls use strict signatures. Signature-bypassing native partials have
only explicitly supported coercions: numeric primitives/arrays are supported for
`$abs`, `$floor`, `$ceil`, `$sqrt`, `$power`; object/function coercion and remaining
native bypasses are unsupported. `$string` partials are unsupported; use it as a callback
or wrap it in a lambda. Untyped `$shuffle` partials require array inputs.

`$eval` inherits bindings, exports assignments and can replace focus while preserving
`$$`. Errors retain outer and inner diagnostics. Randomness is lazy, noncryptographic
and never folded/replayed; deterministic injection controls draws, not compatibility
with another engine's PRNG.

Host functions are synchronous and effectful. JSONata closures cannot transfer between
evaluations or detach into owned snapshots; independently owned host functions can.
There is no async integration, parser recovery or exact upstream diagnostic-text contract.
See the [embedding guide](https://github.com/jo-cube/jx/blob/dev/docs/embedding.md).

## Parent navigation and transforms

Parents require statically derivable ancestry. Derivation through variables, descendants,
constructors or sorted stages fails compilation, as in the reference. Function bodies
cannot resolve parents against an outside path; capture an explicit parent binding first.

Transforms clone the selected argument, then update/delete with selective immutable
copies. Duplicate/overlapping selections observe prior updates. Unrelated leaves can
stay borrowed. Custom `$clone` overrides, locations outside the cloned argument,
unscoped pattern/update bindings, function-valued updates, cyclic output and advanced
mutable JavaScript aliasing are unsupported. `$clone` performs language conversion,
including number rounding and undefined/function handling, rather than a raw byte copy.

## Regex, encoding and pictures

Regex literals compile once and support ECMAScript captures, lookarounds and
backreferences with `i`/`m` flags. Matchers have shared cursor state and retained subjects;
empty continuation matches raise `RegexError`. Regex calls require strings/numeric
offsets; JavaScript coercion and literal-string replacement by a function are unsupported.
Dynamic regex construction, extra engine flags and interruptible regex timeouts are absent.

Legacy case-insensitive folding differs for U+0131/U+017F and Greek extended aliases
U+1F80–1F87, U+1F90–1F97, U+1FA0–1FA7, U+1FB3/U+1FC3/U+1FF3. Patterns containing
these units are rejected under `/i`; matching subjects containing them is unsupported.
The same boundary applies to case-insensitive date pictures/subjects. Other Unicode
case-insensitive regex behavior and case-sensitive surrogate patterns are supported.

URI helpers use strict UTF-8/reserved-character rules. Base64 follows Node's binary-string
(low UTF-16 byte / Latin1) convention, not UTF-8. Non-Latin1 base64 decoding is unsupported
because V8 string storage changes its behavior.

Numeric pictures cover grouping, positive/negative forms, percent/per-mille, exponents
and custom options. Integer pictures cover decimal digit families, letters, Roman
numerals, English words and ordinals. Dates support components, names, widths, timezones,
week fields and pictured parsing. Static pictures are prepared once; dynamic calls may
parse at evaluation.

Remaining picture/date boundaries: host-local ISO timestamps without a timezone,
legacy fractional date-only syntax, lone-surrogate picture/subject text, multi-unit
decimal symbols, fixed integer/decimal/radix output at absolute values ≥10²¹,
nonfinite exponent/word/infinite alphabetic or Roman output, and legacy week/day derivation
in years 0–99. Fraction precision is at most 100. Pinned quirks such as negative timezone
arithmetic and pictured years 0–99 remain tested reference behavior.

## Resource policy

Always-on ceilings:

| Resource | Ceiling |
| --- | ---: |
| JSON/expression nesting | 128 |
| Active non-tail calls | 64 |
| Accumulated function expression levels | 512 |
| Iterations per tail chain | 1,000,000 |
| Range width | 10,000,000, with finite safe-integer endpoints |
| Transform array growth / added padding codepoints | 1,000,000 |

Individual regex/picture/string helpers have additional local growth guards. Non-tail
recursion is bounded rather than risking Rust stack overflow. The library has no input
byte quota; callers must bound input/source sizes.

Optional `EvaluationOptions` accepts `Limits`, `Cancellation` and a monotonic deadline.
Default `Limits` permit one million work checkpoints, intermediate inspected/emitted
items and results, 16 MiB of compact result bytes across an evaluation, 64 calls and
one million tail iterations. Stack/tail controls can tighten hard ceilings. Work counts
are semantic checkpoints, not stable CPU-instruction counts. Controlled planned regions
use tree execution where needed. Unused optional controls allocate no control state.

Validation completes before effects/control failures. Cancellation/deadlines are
cooperative: regex, formatting and noncooperating host code cannot be interrupted
mid-call. Output preflight cannot undo construction already performed. These are not
hard memory/CPU quotas or a sandbox for hostile code. The CLI separately bounds records
and result lines; `--max-work` enables the other default limits per record.
