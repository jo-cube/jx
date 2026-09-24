# JSONata compatibility

Semantic authority: [official language documentation](https://docs.jsonata.org/overview.html)
and the [upstream suite](https://github.com/jsonata-js/jsonata/tree/v2.2.0/test/test-suite).
This is an early subset, not a full JSONata implementation. Errors use local
`ErrorKind` values and byte offsets; upstream diagnostic codes are deferred.

| Area | Current behavior / status |
| --- | --- |
| Identity | `$` returns the complete input value |
| Field paths | `a`, `a.b`, `$.a.b`; ASCII names `[A-Za-z_][A-Za-z0-9_]*` |
| Quoted fields | Backtick-delimited UTF-8 names, including empty names; no escape interpretation in the expression |
| Missing | Zero results; different from JSON null |
| Null/scalar context | Further field lookup yields missing |
| Raw arrays/objects | Borrowed values; path mapping applies the cardinality rules below |
| Duplicate input keys | Last decoded matching key wins for lookup |
| Root/intermediate/nested array mapping | Supported, input order; missing/scalar contexts drop out |
| Result sequences / flattening | Supported for paths and filters; streamed without collecting |
| Indexes / predicates | Literal/computed indexes, numeric lists from input, effective-boolean predicates, chained filters |
| Wildcards, parent/descendant, order/group/join | Unsupported syntax; incremental follow-up |
| Literals | Binary64 numbers, booleans, null, single/double quoted strings |
| Operators | `+ - * / %`, `= != < <= > >=`, `and or`, unary `-` |
| Parentheses | Expression grouping and grouped path steps; `()` is missing; multi-expression blocks deferred |
| Other operators | `in`, `&`, conditionals, ranges, coalescing/default and assignment deferred |
| Aggregates | Direct `$count`, `$sum`, `$min`, `$max` calls; streamed arguments, scalar results |
| Constructors, variables, general functions, closures, transforms, regex, other standard functions | Unsupported syntax; later milestones |
| Comments, general unquoted Unicode names, single/double quoted selectors | Deferred syntax; compile error |
| Keyword field names | `and`/`or` can be names in operand/field positions; `true`, `false`, `null`, `in`, `function` require backticks when used as fields |

## Array and sequence boundaries

A sequence is zero or more result items, not a raw JSON array. No items means
missing, one means that value, and multiple correspond to a JSON array at the
JSONata serialization boundary. The CLI instead writes each item on its own line.
A raw array is one item, including an empty array; it is not recursively flattened.

Field lookup through an array concatenates lookup results. A map stage with exactly
one defined raw-array result preserves it; multiple defined results contribute
array contents one level. An empty raw array counts as a defined result, while a
missing field does not. Nested array contexts have their own lookup normalization.
Leading `$` introduces a map stage, so `a` and `$.a` differ on root arrays.

| Expression | Input | Emitted items (separate values) |
| --- | --- | --- |
| `$` | `[[1],[2]]` | `[[1],[2]]` |
| `a` | `{"a":[1]}` | `[1]` |
| `a` | `[{"a":[1]}]` | `1` |
| `$.a` | `[{"a":[1]}]` | `[1]` |
| `a.b` | `{"a":[{"b":[]},{}]}` | `[]` |
| `a.b` | `{"a":[{"b":[]},{"b":[]}]}` | none |
| `a.b` | `{"a":[{"b":[1]},{"b":[2]}]}` | `1`, `2` |
| `a.b` | `{"a":[{"b":[[1]]},{"b":[[2]]}]}` | `[1]`, `[2]` |

[The readable sequence corpus](tests/semantics/sequences.json) freezes 42 cases,
including nested lookup, singleton and empty boundaries. Borrowing, duplicate
keys, cancellation, depth limits and CLI line framing have separate Rust tests.

## Filters

Filters bind to the preceding step: `a.b[0]` selects the first `b` in each `a`
context; `(a.b)[0]` selects from the combined result. `$` inside a predicate is the
candidate, and ordinary names look up from it. Order is preserved. Filters use the
same scalar operators, short-circuiting and effective-boolean rules described below.

Numbers mean zero-based positions, floored toward negative infinity; negatives count
from the end. A numeric array/sequence selects matching positions (including duplicate
matches); mixed arrays use effective boolean value. Empty candidates yield missing.
Non-numeric predicates use effective boolean value; a numeric `0` selects position
zero rather than rejecting the candidate. Numeric lists can come from input/paths;
array/range construction and `[]` singleton-array retention remain deferred syntax.

Chained filters retain stage shape until the enclosing expression normalizes it.
The upstream literal-index rule preserves a selected raw array; a computed index
produces a sequence containing that array. These deliberately differ:

| Expression | Input | Emitted items |
| --- | --- | --- |
| `a[0][0]` | `{"a":[[1,2],[3]]}` | `1` |
| `a[0+0][0]` | same | `[1,2]` |
| `(a[0+0])[0]` | same | `1` |
| `a[true]` | same | `[1,2]`, `[3]` |
| `a[true]` | `{}` | none |
| `a[true]` | `{"a":null}` | `null` |
| `a.b[true]` | `{"a":[{},{"b":1}]}` | undefined, `1` |

A missing step can still supply an undefined candidate to its predicate: `a[1+null]`
on `{}` raises a type error. One undefined result normalizes to missing. Undefined
retained in a multi-item sequence is exposed as `Value::Undefined`; compact JSON
output writes it as `null`, without making it equal to JSON null internally.

[88 readable filter cases](tests/semantics/filters.json) cover these boundaries.
Additional Rust regressions cover borrowing, cancellation, depth and error precedence.
Full JSON validation always precedes output. Predicate errors may occur during
consumption after earlier items were emitted; there is no per-record rollback.
Earlier-stage evaluation errors take precedence over later-stage errors on complete
consumption. Consumer cancellation deliberately stops further semantic evaluation.

## Aggregates

Direct `$count(expr)`, `$sum(expr)`, `$min(expr)` and `$max(expr)` calls compose with
paths, filters, operators and other aggregate calls. `orders.$sum(price)` aggregates
in each candidate context; `$sum(orders.price)` aggregates the combined result.
These are fixed built-ins; function references, dynamic calls, partial application,
chaining (`~>`) and other functions remain unsupported.

| Normalized argument | `$count` | `$sum` | `$min` / `$max` |
| --- | --- | --- | --- |
| Missing (including no filter matches) | `0` | Missing | Missing |
| Empty raw array | `0` | `0` | Missing |
| Number | `1` | That number (added to zero) | That number |
| Null, boolean, string, object | `1` | Type error | Type error |
| Array / multi-item sequence | Number of members | Sum of numeric members | Numeric minimum / maximum |

Numeric aggregates do not coerce strings, booleans or null and do not recursively
flatten nested arrays. Argument normalization still matters: on `{"a":[[1,2]]}`,
`$sum(a)` is a type error, while `$sum(a[true])` returns `3` because the singleton
result normalizes to its raw array. Undefined retained in a multi-item sequence
counts as a member and fails numeric aggregate type checks; singleton undefined
normalizes to missing. Input order determines binary64 sum rounding.

Numeric signature checks accept infinity and NaN, matching the pinned implementation;
min/max propagate NaN and preserve the reference's signed-zero ordering. Existing
non-finite serialization and scalar-operator rules still apply. Arguments finish
before type/arity checks, so earlier-stage evaluation errors take precedence.
A fold emits no partial value; cancellation can stop between mapped aggregate
results, but cannot interrupt an aggregate whose result is not yet available.

All four functions require exactly one argument; bad arity raises runtime `TypeError`.
The [array-function docs](https://docs.jsonata.org/array-functions) describe a context
fallback for `$count()`, but pinned JSONata 2.2.0 rejects it (`T0410`). We follow the
pinned implementation and test this discrepancy. Use `$count($)` explicitly.
[Numeric aggregate docs](https://docs.jsonata.org/aggregation-functions) and
[92 readable cases](tests/semantics/aggregates.json) define the remaining behavior;
Rust regressions cover error precedence, validation, cancellation and numeric bits.

## JSON boundary policies

- Exactly one complete UTF-8 JSON value, with standard JSON whitespace. Validate
  every byte, including unselected fields and trailing content. BOM is rejected.
- Maximum 128 nested containers; exceeding it returns `DepthLimit`. The library
  has no byte-size limit; the CLI has a configurable record-size limit.
- All syntactically valid JSON numbers are accepted and preserved, including
  integers outside binary64 precision and large exponents. Scalar operators convert
  demanded numbers to binary64; untouched raw values retain their tokens. See below.
- `\uXXXX` escapes permit lone surrogate units, as JSON grammar and the reference
  parser do. Raw values preserve them; field matching compares UTF-16 units.
  Expression field names are Rust UTF-8 and cannot denote an isolated surrogate.
- Serialization is compact but token-preserving. Identity/subtree output retains
  duplicate object keys and original escapes/numbers; it does not canonicalize
  like JavaScript parse/stringify. Lookups still use last-wins semantics. This is
  a deliberate raw JSON boundary policy, not a claim of byte-equivalent JS output.

## Scalar semantics

Precedence follows JSONata: paths/unary minus, multiplicative, additive, comparison,
`and`, then `or`. Binary operators associate left. `and`/`or` short-circuit the RHS;
truth conversion handles missing, null, primitives, nested arrays and objects.
Arrays/sequences are true if any member is true; empty objects/arrays are false.
Boolean NOT is a function (`$not`), still deferred; there is no `!`
operator or unary `+`. String concatenation and implicit string-to-number coercion
are not implemented.

| Operands | Arithmetic / unary minus | Ordering | `=` / `!=` |
| --- | --- | --- | --- |
| Missing | Missing, after checking other operand types | Missing, after type checks | Both false |
| Null | Type error | Type error | Equals only null |
| Number | Binary64 arithmetic | Same-type comparison | Numeric value |
| String | Type error | UTF-16 lexical order; types must match | Decoded UTF-16 units |
| Boolean | Type error | Type error | Same-type value |
| Raw array / object | Type error | Type error | Structural equality, ordered arrays / unordered object keys |
| Singleton result sequence | Its one value | Its one value | Its one value |
| Multi-item result sequence | Type error | Type error | Compared as an ordered array |

A raw `[1]` stays an array; a singleton path sequence containing `1` is a number.
Equality does not coerce types. Duplicate object keys use the last decoded key.
Arithmetic evaluates both sides; boolean short-circuiting does not bypass full
input validation or compile-time syntax checks.

Binary64 rounding follows the reference; division/remainder by zero and overflow
can produce infinity/NaN. Computed non-finite output serializes as JSON `null`,
but remains numeric internally: using infinity as an arithmetic/boolean operand
returns `NumericRange`; NaN is nonnumeric for arithmetic and false for booleans.
Out-of-range numeric literals fail compilation. Comparisons preserve IEEE NaN
behavior. Computed finite numbers use Rust's shortest round-trip formatting, which
can differ textually from JavaScript; negative zero serializes as `0`.

`TypeError`/`NumericRange` report the operator's expression byte offset. Expression whitespace is space, tab, LF, CR or vertical tab; form feed is rejected.
Parser nesting and expression-tree depth are capped at 128 (`DepthLimit`). Upstream error
codes/text are not a stable API. [100 readable scalar cases](tests/semantics/scalars.json)
and separate regression tests freeze these rules, validation order, depth limits,
Unicode, binary64 boundaries and CLI runtime errors.

## Executable coverage

`just conformance` executes all **298** imported cases from complete `fields`,
`missing-paths`, `quoted-selectors`, `flattening`, `numeric-operators`,
`comparison-operators`, `boolean-expresssions`, `literals`, `null`, `parentheses`,
`predicates`, `simple-array-selectors`, `multiple-array-selectors`,
`function-count`, `function-sum` and `function-max` (also containing min cases)
groups of JSONata **2.2.0**, revision
`8ee4476f8a228bfc7a62979ae0a9c13a4043cd03`:

| Classification | Cases | Assertion |
| --- | ---: | --- |
| Supported results | 175 | Semantic JSON result or missing matches upstream |
| Supported errors | 13 | Asserted compile/evaluate phase and mapped local error kind |
| Deferred syntax | 110 | Exactly `UnsupportedExpression` |

These are selected groups, not a percentage of the full suite. No imported case
is skipped. `tests/conformance/manifest.json` lists every case and reason; the
runner fails on unclassified files or changed behavior. Promote status alongside
implementation. Unimported language families remain deferred as listed above.

Upstream separates reusable `datasets/` from `groups/<topic>/caseNNN.json`, with
`expr`, `data`/`dataset`, bindings, and expected result/undefined/error fields.
The general suite also has expression files and multi-case files; the current
adapter handles inline/named data and multi-case files. Expression-file cases,
host functions and nonempty bindings remain deferred.
Original files and MIT notice are preserved under `tests/conformance/`.

Reimport from a checkout at the pinned revision:

```sh
git clone --depth 1 --branch v2.2.0 https://github.com/jsonata-js/jsonata.git /tmp/jsonata-reference
python3 scripts/import-conformance.py /tmp/jsonata-reference
just conformance
```

Independent tests separate JSON syntax/mutations, engine semantics and CLI framing.
Regression cases should be minimal and added before the fix. Ordinary checks are
offline once Cargo dependencies are cached; Node and an upstream checkout are not
required to run them.

Optional differential check (requires Node and the pinned upstream checkout):

```sh
just build
node scripts/check-sequences.cjs /tmp/jsonata-reference target/release/jx
node scripts/check-scalars.cjs /tmp/jsonata-reference target/release/jx
node scripts/check-filters.cjs /tmp/jsonata-reference target/release/jx
node scripts/check-aggregates.cjs /tmp/jsonata-reference target/release/jx
```

It checks the 42 readable cases and 5,894 deterministic generated/curated path
evaluations against the reference stream. The scalar check adds 1,936 curated and
seeded generated evaluations, including runtime error kinds and raw numeric/Unicode
boundaries. The filter check adds 3,820 cases, including nested contexts, chained
positions, numeric lists and stage error precedence. The aggregate check adds 4,948
evaluations across raw arrays, normalized sequences, filters, nested calls and
numeric/error boundaries. Normal `just all` needs neither Node nor the upstream checkout.
