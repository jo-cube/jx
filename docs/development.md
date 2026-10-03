# Contributing and testing

Use Rust **1.98.1** (edition 2024), Cargo and [just](https://just.systems).
Python 3.9+ runs archive tests/tooling. Keep `Cargo.lock`; ordinary checks require no
JavaScript implementation or external corpus once dependencies are cached.

## Repository map

- `crates/jx`: reusable parser, compiler and runtime; public examples and benchmarks.
- `crates/jx-cli`: synchronous NDJSON reader/writer and CLI policy.
- `crates/jx-native`: optional bounded Cranelift backend; excluded from default builds.
- `tests/semantics`: small readable behavior/regression cases.
- `tests/conformance`: pinned upstream corpus, license and reviewed manifest.
- `scripts`: import/differential checks and deterministic archive tooling.
- `fuzz`: optional coverage-guided tests sharing the ordinary boundary harness.

[Architecture](../ARCHITECTURE.md), [compatibility](compatibility.md),
[conformance](../CONFORMANCE.md) and [AGENTS.md](../AGENTS.md) hold the durable rules.
Prefer small changes and explicit ownership. Add a minimal regression before fixing
a semantic bug; do not emulate unsupported behavior silently.

## Commands

| Command | Purpose |
| --- | --- |
| `just build` | Locked release engine/CLI build, without native dependencies |
| `just build-jit` | Explicit native-enabled release CLI |
| `just check` | Default engine/CLI and all target kinds |
| `just fmt` | Format Rust |
| `just clippy` | Strict all-feature workspace Clippy |
| `just test` | All-feature workspace tests |
| `just conformance` | Pinned language inventory |
| `just robustness` | Seeded boundary properties and native safety tests |
| `just all` / `just ci` | Formatting, strict Clippy, default/native tests, archive tests, guide examples and benchmark smoke |
| `just bench` | Warmed execution/compilation and allocation measurements |
| `just package` | Assemble and verify source archives, without publishing |
| `just guides` | Check documentation links and execute Rust guide examples offline |
| `just release-smoke ARCHIVE` | Verify checksum/contents and execute an extracted CLI |

`just all` deliberately exercises the optional backend as well as default builds.
Default-only checks are `cargo test --locked` and `cargo clippy --all-targets --locked -- -D warnings`.
Use a supported native host for the full workflow. **CI** keeps formatting, strict
Clippy, source packaging, Rustdoc, guide examples, archive tooling and allocation smoke
in one Linux job. Five host jobs run default/native tests and release-archive smoke in
separate steps; both modes include all 1,679 conformance outcomes and robustness checks.
Windows executes the extracted `.exe`, not a cross-built placeholder.

The same workflow serves manual **Release artifacts** preparation. Its candidate ref
is resolved once; platform jobs use that exact commit. Only release preparation uploads
verified archives. Actions are commit-pinned and Rust caches are keyed per host target.
Platform jobs are configured coverage, not evidence of a completed run.

Optional differential checks are available through the CI dispatch checkbox and enabled
by default for release preparation. They fetch the pinned JSONata revision and compare
both normal/native CLI modes. Ordinary push/PR CI and `just all` use vendored fixtures
and do not require Node or an upstream checkout.

```sh
cargo run -p jx --example embedding --locked
printf '%s\n' '{"price":2.5,"quantity":3}' |
  cargo run -p jx --example stream --locked -- 'price * quantity'
RUSTDOCFLAGS='-D warnings' cargo doc -p jx --no-deps --locked
```

See [performance](performance.md) for benchmark selection, allocation/retention
measurements and comparison discipline. See [conformance](../CONFORMANCE.md) for fixture
refresh and optional differential checks, and [fuzzing](../fuzz/README.md) for adversarial
boundary testing. Keep failing discoveries as small ordinary regression tests.

## Release checks

Run `just all`, `just build`, `just package`, documentation/examples and the relevant
platform checks. Packaging a working tree for review uses
`cargo package --workspace --locked --allow-dirty`. Inspect extracted archives rather
than relying only on in-workspace builds. [Release preparation](releases.md) covers
platforms, feature policy, checksums, licenses and reproducibility.
