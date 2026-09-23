# Architecture

## Current execution

`source → compiled path → validating selection → borrowed result stream`

- `parse.rs` owns field names once and retains the leading `$`, which affects
  root-array semantics. Unsupported syntax fails compilation.
- `json/scan.rs` validates UTF-8 and JSON, capturing object-only paths during the
  structural scan. At the first array requiring navigation it retains the raw
  array and remaining fields. Last decoded duplicate keys win before traversal.
- The same scanner supplies internal element and field cursors over validated
  subtrees. They reuse the grammar rather than building a DOM or a second parser.
  Escaped keys compare as UTF-16 units without allocating decoded strings.
- `evaluate.rs` implements field mapping and sequence normalization. Each array
  lookup retains at most one value until singleton versus multiple is known.
  The final stage similarly preserves a sole raw array or flattens combined
  results one level. Nested array values and sequences stay distinct.
- `Expression` is immutable and shareable. `evaluate(&[u8])` fully validates first,
  then returns an `Evaluation` borrowing expression and input. Consuming it with
  `for_each` or `try_for_each` streams `RawJson` items; emitted values borrow only
  input. Missing emits nothing; null emits once. Consumer errors stop traversal.
- `RawJson::write_compact` preserves token bytes. The CLI owns framing, limits,
  reusable buffers and synchronous I/O, emitting one NDJSON line per result item.
  The engine creates no threads.

Object-only evaluation remains one structural selection pass after UTF-8 validation.
Array traversal reuses scans to locate member/element boundaries and may rescan a
subtree at each enclosing container/path stage: worst-case O(bytes × input depth).
Working state is O(input depth), on the native stack; validation caps containers
at 128. Evaluation and emission need no heap allocation, independent of array width.
There is no DOM, tape, general field index, result collection, execution IR or JIT.

## Decisions and limits

A borrowed raw value directly serves identity and subtree projection. It is not a
universal future value model: arithmetic will need primitive scalars, and retained
or constructed values will need ownership. Keep those internal decisions separate
from public API stability; breaking the provisional API is allowed.

A custom safe scanner makes full validation and selective capture one structural
pass. A DOM would allocate unused data; a validate-then-lookup design would rescan
it. This small scanner carries a testing obligation: syntax tests, deterministic
mutations against a separate parser, and explicit Unicode/number policies.
SIMD and alternative parsers require profiling evidence, not assumptions.

A fallible callback uses bounded native traversal state and makes cancellation
explicit. A lazy iterator would need to store suspended recursive path state;
collecting into a Vec would retain every result. The provisional API now uses
callbacks because both alternatives add cost or machinery unnecessary for this
milestone. This is a path-result stream, not a universal future value hierarchy.

Keep the scanner reuse as the correctness baseline. Measure redundant scanning
on array workloads before adding trusted skipping, capture fusion or indexes.
Filters and aggregates can consume the stream; constructors can collect or own
only what their semantics require.

## Milestones

1. **Complete:** identity/object paths, UTF-8 JSON validation, borrowed evaluation,
   bounded NDJSON CLI, explicit limitations, pinned conformance and initial benchmarks.
2. **Complete:** root/intermediate/nested array navigation, normalized sequences,
   raw-array preservation, fallible borrowed consumption, pinned flattening cases
   and array benchmarks. Leading `$` remains semantically significant at the root.
3. **Next: scalars and filters.** Add a small semantic tree, numeric/boolean/string
   operations, precedence, index/predicate semantics and aggregates. Determine
   primitive and sequence representations from executable cases. Fuse field demands
   or iteration only where benchmarks show material repeated work.
4. **Construction and functions.** Add owned output, constructors, bindings,
   closures and standard functions incrementally; track remaining language families.
   Allow a general fallback where dynamic behavior requires it.
5. **Evidence-driven compilation.** Introduce normalization or execution IR only
   when it simplifies implemented semantics or measured execution. Decide on JIT
   only after profiling mature repeated execution; no current commitment.

Each milestone updates conformance, tests and representative benchmarks. The full
language goal does not require every expression to take the same execution path.
