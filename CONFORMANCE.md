# JSONata compatibility

Semantic authority: [official language documentation](https://docs.jsonata.org/overview.html)
and the [upstream suite](https://github.com/jsonata-js/jsonata/tree/v2.2.0/test/test-suite).
This is an early subset, not a full JSONata implementation. Errors use local
`ErrorKind` values and byte offsets; upstream diagnostic codes are deferred.

| Area | Current behavior / status |
| --- | --- |
| Context | `$` is current context; `$$` initially references the root record |
| Field paths | `a`, `a.b`, `$.a.b`; ASCII names `[A-Za-z_][A-Za-z0-9_]*` |
| Quoted fields | Backtick-delimited UTF-8 names, including empty names; no escape interpretation in the expression |
| Missing | Zero results; different from JSON null |
| Null/scalar context | Further field lookup yields missing |
| Raw arrays/objects | Borrowed values; path mapping applies the cardinality rules below |
| Duplicate input keys | Last decoded matching key wins for lookup |
| Root/intermediate/nested array mapping | Supported, input order; missing/scalar contexts drop out |
| Result sequences / flattening | Supported for paths and filters; streamed without collecting |
| Indexes / predicates | Literal/computed indexes, numeric lists from input, effective-boolean predicates, chained filters |
| Wildcards / descendants | `*` and `**`, including composed paths, predicates and aggregates |
| Ordering / grouping | Stable `^(<key, >key)` and postfix `{key:value}` |
| Parent/context/index tuples | `%`, `@` and `#` navigation remain deferred together |
| Literals | Binary64 numbers, booleans, null, single/double quoted strings |
| Operators | `+ - * / %`, `= != < <= > >=`, `and or`, unary `-` |
| Parentheses | Expression grouping, lexical blocks and grouped path steps; `()` is missing |
| Other operators | `in`, array ranges, `??`, `?:`, `:=`, `? :`; concatenation `&` deferred |
| Aggregates | `$count`, `$sum`, `$min`, `$max`; direct streaming folds and first-class calls |
| Constructors | Arrays, objects, computed keys/values, nested and mapped construction; see below |
| Singleton retention | `expr[]` preserves sequence shape; missing stays missing |
| Lexical runtime | Variables, bindings, blocks, conditionals, lambdas (`function` / `λ`), calls, closures and higher-order values |
| Builtins | Aggregates, `$boolean`, `$not`, `$exists`, `$lookup`; others fail explicitly when called |
| Deferred runtime/language | Signatures, tail-call elimination, partial application, chaining, transforms and regex |
| Quoted selectors | Single/double quoted strings become field names in dotted paths; escapes decoded; lone-surrogate field names deferred |
| Comments, general unquoted Unicode names | Deferred syntax; compile error |
| Keyword field names | `and`/`or`/`in` can be names in operand/field positions; `true`, `false`, `null`, `function` require backticks when used as fields |

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

## Navigation, retention and reduction

`[]` marks a sequence boundary rather than constructing an array around every value.
`a.b[]` keeps a singleton as an array; `(a.b)[]` cannot restore a singleton already
normalized by parentheses. Missing still emits nothing. Filters can retain an undefined
singleton (`missing[true][]` emits `[null]`). Kept sequences flatten during navigation,
but emit as one array at the API/CLI boundary. Explicit arrays retain their own shape.

Wildcards enumerate immediate members and recursively flatten array-valued members.
The pinned implementation returns an array value if any such member is an array,
even an empty one; otherwise it returns a sequence. Descendants include the starting
non-array value and visit descendants depth-first; array containers themselves are
not emitted. Raw duplicate keys use their last value in first-key order; integer
keys precede other keys as in the reference. Leaves still borrow input bytes.

Ranges occur inside array constructors (`[start..end]`), with inclusive integer endpoints.
Missing endpoints or descending bounds contribute no items; types are checked before
missing propagates. Width is limited to 10 million, matching upstream. Endpoints beyond
±(2^53−1) raise `NumericRange`, avoiding non-progressing binary64 increments.

Postfix grouping groups the whole unparenthesized path, including later steps/filters;
use `(items{key:value}).field` to navigate the constructed object. Equal keys from one
member group contexts; different members still raise `DuplicateKey`. Integer group
keys also precede other keys when evaluating values and choosing error precedence. Undefined group
contexts are ignored by append, without turning an all-undefined group into an empty array.
Ordering is stable, supports ascending/descending terms, and places missing keys last
in either direction. Keys must be numbers or strings of matching types when compared.
Singletons require no comparisons, so their key expression is not evaluated.

`in` uses strict scalar equality and container identity, unlike structural `=`.
`??` checks existence, while `?:` checks effective boolean value. Both follow upstream's
conditional expansion and re-evaluate a selected left expression; `??` calls the
ordinary shadowable `$exists`. RHS evaluation is conditional.

[Readable navigation cases](tests/semantics/navigation.json) and Rust/CLI tests freeze
these rules, validation, borrowed output, cancellation and resource limits. `%` parent
navigation and `@`/`#` tuple bindings remain unsupported; ancestry needs to survive
reduction stages. String concatenation/conversion, chaining and general builtins remain deferred.

## Filters

Filters bind to the preceding step: `a.b[0]` selects the first `b` in each `a`
context; `(a.b)[0]` selects from the combined result. `$` inside a predicate is the
candidate, and ordinary names look up from it. Order is preserved. Filters use the
same scalar operators, short-circuiting and effective-boolean rules described below.

Numbers mean zero-based positions, floored toward negative infinity; negatives count
from the end. A numeric array/sequence selects matching positions (including duplicate
matches); mixed arrays use effective boolean value. Empty candidates yield missing.
Non-numeric predicates use effective boolean value; a numeric `0` selects position
zero rather than rejecting the candidate. Numeric lists can come from input/paths and
can also be constructed inline (`a[[0,2]]`, `a[[1..3]]`). Bare `a[1..3]` is invalid;
the range belongs inside an array constructor.

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
These builtins also support references and dynamic calls. Partial application and
chaining (`~>`) remain deferred. Statically unshadowed direct calls stream their arguments.

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

## Constructors

Array and object construction follows the [result structure documentation](https://docs.jsonata.org/construction)
and [sequence rules](https://docs.jsonata.org/processing). Computed members use the
same operators, filters, paths and four aggregates. Constructed containers own their
structure; input leaves still borrow their original bytes. Navigation, equality,
truth conversion and aggregation accept constructed values without serializing them.

| Expression | Input | Emitted items |
| --- | --- | --- |
| `[missing,null]` | `{}` | `[null]` |
| `[1,[2,3],([4,5])]` | `{}` | `[1,[2,3],4,5]` |
| `{"gone":missing,"nil":null,"empty":[]}` | `{}` | `{"nil":null,"empty":[]}` |
| `a.[b]` | `{"a":[{"b":1},{"b":2}]}` | `[1]`, `[2]` |
| `a.([b])` | same | `1`, `2` |
| `{"x":a.b}.x` | same | `1`, `2` |
| `{"x":[a.b]}.x` | same | `[1,2]` |
| `{"sum":$sum(a[$>1]),"values":[a[$>1]]}` | `{"a":[1,2,3]}` | `{"sum":5,"values":[2,3]}` |

Arrays append normalized members one level; directly nested array constructors
(including their predicates) retain their result. Grouping parentheses change this
syntactic rule. A parenthesized path beginning with an explicit array retains its
input focus when used as another path step. Explicit array steps in a path preserve their boundaries. Stored
multi-item sequences keep their internal sequence identity when read back from an
object; undefined remains distinct from null even though both serialize as null.

Object keys must normalize to a string or missing. A missing key skips its value;
a missing value omits the member. Keys compare decoded UTF-16 units, so `"a"` and
`"\u0061"` collide. Different member expressions yielding the same key raise
`DuplicateKey`, even if a value would be missing. All keys finish before any values.
In a local array context, repeated keys from the **same** member expression group
candidate contexts before its value is evaluated (for example `a.({k:$sum(v)})`).
This is distinct from duplicate input keys, whose lookup remains last-wins.

The top-level input array is one context; `$.{...}` explicitly maps its items.
A leading array constructor evaluates once, even on empty input. Pinned upstream
also preserves an empty leading constructor through following path steps and drops
a leading filtered constructor that collapses to a number, boolean or object.
Null collapse maps the reference's native exception to `TypeError`. Implicit
string iteration after that collapse (`["ab"][0].$`) is explicitly deferred with
runtime `UnsupportedExpression`; parenthesized normal navigation remains available.
Postfix grouping, singleton retention and ranges follow the navigation rules above.
Function support and deferred calls are described below.

Each constructor completes before emitting a container. Mapped constructors can
emit earlier complete containers before a later failure, and consumer cancellation
stops there. Full JSON validation still precedes all output. Constructed values
preserve IEEE numbers and lone UTF-16 surrogates until JSON serialization.
[102 readable cases](tests/semantics/constructors.json), ownership/serialization/error
regressions and CLI tests freeze these rules.

Two explicit host-boundary differences from jsonata-js: prototype-related names
such as `__proto__` are ordinary JSON keys, and object construction never mutates
an empty input array. The reference's inherited-property behavior and empty-array
mutation side effects are not emulated. Member values evaluate in integer-key order followed by group insertion
order; asynchronous races between several failing reference members are not emulated. Dedicated Rust tests freeze these policies;
the seeded differential generator excludes mutation-dependent empty-root cases,
while isolated empty-array semantics remain covered by the readable corpus.

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
Boolean NOT is the supported function `$not`; there is no `!`
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

## Lexical runtime

Bindings (`$x := expr`) evaluate once and return the stored value. Blocks evaluate in
order, return the final expression and introduce scope, including single-expression
parentheses. Inner assignments shadow; they do not update a parent binding. Missing
shadows an outer value just as null does. Unbound user variables are missing.

Lambdas capture current `$` and their lexical frame. Later assignments in that frame
are visible, including forward references and recursive calls. Calls retain argument
values before invocation, bind missing parameters as undefined, and evaluate/ignore
extra arguments for lambdas without signatures. Arguments use the caller's context; the body uses
the captured context. Functions can be passed, returned, stored in containers and called
through computed expressions. The root binding `$$` is available across navigation.
Conditionals use effective boolean value, evaluate only the selected branch, and return
missing without an else branch when false. Function values are false in boolean context;
equality compares function identity. `$boolean()` / `$not()` default to current context;
`$exists` requires one argument. Explicit missing propagates through boolean/not.

Bindings retain normalized sequences with borrowed leaves. They never retain computation
recipes. Stateful mapping/filter stages finish once before later stages can replay them;
negative indexes and repeated variable use cannot repeat assignments. Pure expressions
keep streaming. See [readable cases](tests/semantics/lexical.json) and Rust ownership,
record isolation, cancellation and recursion regressions.

Limits and explicit policies:

- Calls stop with `DepthLimit` after 64 active calls or 512 accumulated body-tree levels.
  This includes tail recursion; tail-call elimination and optional signatures are deferred.
- Known unimplemented builtins are function values; calling them raises runtime
  `UnsupportedExpression`, including dynamic calls. Unknown user-function calls are `TypeError`.
- JSONata-js runs constructor members concurrently. Unscoped assignments shared across
  array members, or in object members, are rejected at compile time. Use member-local
  parenthesized blocks; race-dependent shared assignment is deferred.
- `Value::Function` is opaque. `write_compact` and the CLI reject function output because
  it has no JSON encoding. Host function registration/invocation remains deferred.

## Lookup and specialization

[`$lookup`](https://docs.jsonata.org/object-functions) accepts an object and string key;
its one-argument form uses current context as the object. Missing/scalar/null objects
produce missing. Arrays recursively search their objects and flatten returned array
members one level. Explicit missing key looks up `"undefined"`, matching the reference;
other non-string keys are type errors. Last decoded duplicate input keys win.

Compilation changes execution cost, not result shape, identity or error timing.
Constant containers remain fresh constructions; retained bindings share identity.
Computed indexes stay distinct from literal indexes after folding. Invalid constant
expressions still fail at evaluation, after record validation, and unselected branches
remain unevaluated. [Compiler regressions](crates/jx/tests/compiler.rs) freeze these rules.

## Executable coverage

`just conformance` executes all **568** imported cases from complete `fields`,
`missing-paths`, `quoted-selectors`, `flattening`, `numeric-operators`,
`comparison-operators`, `boolean-expresssions`, `literals`, `null`, `parentheses`,
`predicates`, `simple-array-selectors`, `multiple-array-selectors`,
`function-count`, `function-sum`, `function-max` (also containing min cases),
`array-constructor`, `object-constructor`, `variables`, `blocks`, `conditionals`,
`closures`, `lambdas`, `higher-order-functions`, `function-boolean`, `function-exists`,
`wildcards`, `descendent-operator`, `range-operator`, `sorting`, `inclusion-operator`,
`coalescing-operator`, `default-operator` and `function-lookup`
groups of JSONata **2.2.0**, revision
`8ee4476f8a228bfc7a62979ae0a9c13a4043cd03`:

| Classification | Cases | Assertion |
| --- | ---: | --- |
| Supported results | 481 | Semantic JSON result or missing matches upstream |
| Supported errors | 55 | Asserted compile/evaluate phase and mapped local error kind |
| Deferred syntax | 13 | Compile-time `UnsupportedExpression` |
| Deferred builtin calls | 16 | Runtime `UnsupportedExpression` |
| Recursion guard | 3 | Runtime `DepthLimit`; upstream uses tail calls |

These are selected groups, not a percentage of the full suite. No imported case
is skipped. `tests/conformance/manifest.json` lists every case and reason; the
runner fails on unclassified files or changed behavior. Promote status alongside
implementation. Unimported language families remain deferred as listed above.

Upstream separates reusable `datasets/` from `groups/<topic>/caseNNN.json`, with
`expr`, `data`/`dataset`, bindings, and expected result/undefined/error fields.
The general suite also has expression files and multi-case files; the current
adapter handles inline/named data, multi-case files and expression files. JSON host
bindings are explicitly adapted to lexical declarations. Absent host input uses an
undefined predicate context, distinct from explicit JSON null; `unordered` assertions
compare multisets. Host functions remain deferred.
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
node scripts/check-constructors.cjs /tmp/jsonata-reference target/release/jx
node scripts/check-lexical.cjs /tmp/jsonata-reference target/release/jx
node scripts/check-navigation.cjs /tmp/jsonata-reference target/release/jx
node scripts/check-compiler.cjs /tmp/jsonata-reference target/release/jx
node scripts/check-execution.cjs /tmp/jsonata-reference target/release/jx
```

It checks the 42 readable cases and 5,894 deterministic generated/curated path
evaluations against the reference stream. The scalar check adds 1,936 curated and
seeded generated evaluations, including runtime error kinds and raw numeric/Unicode
boundaries. The filter check adds 3,820 cases, including nested contexts, chained
positions, numeric lists and stage error precedence. The aggregate check adds 4,948
evaluations across raw arrays, normalized sequences, filters, nested calls and
numeric/error boundaries. The constructor check adds 5,000 evaluations including
102 readable cases, shape matrices and 2,000 seeded expressions. The lexical check adds 2,796 evaluations across bindings, captured contexts,
escaping functions, argument retention and stateful predicates. The navigation check
adds 1,374 comparisons, including 129 readable cases, shape matrices, grouping/sorting
composition, duplicate-key enumeration, ranges, kept sequences and fallback/membership
behavior. The compiler check adds 947 comparisons covering constant folding, constructor
identity/shape, indexed lookup, dynamic keys and builtin shadowing. Numeric-plan checks
add 4,443 comparisons across input shapes, IEEE boundaries, repeated loads and streamed
contexts. Offline plan/tree tests also compare exact error offsets and floating-point
bits. Milestone 11 changes execution only; the upstream classifications are unchanged.
The known upstream
empty-root-array mutation during object construction is excluded from generated
constructor/lexical comparisons. Normal `just all` needs neither Node nor the upstream checkout.
