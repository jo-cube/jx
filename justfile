default:
    @just --list

build:
    cargo build --workspace --release --locked

test:
    cargo test --workspace --all-features --locked

fmt:
    cargo fmt --all

clippy:
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

check:
    cargo check --workspace --all-targets --locked

bench:
    cargo bench -p jx --bench throughput --locked

conformance:
    cargo test -p jx --test conformance --locked -- --nocapture

all:
    cargo fmt --all -- --check
    just clippy
    just test
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
