# Working on jx

Build a high-performance, idiomatic Rust implementation of the full JSONata language.
The dominant workload is compile once/evaluate many independent 500 B–1 KiB records;
large records must remain viable. Single-threaded throughput, allocation, cache
behavior and predictable latency matter more than compile startup. APIs are unstable.

## Boundaries and decisions

- `crates/jx` owns parsing, semantics and evaluation; `crates/jx-cli` owns NDJSON,
  files, buffering and backpressure. Rust minimum/toolchain: **1.98.1**, edition 2024.
- Prefer borrowed bytes, primitives and streaming where semantics allow. No DOM
  for convenience, mandatory collection, engine worker pools or per-record caches.
  Allocate when correctness needs ownership, construction or retention.
- Small cohesive files, explicit data flow and auditable hot paths. Before a major
  abstraction, identify its concrete current or next-milestone requirement.
- No speculative IR, JIT/Cranelift, SIMD, generic runtime frameworks, migration
  layers or compatibility scaffolding. Introduce complexity only with semantic
  need or reproducible profiling evidence. Delete superseded designs.
- Production Rust forbids unsafe. The isolated benchmark allocation counter is
  the documented exception. Every dependency needs a technical reason.

## Semantics and evidence

- Authority: https://docs.jsonata.org/overview.html and official jsonata-js tests.
  `CONFORMANCE.md` tracks supported, unsupported and deferred semantics. Update it
  with behavior changes; never silently approximate unsupported features.
- Tests specify behavior: keep syntax validation, semantics, CLI, upstream cases
  and benchmarks separate. Add a minimal regression before fixing a bug.
- Every imported upstream case has a manifest status and asserted outcome; no
  silent skips. Preserve provenance/licenses and pin revisions.
- Measure compile separately from warmed execution. Report records/s, bytes/s and
  allocations where meaningful; retain workload/environment/commands. Investigate
  regressions and repeat noisy results before optimizing. No unscoped speed claims.
- Keep README, ARCHITECTURE, CONFORMANCE and PERFORMANCE concise and accurate.
  See ARCHITECTURE's next milestone before expanding scope.

## Commands

`just build`, `test`, `fmt`, `clippy`, `check`, `bench`, `conformance`, `all`, `ci`.
Run **`just all`** before handoff (format check, strict Clippy, tests, benchmark smoke
including allocation assertions). Use full `just bench` for hot-path changes.
Keep `Cargo.lock`; ordinary CI never downloads an upstream language implementation.
`just` orchestrates visible Cargo commands; substantive tools live in `scripts/`.
Do not publish crates or add release infrastructure unless requested.

## Local exploration

If `.codegraph/` exists, use `codegraph explore` (or its MCP equivalent) before
text search/read when locating code. If unavailable/stale, use `rg`. Do not index
on the user's behalf or make any build/test workflow depend on CodeGraph.

The optional local reference repository `/Users/josh/Documents/Workspace/jo-cube/jtx`
can inform experiments, test ideas and benchmark design. Its source structure,
APIs, implementation choices and behavior are not authoritative. Implement cleanly
here and resolve semantic questions with official sources. Avoid mentioning the
reference in product documentation or introducing any dependency on its checkout.
