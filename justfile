default:
    @just --list

build:
    cargo build --workspace --release --locked

test:
    cargo test --workspace --locked

fmt:
    cargo fmt --all

clippy:
    cargo clippy --workspace --all-targets --locked -- -D warnings

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
    cargo bench -p jx --bench throughput --locked -- --smoke

ci: all
