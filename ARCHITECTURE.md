# Architecture

## Current execution

`source → compiled field names → validating selection → borrowed result iterator`

- `parse.rs` recognizes the supported path grammar and owns names once. An empty
  field list represents `$`. Unsupported syntax fails compilation.
- `json/scan.rs` validates JSON and selects an object path in the same structural
  walk. A separate standard-library UTF-8 check precedes it. Unselected bytes
  are always validated. Each selected duplicate field replaces the previous
  selection, including missing or deferred array traversal.
- `json/string.rs` compares ordinary keys directly and escaped keys as decoded
  UTF-16 units, without building strings. Raw strings and numeric spellings remain
  unchanged. No numeric computation exists yet.
- `Expression` is immutable and shareable. `evaluate(&[u8])` returns an opaque
  `Evaluation` iterator whose items are `RawJson` slices. Today it yields zero or
  one item; a JSON array is one value. Input errors precede all result exposure.
- `RawJson::write_compact` strips structural whitespace and preserves token bytes.
  The CLI owns framing, byte limits, file ordering, reusable buffers and I/O errors.
  Backpressure is synchronous; the engine creates no threads.

Validation is O(input bytes + compared key bytes), with O(container depth) native
stack space, capped at 128 containers. Path selection does not rescan subtrees.
Ordinary evaluation allocates no heap memory. There is no tape, field index, DOM,
cache, boxed runtime value hierarchy, execution IR, or JIT infrastructure.

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

Object-only paths deliberately avoid deciding sequence representation now. Array
traversal records a pending unsupported result, finishes validation, and reports
it only if no later duplicate replaces it. Selecting an array itself is supported.
The iterator is useful to record consumers today without inventing generic streams,
closures or an execution framework before their semantics exist.

## Milestones

1. **Complete:** identity/object paths, UTF-8 JSON validation, borrowed evaluation,
   bounded NDJSON CLI, explicit limitations, pinned conformance and initial benchmarks.
2. **Next: array navigation and sequences.** Support root/intermediate/nested arrays
   for existing field paths; distinguish missing, singleton sequences, multiple
   sequences and JSON arrays using upstream results. Promote the six imported
   array cases to supported. Add a focused cardinality/nesting corpus before code.
   Validate before exposing output; support consumer cancellation and bounded
   traversal state. Define CLI sequence output explicitly (initial intent: one
   NDJSON value per emitted result) and test it. Preserve object-path benchmarks.
3. **Scalars and filters.** Add a small semantic tree, numeric/boolean/string
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
