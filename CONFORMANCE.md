# JSONata compatibility

Semantic authority: [official language documentation](https://docs.jsonata.org/overview.html)
and the [upstream suite](https://github.com/jsonata-js/jsonata/tree/v2.2.0/test/test-suite).
This is an early subset, not a full JSONata implementation. Errors use local
`ErrorKind` values and byte offsets; upstream diagnostic codes are deferred.

| Area | Current behavior / status |
| --- | --- |
| Identity | `$` returns the complete input value |
| Object fields | `a`, `a.b`, `$.a.b`; ASCII names `[A-Za-z_][A-Za-z0-9_]*` |
| Quoted fields | Backtick-delimited UTF-8 names, including empty names; no escape interpretation in the expression |
| Missing | Zero results; different from JSON null |
| Null/scalar context | Further field lookup yields missing |
| Terminal arrays/objects | Borrowed intact as one value, including empty/singleton/nested arrays |
| Duplicate input keys | Last decoded matching key wins for lookup |
| Root/intermediate array mapping | **Deferred M2**; evaluation raises `ArrayTraversal`, even for empty arrays |
| Multiple-result sequences / flattening | **Deferred M2**, never silently approximated |
| Indexes, predicates, wildcards, parent/descendant, order/group/join | Unsupported syntax; incremental follow-up |
| Literals, operators, parentheses, conditionals, ranges | Unsupported syntax; scalar milestone |
| Constructors, variables, functions, closures, transforms, regex, standard library | Unsupported syntax; later milestones |
| Comments, general unquoted Unicode names, single/double quoted selectors | Deferred syntax; compile error |
| `true`, `false`, `null`, `and`, `or`, `in`, `function` | Rejected unquoted; backticks can select these field names |

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

`just conformance` executes all **22** imported cases from complete `fields`,
`missing-paths` and `quoted-selectors` groups of JSONata **2.2.0**, revision
`8ee4476f8a228bfc7a62979ae0a9c13a4043cd03`:

| Classification | Cases | Assertion |
| --- | ---: | --- |
| Supported | 7 | Semantic JSON result or missing matches upstream |
| Deferred array mapping | 6 | Exactly `ArrayTraversal` |
| Deferred syntax | 9 | Exactly `UnsupportedExpression` |

These are selected groups, not a percentage of the full suite. No imported case
is skipped. `tests/conformance/manifest.json` lists every case and reason; the
runner fails on unclassified files or changed behavior. Promote status alongside
implementation. Unimported language families remain deferred as listed above.

Upstream separates reusable `datasets/` from `groups/<topic>/caseNNN.json`, with
`expr`, `data`/`dataset`, bindings, and expected result/undefined/error fields.
The general suite also has expression files and multi-case files; the current
adapter handles exactly the imported shapes and no host functions or bindings.
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
