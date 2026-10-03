# Boundary fuzzing

`cargo install cargo-fuzz --locked`, then from the repository root:

```sh
cargo +nightly fuzz run boundaries -- -max_len=16384 -timeout=10
```

The target shares the ordinary deterministic mutation/property harness. It compiles
arbitrary UTF-8 expressions and validates/evaluates arbitrary JSON with bounded
expression templates and cooperative controls, checking successful serialization
and owned snapshots. Arbitrary regex execution is intentionally excluded: the regex
engine has no interruptible timeout. Keep discovered minimal regressions in ordinary
tests. Generated corpus/artifacts and optional fuzz dependencies stay outside CI.

`JX_FUZZ_ITERATIONS=100000 just robustness` extends the reproducible mutation run.
Native/interpreter adversarial-shape parity and embedding failure tests run with
`just robustness`; executable buffers/code lifetime have separate native unit tests.
