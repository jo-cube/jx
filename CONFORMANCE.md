# JSONata compatibility

Semantic authority: [official language documentation](https://docs.jsonata.org/overview.html)
and the [upstream suite](https://github.com/jsonata-js/jsonata/tree/v2.2.0/test/test-suite).
The large majority of the pinned language corpus passes; compatibility boundaries
beyond that inventory remain explicit. Errors use local
`ErrorKind` values and byte offsets; upstream diagnostic codes are deferred.

| Area | Current behavior / status |
| --- | --- |
| Context | `$` is current context; `$$` initially references the root record |
| Field paths | `a`, `a.b`, `$.a.b`; Unicode/unquoted names delimited by JSONata operators or whitespace |
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
| Context/index bindings | `@` and `#` path bindings, joins, filters, sorting and grouped bindings |
| Parent navigation | `%` with statically derivable path ancestry; filters, sorting, grouped paths and tuple bindings |
| Literals | Binary64 numbers, booleans, null, single/double quoted strings |
| Operators | `+ - * / %`, `= != < <= > >=`, `and or`, unary `-` |
| Parentheses | Expression grouping, lexical blocks and grouped path steps; `()` is missing |
| Other operators | `in`, array ranges, `??`, `?:`, `:=`, `? :`; concatenation `&` and chaining `~>` supported |
| Aggregates | `$count`, `$sum`, `$min`, `$max`, `$average`; direct streaming folds and first-class calls |
| Constructors | Arrays, objects, computed keys/values, nested and mapped construction; see below |
| Structural updates | `|pattern|update[,delete]|` captured callables, `$clone`, selective immutable copy/update; boundaries below |
| Singleton retention | `expr[]` preserves sequence shape; missing stays missing |
| Lexical runtime | Variables, bindings, blocks, conditionals, lambdas (`function` / `λ`), calls, closures, compiled signatures and tail execution |
| Builtins | Aggregates, boolean helpers, lookup, string/numeric/collection helpers and higher-order functions; see library table below |
| Dynamic evaluation / effects | `$eval` with inherited environment and optional focus; lazy `$random` / `$shuffle` with deterministic injection |
| Deferred runtime/integration | Native host coercions, embedding and non-tail recursion beyond resource guards |
| Quoted selectors | Single/double quoted strings become field names in dotted paths; escapes decoded; lone-surrogate field names deferred |
| Comments / names | Non-nesting `/* … */` comments; Unicode field/variable names supported |
| Keyword field names | `and`/`or`/`in` can be names in operand/field positions; `true`, `false`, `null` require backticks; bare `function` / `λ` are names outside lambda syntax |

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
use `(items{key:value}).field` to navigate the constructed object. Predicates after
a non-path grouping require parentheses; empty `[]` retention remains allowed. Equal keys from one
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
these rules, validation, borrowed output, cancellation and resource limits. Parent and
transform rules appear below; remaining library coverage stays explicit.

## Scoped paths

`rows#$i` binds each result's zero-based position; `rows@$r` binds the selected value
while keeping the preceding context for the next step. Thus
`rows@$r.bands[kind=$r.kind]` joins two fields of the same object. Bindings follow
candidates through maps, filters, sorting and grouping, shadow lexical variables,
and end at the path/parentheses boundary. Closures can retain a candidate's bindings.

A `#` directly on a map step counts that step's local results. After a filter it
counts the combined filtered sequence. Once a path carries bindings, subsequent
step predicates filter the combined tuple sequence: `a#$i.b[0]` selects one `b`
overall. Arrays and sequences still normalize independently; tuple steps append
one level, including explicit array-valued results. `[]` retains the final shape.

Grouping combines both current values and each named binding by key before evaluating
member values. Equal keys from different members still fail. Sorting keeps bindings
attached to their candidates. The pinned reference ignores a direct `#` on a sort
that already carries tuples; `^(key)[true]#$i` explicitly indexes the sorted stream.
Context-dependent predicates, grouping and another sort immediately after sorting an
existing tuple stream are explicitly deferred: the reference drops its tuple marker
and exposes internal objects there. Literal predicates remain supported. Insert a
map (`^(key).$[predicate]` or `^(key).${key:value}`) to restore the binding context.
Readable cases freeze these boundaries.

Boolean predicates and nonnegative literal positions stream. Predicates that may
need the sequence length retain rows once; sorting/grouping also retain their inputs.
Bindings store evaluated values, never replayable expressions. Input leaves stay
borrowed. Cancellation stops streamed stages, and full JSON validation remains first.

`tests/semantics/tuples.json`, ownership/cancellation regressions and the complete
upstream `joins` group cover these rules. One explicit host-boundary policy: grouping
an empty tuple stream produces `{}`, consistent with ordinary empty grouping;
JSONata 2.2.0 instead throws an uncoded JavaScript exception. This local case is tested
separately from differential cases. Remaining library functions stay deferred.

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

Direct `$count(expr)`, `$sum(expr)`, `$min(expr)`, `$max(expr)` and `$average(expr)` calls compose with
paths, filters, operators and other aggregate calls. `orders.$sum(price)` aggregates
in each candidate context; `$sum(orders.price)` aggregates the combined result.
These builtins also support references and dynamic calls. Chaining (`~>`) and typed partial calls are supported. Statically unshadowed direct calls stream their arguments.

| Normalized argument | `$count` | `$sum` | `$min` / `$max` |
| --- | --- | --- | --- |
| Missing (including no filter matches) | `0` | Missing | Missing |
| Empty raw array | `0` | `0` | Missing |
| Number | `1` | That number (added to zero) | That number |
| Null, boolean, string, object | `1` | Type error | Type error |
| Array / multi-item sequence | Number of members | Sum of numeric members | Numeric minimum / maximum |

`$average` returns missing for missing/empty input, otherwise the binary64 sum divided
by member count. It uses the same streaming fold, numeric checks and error ordering.

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

All five functions require exactly one argument; bad arity raises runtime `TypeError`.
The [array-function docs](https://docs.jsonata.org/array-functions) describe a context
fallback for `$count()`, but pinned JSONata 2.2.0 rejects it (`T0410`). We follow the
pinned implementation and test this discrepancy. Use `$count($)` explicitly.
[Numeric aggregate docs](https://docs.jsonata.org/aggregation-functions) and
[92 readable cases](tests/semantics/aggregates.json) define the remaining behavior;
Rust regressions cover error precedence, validation, cancellation and numeric bits.

## Constructors

Array and object construction follows the [result structure documentation](https://docs.jsonata.org/construction)
and [sequence rules](https://docs.jsonata.org/processing). Computed members use the
same operators, filters, paths and aggregates. Constructed containers own their
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

## Parent navigation and transforms

Authority: [path operators](https://docs.jsonata.org/path-operators),
[transform operator](https://docs.jsonata.org/other-operators) and
[clone](https://docs.jsonata.org/object-functions). `%` resolves statically to a
preceding name/wildcard stage's incoming context. Arrays do not add ancestry levels.
`orders.items[price>%.limit]` reads the enclosing order; `orders.items.price.%.%.id`
steps back twice. Filters, sort keys, grouped paths and tuple rows preserve demanded
contexts; grouping combines captured bindings with their corresponding rows.

Parent derivation through a variable, descendant, constructor or sorted stage is a
compile error, as in the reference. Bare `%`, `$.%` and excessive parents also fail
compilation. Parent requests inside function bodies, callees, transform updates or
postfix group members do not resolve against an outside path: bind `%` explicitly
before capturing it in a closure. Parentheses retain parent provenance while ending
user path bindings. Expanded paths count toward the 128-level expression and
512-level function-body budgets. Root-array wrapping follows the reference, which can capture
the entire root array rather than an individual object. Small cases freeze these
sometimes surprising boundaries.

`$clone(objectOrArray)` creates fresh container identities, breaks constructor aliases
and leaves the original unchanged. Conversion follows `$string`/JSON parsing: fractional
numbers round to 15 significant digits, functions become empty strings, undefined
object members disappear, undefined array members and nested NaN become null. Infinity
fails even in an unselected member. Borrowed string encodings, including lone UTF-16
surrogates, remain intact. Cloning a scalar/null is a type error; missing stays missing.

A transform is a one-argument function with lexical bindings captured at definition.
It clones first, selects all locations before updates, merges object members at each
location, then deletes the named string/string-array keys. Missing updates/deletions
are no-ops; wrong types are errors. Duplicate selections and overlapping locations
observe earlier updates. Delete expressions see the merged object; `$$` still refers
to the captured caller root. Numeric array keys and `length` updates preserve array
shape and holes; deleting an index yields a null JSON slot. Array growth is bounded
at one million items (`NumericRange`). No-match still clones and checks conversion.

Explicit deferred edges: custom `$clone` overrides, transform locations outside the
cloned argument, unscoped bindings in the pattern/update, function-valued updates and
cyclic output raise `UnsupportedExpression`. These need mutable alias/lifetime
semantics beyond selective immutable updates. Member-local blocks and callbacks
returning JSON remain supported. Rebinding `$clone` to the builtin itself works;
a nonfunction override is a type error. Nonempty updates to primitive locations map
the reference's uncoded host exception to `TypeError`. Prototype-related names retain
the existing ordinary-JSON-key policy. Whole-record validation precedes output;
transforms finish before emission and can fail without exposing partial updates.

[Readable parent/clone/transform cases](tests/semantics/structure.json), borrowing,
validation/cancellation regressions and CLI tests freeze these rules. Complete upstream
`parent-operator` and `transforms` groups have asserted outcomes, including errors.

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

Precedence follows JSONata: paths/unary minus, multiplicative, additive/concatenation, comparison/chaining,
`and`, then `or`. Binary operators associate left. `and`/`or` short-circuit the RHS;
truth conversion handles missing, null, primitives, nested arrays and objects.
Arrays/sequences are true if any member is true; empty objects/arrays are false.
Boolean NOT is the supported function `$not`; there is no `!`
operator or unary `+`. Concatenation `&` uses JSONata string conversion; arithmetic never implicitly converts strings.

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

`TypeError`/`NumericRange` report the operator's expression byte offset. Expression whitespace is space, tab, LF, CR or vertical tab; form feed is not whitespace.
Parser nesting and expression-tree depth are capped at 128 (`DepthLimit`). Upstream error
codes/text are not a stable API. [100 readable scalar cases](tests/semantics/scalars.json)
and separate regression tests freeze these rules, validation order, depth limits,
Unicode, binary64 boundaries and CLI runtime errors.

## Conversion and composition

`$string(value[, pretty])` preserves strings and missing, converts functions to `""`,
and canonicalizes other values as JSON text. Fractional numbers round to 15 significant
digits; integer precision and ECMAScript exponent thresholds are preserved. Nested
functions become empty strings, undefined array members become null, undefined object
members are omitted, duplicate raw keys resolve last-wins, and integer keys come first.
`pretty=true` indents two spaces. Top-level non-finite numbers fail with `NumericRange`;
nested infinity also fails, while nested NaN becomes null. This conversion is separate
from the byte-preserving API/CLI serialization policy above.

`$number` preserves numbers/missing, maps booleans to 1/0, and accepts finite decimal
text or unsigned `0x`/`0o`/`0b` text. Leading decimal zeros follow the pinned reference.
Whitespace, leading `+`, incomplete fractions/exponents and nonnumeric text fail;
null, arrays, objects and functions are type errors. Unescaped numeric text needs no
owned copy. The reference's asymmetric radix regex also admits some malformed prefixes:
these produce NaN, as explicitly tested, rather than broad JavaScript coercion.

`&` evaluates both operands and applies string conversion, treating missing as empty.
An empty side can preserve the other string's borrowing. Arithmetic remains strict.
`lhs ~> f(args)` prepends `lhs`; `lhs ~> f` calls with one argument, or composes when
both sides are functions. A composed function takes one argument and ignores extras.
Bare RHS call predicates are bypassed by the pinned reference; parenthesize the complete
pipeline before filtering. A first-step or direct-expression `[]` propagates retention to the chain result;
a later path-step `[]` keeps only its argument. Parentheses end that propagation.

`f(?, value)` binds evaluated values once and returns a function whose parameters are
the remaining holes, in order. Omitted parameters become undefined; extra supplied
expressions still evaluate. Repeated partials merge slots once; lexical focus and
captured environments remain intact. Builtin partials do not substitute context or
promote scalars to arrays. Typed array calls and numeric conversion are supported;
Lambda partials bypass their original signature. Native count reads array/string
length (UTF-16 for strings), returns zero for missing, reads an object's `length`, and
returns missing for scalar numbers/booleans; null is a type error. Native zip partials
take one formal argument, return singleton tuples for arrays and `[]` otherwise.
Other JavaScript signature-bypass coercions remain runtime `UnsupportedExpression`.
Native `$string` partials are deferred because the pinned reference cannot represent
its default parameter; use `$string` as a callback or wrap it in a lambda.

[178 readable cases](tests/semantics/composition.json), borrowing/escape/error/guard
regressions and CLI tests freeze these boundaries. Known builtin call chains compile
to ordinary calls, preserving streaming aggregation and numeric-plan lowering.

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

- Non-tail calls stop with `DepthLimit` after 64 active calls or 512 accumulated
  body-tree levels. Tail calls do not grow that stack; each tail chain permits one
  million iterations, then reports `EvaluationLimit`. This deterministic guard replaces
  the reference's time guard for the infinite-tail corpus case. Consumer cancellation
  applies after function evaluation has produced its retained result.
- Known unimplemented builtins are function values; calling them raises runtime
  `UnsupportedExpression`, including dynamic calls. Unknown user-function calls are `TypeError`.
- JSONata-js runs constructor members concurrently. Unscoped assignments shared across
  array members, or in object members, are rejected at compile time. Use member-local
  parenthesized blocks; race-dependent shared assignment is deferred.
- `Value::Function` is opaque. `write_compact` and the CLI reject function output because
  it has no JSON encoding. Host function registration/invocation remains deferred.

### User signatures and tail calls

Authority: [function signatures and tail recursion](https://docs.jsonata.org/programming).
Signatures compile with their lambda. Simple types, JSON/any types, unions, optional
`?`, context `-`, variadic `+`, array and function subtype syntax are accepted.
All arguments evaluate before validation. An explicit missing scalar is valid and
is distinct from an omitted required slot; a missing function fails unless in a union.
Context is substituted only when the signature matched an absent slot; explicit
missing does not substitute it. Array parameters promote supplied scalars, preserve
borrowed arrays and reject incompatible element types. Context substitution bypasses
array promotion/subtype validation, matching the reference.

Return types and function sub-signatures are descriptive, not runtime checks. Signatures
accept at most 128 input parameters; wider signatures report `SignatureError`.
Array subtype checks intentionally follow the pinned shallow homogeneous rule:
`a<a<n>>` checks that immediate members are arrays, not their numeric leaves. Optional
matching retains the reference's positional quirk: a skipped optional slot still
advances the argument index. Partial lambdas remove the original signature entirely;
subsequent ordinary tail calls validate their own target signatures.
The documented primitive `u` union is supported; JSONata 2.2.0's parser ignores that
symbol. This deliberate difference has a local semantic test, not a differential case.

Direct tail calls through conditional branches and final block expressions run with
bounded Rust stack and reusable uncaptured frames, including mutual recursion and
partial/composed callable targets. Captured frames remain live until record completion.
Arguments use lexical focus; tail signature/context defaults use the original caller's
focus, including contextual builtins. A pipeline expression or postfix result processing
is not a tail position in the pinned parser. Non-tail recursion retains its stack guard.
[Readable cases](tests/semantics/functions.json) and borrowing/frame/limit regressions
cover these boundaries. No public host invocation or JIT is added.

## Standard library

| Area | Implemented functions |
| --- | --- |
| Strings | `$string`, `$length`, `$uppercase`, `$lowercase`, `$trim`, `$pad`, `$substring`, `$substringBefore`, `$substringAfter`, `$contains`, `$split`, `$join`, `$match`, `$replace` |
| Encoding | `$encodeUrl`, `$encodeUrlComponent`, `$decodeUrl`, `$decodeUrlComponent`, `$base64encode`, `$base64decode`; host boundary below |
| Numbers | `$number`, `$abs`, `$floor`, `$ceil`, `$round`, `$sqrt`, `$power`, `$formatNumber`, `$formatInteger`, `$parseInteger`, `$formatBase`, `$random`; aggregates above |
| Date/time | `$fromMillis`, `$toMillis`, `$now`, `$millis`; pictures and boundaries below |
| Collections | `$append`, `$reverse`, `$distinct`, `$sort`, variadic `$zip`, `$single`, `$shuffle` |
| Objects / types | `$keys`, `$spread`, `$merge`, `$type`, `$lookup`, `$clone` |
| Higher-order | `$map`, `$filter`, `$reduce`, `$each`, `$sift` |
| Boolean / diagnostics | `$boolean`, `$not`, `$exists`, `$error`, `$assert` |

Fixed signatures follow the pinned reference: context substitution is distinct from
an explicit missing argument; null does not stand in for missing. Array parameters
accept scalar singleton inputs; typed arrays reject wrong member types. Arguments
finish before signature checks or callbacks. Apart from the explicit conversion functions, string/numeric helpers do not coerce
values. `$length`/`$substring` count Unicode codepoints; empty-separator `$split`
uses UTF-16 units, including isolated surrogates. `$trim` collapses only space, tab,
LF and CR. `$contains`/`$split`/`$replace` accept literal strings or matchers.

Map/filter/each return sequences, omitting missing callback results and retaining
nested arrays. Empty output normalizes to missing; `[]` retains a singleton before
normalization. Native sequences returned by a lambda tail call remain nested callback
values, including empty sequences, matching the pinned reference. `$filter` uses
boolean conversion, not positional predicate rules. Callbacks receive value, optional
index/key, and optional original collection according to declared arity. Reduce
callbacks require at least two parameters; optional third/fourth parameters are index
and original array. Missing input stays missing even with an initial accumulator.

`$distinct` uses existing structural equality and preserves explicit array versus
sequence shape. `$merge` accepts only objects, with later keys winning; `$keys`
deduplicates recursively across arrays. Object iteration uses last decoded keys and
integer-key ordering. Prototype-related keys remain ordinary JSON keys. `$sift`
returns missing for no retained members. Computed strings retain encoded UTF-16 units;
borrowed leaves remain borrowed through collection operations.

The reference throws uncoded host exceptions for missing matchers in `$contains`/
`$split`, a matching missing delimiter in `$substringAfter`, and nonempty `$sift`
without a callback. These raise local `TypeError`. The readable
[builtin corpus](tests/semantics/builtins.json), Rust ownership/error regressions and
CLI tests freeze supported boundaries. [Official function documentation](https://docs.jsonata.org/string-functions)
and complete imported groups remain the authority.

Still deferred: untyped native partial coercions and host-function embedding. Known deferred calls raise
`UnsupportedExpression`; existing order-by syntax remains available.

## Dynamic evaluation and randomness

[`$eval`](https://docs.jsonata.org/string-functions#eval) takes a string and optional
focus. Missing source returns missing; other source types are errors. It inherits
lexical bindings, sees rebinding/shadowing and exports assignments to that environment.
Explicit missing focus retains current context; null replaces it. Explicit arrays
use ordinary root-array wrapping. `$$` continues to refer to the enclosing root.
Syntax failures wrap as `EvalSyntax` (D3120); execution failures as `EvalError` (D3121),
retaining the nested diagnostic. Full input validation precedes compilation/effects.

Returned literals, nested containers, closures, partials, transforms and matcher
continuations survive the dynamic compilation region. Bound sequences retain values
once. Escaped recursive closures use the existing bounded tail loop. Upstream native
tail `$eval` uses the invocation environment/focus, while non-tail evaluation uses the
lexical body environment/focus; function parameters therefore need not be visible to
a native tail eval. Its own result normalizes before an outer `[]` postfix.

[`$random`](https://docs.jsonata.org/numeric-functions#random) returns `[0,1)` draws;
[`$shuffle`](https://docs.jsonata.org/array-functions#shuffle) returns a permuted explicit
array. Missing stays missing, scalar/null inputs become singleton arrays, and empty/
singleton arrays preserve their shape without drawing. Draws are not folded, memoized
or replayed. `Random::seeded` plus `evaluate_with_random` shares deterministic draws
across records, nested eval and closures; default seeding is lazy and noncryptographic.
Random values need not match another engine's PRNG, but permutation and draw order do.

Boundaries outside the corpus: isolated UTF-16 surrogates in dynamic source return
`EvalSyntax` because Rust source is UTF-8; surrogate string values remain supported.
Untyped native shuffle partials with non-array inputs remain `UnsupportedExpression`.
Host invocation, asynchronous evaluation and non-tail stack expansion stay deferred.
[Readable cases](tests/semantics/effects.json) and deterministic Rust tests freeze
context, ownership, error wrapping, borrowed output and effect ordering.

## Everyday helpers

`$round` uses decimal exponent shifting followed by half-even rounding; all
precisions allocate nothing. Shifting uses a stack buffer, avoiding
binary multiplication that changes decimal ties. Missing stays missing; null and
wrong types fail. Fractional precision follows the pinned implementation's NaN
behavior. `$pad` truncates width, counts codepoints, repeats/truncates its pattern,
and returns borrowed input when no padding is needed. Padding beyond one million
additional codepoints raises local `NumericRange` (a resource guard).

`$sort` is stable, uses the existing fallible top-down merge order, and keeps borrowed
leaves. Default sorting requires only numbers or only strings for two or more items;
singleton/empty inputs retain their shape. Strings compare UTF-16 units, matching the
reference. Comparator results use native truth, including truthy empty containers;
`$single` predicates use effective boolean conversion. Single accepts an optional
callback receiving value/index/original array and errors for zero or multiple matches.
An explicitly missing callback is a type error for sort/single; omitting it uses the
default. Typed native partials may omit it. `$zip` accepts one or more arguments,
promotes scalars, stops at the shortest array, and preserves nested values. Variadic
native partial application accepts one formal argument and ignores additional bound
slots, as in the pinned reference; direct `$zip` remains variadic.

URI encode/decode functions follow the reserved-character and strict UTF-8 rules of
`encodeURI` / `encodeURIComponent` and their inverses; malformed percent encodings
and unpaired surrogates during encoding raise `EncodingError`. ASCII-safe encoding
and decoding without percent escapes keep borrowing. Base64 uses the pinned Node binary-string convention, not UTF-8: encode
uses each UTF-16 unit's low byte and decode returns Latin1 characters. Decode accepts
URL alphabet, ignored characters, omitted padding and trailing bits. **Non-Latin1
base64 decoding is deferred**: Node Buffer results can change with V8 string storage
(e.g. literal versus concatenated `ＡYQ`); it raises `UnsupportedExpression` here.
The `base64` crate supplies the codec; no custom base64 machinery is introduced.

`$error` always raises `UserError`; `$assert` requires a boolean and returns missing
when true, otherwise `AssertionFailed`. Empty/missing messages use the reference
defaults. Dynamic messages are owned; static errors remain borrowed. Rust diagnostic
text replaces isolated UTF-16 units with U+FFFD; JSON string values still preserve
those units. Full JSON validation and argument evaluation precede helper failures.
The [helper corpus](tests/semantics/helpers.json) freezes these boundaries separately
from locally documented guards/deferred host behavior.

## Numeric and date pictures

Authority: [numeric functions](https://docs.jsonata.org/numeric-functions),
[date/time functions](https://docs.jsonata.org/date-time-functions) and
[date pictures](https://docs.jsonata.org/date-time). Missing primary inputs stay missing;
null/wrong types fail signature validation. Arguments finish before validation, and
constant picture errors remain conditional runtime errors after complete JSON validation.

`$formatNumber` supports positive/negative subpictures, optional/mandatory digits,
regular/irregular integer and fractional grouping, percent/per-mille, exponents and
custom decimal-format options. Rounding reuses decimal half-even semantics.
`$formatInteger` / `$parseInteger` support BMP decimal digit families, grouping,
letters, Roman numerals, English words and ordinals. Formatting floors integers;
parsing retains the reference's permissive numeric-prefix and case behavior.
`$formatBase` half-even rounds the number/radix and accepts bases 2–36.

Date rendering supports Gregorian components, names, widths, ordinals, ISO week/year/
month components, fractional milliseconds, escaped brackets and timezone pictures.
Pictured parsing handles separated/adjacent integer fields, names, timezones and
leading/trailing defaults; missing matches produce missing, interior gaps fail.
Default parsing accepts the reference ISO grammar with explicit UTC/offset timestamps
and date-only forms. Invalid calendar components preserve computed NaN (JSON output
is null), distinct from no match. `$now` and `$millis` share a timestamp per evaluation.

Pinned quirks are deliberate: negative HHMM formatting uses floor hours plus signed
remaining minutes; pictured negative timezone parsing adds minutes to negative hours;
`f` renders integer milliseconds but parses decimal fractions; parsed pictured years 0–99
use the reference's 1900 offset. ISO-week parsing fails with the same D3136 category as
upstream. Non-finite decimal output follows the reference's picture-dependent strings.

Explicit boundaries: host-local ISO times without a timezone; legacy fractional
date-only syntax; lone-surrogate picture/subject text; non-ASCII case-insensitive date
pictures and dotless-i/long-s subjects; multi-unit decimal syntax symbols; large fixed
integer/decimal/radix output (absolute value ≥10²¹); non-finite exponent, word or infinite
alphabetic/Roman rendering; and legacy week/day derivation in years 0–99 are deferred.
Huge padding/Roman outputs and exponent overflow have tested resource guards. Fraction
precision is at most 100, matching the reference fixed-decimal operation. Typed
first-class functions/partials work, but their dynamic calls reparse pictures.
[Readable formatting cases](tests/semantics/formatting.json), Rust validation/reuse
regressions, CLI tests and all six imported families freeze these distinctions.

## Regex and matcher text processing

Authority: [regex/matcher model](https://docs.jsonata.org/regex) and
[string functions](https://docs.jsonata.org/string-functions). `/pattern/` supports
`i`/`m` flags; division remains an infix operator. Patterns compile once. ECMAScript
captures, noncapturing groups, lookarounds, backreferences, greedy/lazy quantifiers
and named captures are supported. Matching and indexes use UTF-16 code units,
including lone surrogates. Unmatched capture groups retain undefined and serialize
as null inside the groups array.

Regex literals return functions. Calling with a string and optional numeric starting
index returns missing or `{match,start,end,groups,next}`. A matcher owns one shared
cursor; invoking it resets that cursor, and earlier continuations observe later calls.
Each continuation retains its own subject. A failed search resets the cursor to zero.
`next()` stops when the cursor reaches the subject end and raises `RegexError`
(reference D1004) if its next match is empty. The initial match may be empty.

`$match` returns a normalized result sequence of `{match,index,groups}`;
`$split` returns an explicit array. Missing source stays missing. Limits are
nonnegative numbers; fractional matcher limits use the reference loop behavior,
zero/NaN suppress matching, negative limits raise `NumericRange`. Consumers invoke
`next` after the final accepted item, even at a limit, preserving errors and side
effects. Well-formed custom matcher functions use the same protocol; invalid matcher
results raise `TypeError`.

Regex replacement strings expand `$$`, `$0`, and numbered captures with the
reference's group-count-dependent digit parsing. Literal replacements copy text
verbatim and reject an empty pattern. Matcher replacement callbacks receive the
complete match object, share lexical state, and must return a string. No-match and
zero-limit replacement preserve the input value. A matcher object's `next` is a
function: JSON output still rejects it; select its JSON members or use `$match`.

Explicit boundaries: native regex calls require string subjects and numeric offsets;
JavaScript argument coercions and literal-string replacement by a function are
unsupported. The engine's legacy `/i` folding differs for dotless-i/long-s:
case-insensitive patterns containing non-ASCII source or `\u` escapes are rejected
at compilation, and subjects containing U+0131/U+017F raise `UnsupportedExpression`
when matched under `/i`. Case-sensitive Unicode and surrogate patterns are supported.
`g`/`u`/`s` flags are not JSONata literal syntax. Dynamic regex construction, engine
extensions, and regex timeouts remain deferred. The
[matcher corpus](tests/semantics/matchers.json), retention regressions, imported groups
and differential suite freeze these boundaries.

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

`just conformance` executes the complete pinned **1,679-case, 102-group** JSONata
**2.2.0** language corpus, revision `8ee4476f8a228bfc7a62979ae0a9c13a4043cd03`.
There are **no unclassified or skipped language cases**. Separate upstream JavaScript
embedding, asynchronous API and parser-recovery tests are outside this inventory.

| Classification | Cases | Assertion |
| --- | ---: | --- |
| Supported results | 1390 | JSON result / missing agrees with upstream |
| Mapped expected errors | 288 | Compile/evaluate phase and local kind; user messages where applicable |
| Blocked compatibility | 0 | All prior blockers promoted |
| Recursion guard | 1 | Non-tail factorial exceeds the bounded stack |

M22 promotes all **16** remaining `$eval`/randomness cases: **14** results and **2**
mapped errors. The sole local limit is non-tail factorial; no language case is blocked.
M21 promotes **48** signature/tail/guard cases: **38** results and **10** mapped errors.
All signature cases and finite tail-recursive cases pass; one non-tail factorial
remains an explicit local limit. Infinite tail recursion asserts an execution-budget
error rather than a stack-depth error.
M20 promotes all **333** number/integer/base/date family cases: **298** results and
**35** mapped errors. Picture/date boundaries outside that pinned corpus remain explicit below.
The legacy singular `transform` group is now entirely supported: its 104 cases
exercise ordinary queries/constructors; structural updates live in `transforms`.
M19 promoted **142** cases that the baseline already handled (73 results, 69 mapped errors),
then added helper coverage, corrected two parser gaps and fixed a syntax-adapter gap. A blocker is not
counted as a passing upstream case simply because it rejects the expression.

`tests/conformance/manifest.json` is the reviewed inventory and importer source of
truth. Every row has an asserted outcome; changing implementation requires promoting
or updating its classification. The importer checks the complete pinned inventory.
`scripts/probe-conformance.py` records observations through the CLI for review; it
never labels cases automatically. Expected diagnostics are mapped coarsely: exact
upstream codes, token payloads and positions remain deferred.

Fixtures retain their MIT provenance. Host JSON bindings become lexical declarations;
absent input uses an undefined context; unordered results compare multisets. Syntax
errors compile the original expression so these adapters cannot conceal malformed
syntax. Two URI fixtures spell lone-surrogate expression characters as equivalent
JSONata escapes; their unused error-value metadata uses escaped text because Rust
strings cannot hold isolated UTF-16 units. All other fixture bytes are preserved.

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
node scripts/check-plans.cjs /tmp/jsonata-reference target/release/jx
node scripts/check-tuples.cjs /tmp/jsonata-reference target/release/jx
node scripts/check-builtins.cjs /tmp/jsonata-reference target/release/jx
node scripts/check-composition.cjs /tmp/jsonata-reference target/release/jx
node scripts/check-matchers.cjs /tmp/jsonata-reference target/release/jx
node scripts/check-structure.cjs /tmp/jsonata-reference target/release/jx
node scripts/check-helpers.cjs /tmp/jsonata-reference target/release/jx
node scripts/check-formatting.cjs /tmp/jsonata-reference target/release/jx
node scripts/check-functions.cjs /tmp/jsonata-reference target/release/jx
node scripts/check-effects.cjs /tmp/jsonata-reference target/release/jx
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
bits. Expanded-plan checks add 6,546 branch, aggregate, constructor and lookup
comparisons, with offline coverage for skipped demands, duplicate keys, exact errors
and borrowed numeric tokens. Milestones 11–13 change execution only; upstream
classifications are unchanged.
Scoped-path checks add **947** comparisons across root/array shapes, local/global
positions, joins, closures, sorting, grouping, and the imported supported join cases.
Builtin checks add **1,923** comparisons covering fixed signatures, missing/null,
Unicode, callback arity/context, closure effects, nested sequences and mixed pipelines.
Conversion/composition checks add **2,635** comparisons across numeric/escaping
boundaries, partials, closures, sequence shapes and mixed pipelines. The known upstream
empty-root-array mutation during object construction is excluded from generated
constructor/lexical comparisons. Normal `just all` needs neither Node nor the upstream checkout.

Demand capture does not change language coverage. Plan/tree checks include nested
prefixes, duplicate parent replacement and array fallback. Mutation tests require
identical validation errors (including offsets) for captured and unselected input,
including invalid UTF-8, trailing content and excessive nesting.

Matcher checks add **1,453** comparisons over regex syntax, UTF-16 boundaries, cursor
sharing, empty matches, callbacks, limits and mixed filters/grouping/pipelines.

Parent/clone/transform checks add **624** comparisons, including **135** readable
cases, nested/root-array shape matrices, tuple boundaries, closure captures, sorting,
sequential/overlapping updates and original-input isolation. The conformance adapter
preserves dataset and inline object-key order, which `$keys` makes observable.

Everyday-helper checks add **1,884** comparisons, including **132** readable cases
(with three explicit local guards/host boundaries), seeded decimal rounding, typed
partials, stable callbacks, URI/base64 boundaries and mixed lexical/grouped pipelines.
Formatting checks add **2,313** comparisons across static/dynamic pictures, symbols,
rounding, integer/word/base forms, calendar/week boundaries, timezones, missing/errors
and mixed pipelines. The original 18 differential suites now total **50,083** comparisons.

Function-runtime checks add **558** comparisons across signatures, optional/variadic
matching, context defaults, partials and deep direct/mutual/composed calls. Primitive
`u` is locally tested against its documented meaning; the pinned parser omits it.

Dynamic/effect checks add **253** comparisons, including **87** readable cases,
lexical/focus shape matrices, escaped callable recursion and randomized permutation
invariants. Eight Rust tests separately assert wrapped diagnostics, compatibility boundaries, deterministic draws, borrowing, skipped
effects and validation/error ordering. All **19** suites total **50,336** comparisons.
