default:
    @just --list

build:
    cargo build --release --locked

test:
    cargo test --workspace --all-features --locked

fmt:
    cargo fmt --all

clippy:
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

check:
    cargo check --all-targets --locked

bench:
    cargo bench -p jx --bench throughput --locked

conformance:
    cargo test -p jx --test conformance --locked -- --nocapture

all:
    cargo fmt --all -- --check
    just clippy
    cargo test -p jx -p jx-cli --locked
    just test
    just packaging-tests
    just guides
    cargo bench -p jx --bench throughput --all-features --locked -- --smoke
    JX_BENCH_NATIVE_ONLY=1 JX_BENCH_NATIVE=1 cargo bench -p jx --features jit --bench throughput --locked -- --smoke

ci: all

# Native code is opt-in; both modes use the same release binary for comparisons.
bench-jit:
    JX_BENCH_NATIVE_ONLY=1 JX_BENCH_NATIVE=1 cargo bench -p jx --features jit --bench throughput --locked

bench-plan:
    JX_BENCH_NATIVE_ONLY=1 cargo bench -p jx --features jit --bench throughput --locked

bench-jit-memory:
    cargo bench -p jx --features jit --bench native_memory --locked

# Callback throughput and live heap are measured separately.
bench-runtime:
    JX_BENCH_RUNTIME_ONLY=1 cargo bench -p jx --bench throughput --locked

bench-runtime-memory:
    cargo bench -p jx --bench runtime_memory --locked

# Public API overhead, decoding, ownership and cooperative controls.
bench-embedding:
    JX_BENCH_EMBEDDING_ONLY=1 cargo bench -p jx --bench throughput --locked

# Seeded mutation properties, fallbacks and embedding failure boundaries.
robustness:
    cargo test -p jx --test robustness --all-features --locked
    cargo test -p jx-native --locked

# Builds with no native dependencies unless explicitly requested.
build-jit:
    cargo build -p jx-cli --release --features jit --locked

# Never publishes; Cargo verifies extracted source packages outside the workspace.
package:
    cargo package --workspace --locked

packaging-tests:
    python3 -m unittest discover -s scripts/tests

# Public guide examples and documentation links, without network access.
guides:
    python3 scripts/check-guides.py

# Verify and execute the exact files in an already-prepared archive.
release-smoke archive:
    python3 scripts/smoke-release.py {{quote(archive)}}
