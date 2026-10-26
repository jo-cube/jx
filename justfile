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
    just test
    just packaging-tests
    just guides
    cargo bench -p jx --bench throughput --all-features --locked -- --smoke

ci: all

# Retained closure/sequence peak heap is measured separately from throughput.
bench-runtime-memory:
    cargo bench -p jx --bench runtime_memory --locked

# Seeded mutation properties, fallbacks and embedding failure boundaries.
robustness:
    cargo test -p jx --test robustness --all-features --locked

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
