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
| Result sequences / flattening | Supported for field paths; streamed without collecting |
| Indexes, predicates, wildcards, parent/descendant, order/group/join | Unsupported syntax; incremental follow-up |
| Literals, operators, parentheses, conditionals, ranges | Unsupported syntax; scalar milestone |
| Constructors, variables, functions, closures, transforms, regex, standard library | Unsupported syntax; later milestones |
| Comments, general unquoted Unicode names, single/double quoted selectors | Deferred syntax; compile error |
| `true`, `false`, `null`, `and`, `or`, `in`, `function` | Rejected unquoted; backticks can select these field names |

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

## JSON boundary policies

- Exactly one complete UTF-8 JSON value, with standard JSON whitespace. Validate
  every byte, including unselected fields and trailing content. BOM is rejected.
- Maximum 128 nested containers; exceeding it returns `DepthLimit`. The library
  has no byte-size limit; the CLI has a configurable record-size limit.
- All syntactically valid JSON numbers are accepted and preserved, including
  integers outside binary64 precision and large exponents. Numeric conversion,
  range errors and canonical number formatting are deferred.
- `\uXXXX` escapes permit lone surrogate units, as JSON grammar and the reference
  parser do. Raw values preserve them; field matching compares UTF-16 units.
  Expression field names are Rust UTF-8 and cannot denote an isolated surrogate.
- Serialization is compact but token-preserving. Identity/subtree output retains
  duplicate object keys and original escapes/numbers; it does not canonicalize
  like JavaScript parse/stringify. Lookups still use last-wins semantics. This is
  a deliberate raw JSON boundary policy, not a claim of byte-equivalent JS output.

## Executable coverage

`just conformance` executes all **83** imported cases from complete `fields`,
`missing-paths`, `quoted-selectors` and `flattening` groups of JSONata **2.2.0**, revision
`8ee4476f8a228bfc7a62979ae0a9c13a4043cd03`:

| Classification | Cases | Assertion |
| --- | ---: | --- |
| Supported | 22 | Semantic JSON result or missing matches upstream |
| Deferred syntax | 61 | Exactly `UnsupportedExpression` |

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
```

It checks the 42 readable cases and 5,894 deterministic generated/curated path
evaluations against the reference stream. Normal `just all` needs neither Node
nor the upstream checkout.
