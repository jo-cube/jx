# Architecture

## Current execution

`source → small expression tree → validating selection or scalar execution → result stream`

- `parse/lex.rs` and `parse.rs` own tokenization, precedence and grouping. Field
  names and encoded string literals are owned once; operators retain source offsets.
  Parser nesting and tree depth are capped at 128, including flat operator chains.
  `expression.rs` holds only implemented paths, literals and unary/binary operations.
- A top-level path still uses `json/scan.rs` to capture object paths during full
  validation. The first array needing navigation retains its raw range and remaining
  fields. Last decoded duplicate keys win before traversal.
- `path.rs` maps fields through arrays and normalizes sequence cardinality. Each
  lookup retains at most one pending value. A sole final raw array is preserved;
  combined results flatten one level. Leading `$` matters for root-array mapping.
- Scalar trees validate the record first, then execute directly in `evaluate.rs`.
  Path operands use borrowed field/element cursors over validated input. A stream
  is inspected up to its second item to distinguish missing, singleton and multiple;
  multiple items remain a replayable path, not an eagerly collected vector.
- Numbers use binary64, booleans/null are primitives, and strings retain validated
  JSON encodings. Escapes and ordering compare as UTF-16 units without allocating
  decoded strings. Raw paths preserve all original number/string tokens.
- `compare.rs` handles typed ordering and structural equality. Sequence-to-array
  equality streams both sides. Comparing two push streams retains one side's raw
  slices. Object equality uses a temporary map of borrowed left members and applies
  last-key-wins on both sides. These allocations are confined to equality.
- `Value` is the small public output union: raw input, primitive scalars or a
  compiled string literal. Its two lifetimes distinguish input from expression
  storage. `as_raw()` extracts input slices that can outlive the expression.
- `Expression` is immutable/shareable. `evaluate(&[u8])` returns JSON and scalar
  errors before consumption. `for_each`/`try_for_each` stream output; consumer errors
  stop immediately. Missing emits nothing, null emits once, and arrays remain values.
  The CLI owns NDJSON, limits, reused buffers and synchronous I/O, one line per item.

## Decisions and measured limits

The tree exists because precedence, short-circuiting and typed operators now need
structure. There is no execution IR, JIT, generic function/value framework or DOM.
No constant folding or field-demand fusion yet: neither is needed for correctness,
and the benchmark suite now exposes their potential value.

The custom safe scanner combines validation and selective capture for standalone
paths. Element/member cursors reuse its grammar instead of adding a trusted second
parser. Validation covers every byte, including unselected fields, before any output.
JSON container depth is capped at 128; all production Rust forbids unsafe.

Callbacks keep traversal state on the bounded native stack. Suspending two callbacks
for ordered equality would require iterator machinery; retaining one side only in
that operation is simpler. Object equality initially rescanned objects per key;
measuring 128-field objects justified a borrowed-member map confined to that case.
Neither choice imposes allocation on ordinary paths or scalar operators.

Array traversal can rescan a subtree at each nesting/path level: worst-case
O(bytes × input depth). Scalar operands also rescan demanded paths separately after
validation. The benchmarks retain deep arrays, tiny records, multi-field scalars
and structural equality so these costs remain visible. Add capture fusion, indexes,
trusted skipping or specialization only for a measured benefit with a simple design.

## Milestones

1. **Complete:** identity/object paths, UTF-8 JSON validation, borrowed evaluation,
   bounded NDJSON CLI, pinned conformance and initial benchmarks.
2. **Complete:** array navigation, normalized sequences, raw-array preservation,
   borrowed callbacks and flattening benchmarks.
3. **Complete:** scalar literals, arithmetic, equality/ordering, boolean logic,
   unary minus and grouping; primitive output, explicit runtime errors, scalar
   differential/conformance tests and performance evidence.
4. **Next: filters and aggregates.** Add numeric index and predicate semantics,
   then selected aggregates consuming sequences. Freeze context, cardinality and
   predicate-position rules first. Avoid collecting where iteration suffices.
5. **Construction and functions.** Add owned output, constructors, bindings,
   closures and standard functions incrementally; a general fallback may serve
   dynamic semantics without burdening common cases.
6. **Evidence-driven compilation.** Introduce normalization/IR only when it
   simplifies implemented semantics or measured execution. JIT remains undecided.

Each milestone updates conformance, tests and representative benchmarks. Full
language support does not require every expression to use the same execution path.
