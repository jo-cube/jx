# Validate and measure experimental execution

Use Rust 1.98.1 and the workspace release profile (thin LTO, one codegen unit).
Keep builds, profiles and allocation probes outside throughput windows. Run timings
sequentially on a quiet host; record source, environment and executable identities.
Legacy execution remains default. Ordinary CI downloads no upstream JavaScript.

## Correctness

```sh
just all
just build
cargo build -p jx --features experimental --example architecture_probe --release --locked
node scripts/execution-inventory.cjs target/release/examples/architecture_probe target/execution-upstream.json
python3 scripts/execution-coverage.py target/execution-upstream.json target/execution-capabilities.json --entry-inventory target/execution-upstream.json
```

All 1,679 vendored ids have asserted admission/outcomes in
`tests/conformance/execution.json`; `just all` checks them with `experimental`.
The inventory checker emits rejection reasons. Supply a saved `--entry-inventory`
for a cohort comparison. Add `--gate tests/conformance/execution.json` only for a
deliberate reviewed update. Normal validation does not change the gate.

Optional live-reference checks require an existing jsonata-js checkout pinned at
`8ee4476f8a228bfc7a62979ae0a9c13a4043cd03`; see
[conformance setup](../../CONFORMANCE.md#regression-and-differential-checks).

```sh
node scripts/execution-differential.cjs target/jsonata-reference target/release/examples/architecture_probe target/execution-generated.json
node scripts/check-execution.cjs target/jsonata-reference target/release/jx
node scripts/check-optimizing.cjs target/jsonata-reference target/release/jx
```

The original CLI numeric checks and foundation admission/generated checks are
separate. Generated comparisons assert values, errors, rejection status and complete
JSON output. Unsupported cases are never silently skipped.

## Runtime and memory

```sh
cargo bench -p jx --features experimental --bench architecture --no-run --locked
```

Cargo prints the executable path. Save an immutable copy under target and its
SHA-256. Build compared sources with identical toolchain, profile, features and
benchmark fixtures; save them as BEFORE and AFTER. Both must support selected
cases. The driver also measures legacy execution from AFTER:

```sh
python3 scripts/benchmark-execution.py BEFORE AFTER target/execution-core --ordinary --cases benchmarks/execution/workloads.json
python3 scripts/benchmark-execution.py BEFORE AFTER target/execution-calls --ordinary --calls --cases benchmarks/execution/call-cases.json
ARCH_CONVERGENCE=1 python3 scripts/benchmark-execution.py BEFORE AFTER target/execution-sequences --ordinary --expanded --cases benchmarks/execution/sequence-cases.json
ARCH_MEMORY=1 AFTER
```

`--expanded` compares only legacy/AFTER when BEFORE lacks coverage. Each version
gets fourteen warmed observations in two reversed process orders, seven/process,
at least 200 ms each. `--ordinary` disables allocation instrumentation during timing;
allocation/heap probes run separately. Raw samples, summaries and identities are
saved. `--raw` adds separately labelled validated-input execution.

Independent default-engine controls:

```sh
JX_BENCH_REALISTIC_ONLY=1 just bench
JX_BENCH_REALISTIC_ONLY=1 just bench-runtime-memory
```

## Compiler scaling and profiles

```sh
cargo build -p jx --features experimental --release --locked
rustc -O -C lto=thin -C codegen-units=1 --edition=2024 crates/jx/benches/architecture/scaling.rs --extern jx=target/release/libjx.rlib -L dependency=target/release/deps -o target/execution-scaling
SCALE_MAX=192 target/execution-scaling
ARCH_EXPANDED=1 python3 scripts/profile-execution.py AFTER target/execution-profile --cases benchmarks/execution/profile-cases.json
```

Compile the same scaling source against each library. Repeat processes; retain
parser rejections and allocations/retained/peak totals. Top-level instruction counts
exclude nested bodies. Do not execute the call-fanout family: runtime work grows
exponentially even when compilation does not. Scaling instrumentation affects
timing; ordinary warmed compilation is a separate control.

The profile driver uses macOS `sample` after readiness/warmup. Sample counts are not
throughput or precise cycle attribution. Other hosts can profile the same filtered
executable with their native sampling tools.

## Isolated actual worker

```sh
python3 scripts/check-jkq-execution.py ../jkq target/execution-jkq
JKQ_CHECK=1 target/execution-jkq/worker
python3 scripts/benchmark-jkq-execution.py target/execution-jkq/worker target/execution-worker --cases benchmarks/execution/worker-cases.json
```

The generator copies actual sibling worker modules under target and adapts only
the foundation API. It does not modify the sibling or involve broker/network work.
Record the sibling revision and check cleanliness before/after. Known compatible
worker source is revision `7eff087645f1c11e6c0754c7a7f239c3decb34a2`; the generator
records source hashes. Sibling changes may require adapter review. Validation,
drop/tombstone ordering, immutable lookup setup, payload ownership and complete
serialization must continue to match. Use `--entry BEFORE_WORKER` for a matched
saved-worker comparison.

Builds, copied workers, snapshots, raw samples, profiles and intermediate logs
belong under ignored target or external review storage, not in source commits.
