# Conformance

Semantic authority is the [official JSONata documentation](https://docs.jsonata.org/overview.html)
and [jsonata-js language tests](https://github.com/jsonata-js/jsonata/tree/v2.2.0/test/test-suite).
User-visible differences and resource policies are in [compatibility](docs/compatibility.md).
Small [semantic fixtures](tests/semantics) specify tricky behavior without requiring
readers to navigate the upstream corpus.

## Pinned language corpus

`tests/conformance/manifest.json` inventories JSONata **2.2.0**, revision
`8ee4476f8a228bfc7a62979ae0a9c13a4043cd03`: **1,679 cases in 102 groups**.
Every entry is asserted; there are no skipped or unclassified language cases.

| Outcome | Cases | Check |
| --- | ---: | --- |
| Supported result | 1,390 | JSON result or missing agrees with upstream |
| Expected error | 288 | Compilation/evaluation phase and local error kind; user messages where applicable |
| Resource limit | 1 | Non-tail factorial exceeds the guarded call stack |
| Blocked | 0 | No language case remains blocked |

Finite tail-recursive cases pass. Infinite tail recursion asserts an execution-budget
error. The corpus excludes upstream JavaScript embedding, asynchronous APIs and parser
recovery. Exact upstream diagnostic strings/codes are not a compatibility contract.

Fixtures keep upstream MIT provenance in [LICENSE](tests/conformance/LICENSE). The adapter preserves object order, imports host
bindings as lexical declarations, represents absent context explicitly and compares
unordered results as multisets. Syntax-error cases compile their original source.
Two URI fixtures escape lone-surrogate source units because Rust source is UTF-8;
JSON string values still preserve those units.

```sh
just conformance
```

To refresh fixtures, use a checkout at the pinned revision:

```sh
git clone --depth 1 --branch v2.2.0 https://github.com/jsonata-js/jsonata.git /tmp/jsonata-reference
python3 scripts/import-conformance.py /tmp/jsonata-reference
just conformance
```

The importer checks the complete inventory; manifest changes need review. A blocker
must never be promoted merely because the engine rejects it. `scripts/probe-conformance.py`
helps inspect CLI observations but does not classify them automatically.

## Regression and differential checks

JSON syntax/validation, language semantics, CLI behavior and performance budgets have
separate tests. Regression coverage includes borrowing, retained closures, dynamic
programs, effect order, demand capture, exact numeric bits/error offsets and native/tree
fallback. [Robustness testing](fuzz/README.md) adds seeded and coverage-guided boundary
checks. These are evidence, not a sandbox guarantee.

The optional `scripts/check-*.cjs` suites compare readable and seeded expressions with
the pinned upstream implementation. They exercise sequences, scalar/lexical semantics,
constructors, functions, matchers, pictures, tuples, transforms and optimized execution.
Run one family or all of them:

```sh
just build
node scripts/check-sequences.cjs /tmp/jsonata-reference target/release/jx
for suite in scripts/check-*.cjs; do
  node "$suite" /tmp/jsonata-reference target/release/jx || exit
done
```

For native parity, build with `just build-jit` and set `JX_DIFFERENTIAL_JIT=1` for the
same loop. The helper verifies the upstream revision. Ordinary `just all` and CI use
vendored fixtures and need neither Node nor an upstream checkout.
