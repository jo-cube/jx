<!-- CODEGRAPH_START -->
## CodeGraph

If `.codegraph/` exists, use `codegraph explore` or the MCP equivalent before searching
or reading indexed code. If unavailable/stale, use `rg`. Do not index on the user's
behalf or make builds/tests depend on CodeGraph.
<!-- CODEGRAPH_END -->

# Working on jx

Build an idiomatic Rust JSONata engine for compile-once/evaluate-many workloads.
Optimize single-threaded throughput, allocations, retained memory and predictable
latency. Typical records are 500 B–1 KiB; large inputs must remain viable. APIs are unstable.

## Design

- `crates/jx` owns language semantics; `jx-cli` owns NDJSON/I/O; optional `jx-native`
  owns executable code. Rust minimum/toolchain: **1.98.1**, edition 2024.
- Preserve borrowed bytes, primitive values and streaming where semantics allow.
  Retain evaluated values when correctness needs lexical lifetime or construction.
  No convenience input DOM, mandatory collection, worker pool or per-record cache.
- Keep small cohesive files, explicit data flow and clear ownership. Before a new
  abstraction, identify its concrete semantic requirement or measured benefit.
- Keep tree evaluation, bounded plans and optional native acceleration separate.
  Lower only replay-safe regions; preserve effects, validation and error order.
  Default builds must remain independent of Cranelift; native execution is opt-in.
- Engine/CLI Rust forbid unsafe. Executable invocation/release stays in
  `crates/jx-native/src/executable.rs`; allocation instrumentation is benchmark-only.
  Bounded buffers, code ownership and fallback must remain auditable.
- Every dependency needs a technical reason. Prefer simplification/deletion over
  speculative IR/JIT expansion, general frameworks, caches or compatibility layers.

## Semantics and checks

- Authority: https://docs.jsonata.org/overview.html and official jsonata-js tests.
  `CONFORMANCE.md` describes the pinned inventory; `docs/compatibility.md` records
  boundaries. Unsupported behavior must be explicit, never silently approximated.
- Tests specify behavior. Separate JSON validation, language semantics, CLI,
  conformance, regression and performance checks. Add a minimal regression first.
- Every upstream case has a manifest status and asserted outcome; no silent skips.
  Keep revisions, provenance and licenses. Ordinary CI does not download upstream JS.
- Run **`just all`** before handoff: formatting, strict Clippy, default/native tests,
  archive tests, guide examples and allocation-asserting benchmark smoke. Run
  `just build` as well.
  Hot-path changes need warmed benchmarks and profiles, not just smoke timing.
- Measure compilation separately from execution. Preserve workload/environment,
  commands and raw samples for a claim; report records/s, bytes/s and allocations
  where meaningful. Repeat noisy results and investigate regressions before optimizing.
- Keep user docs current and concise. Do not add histories or redundant status reports.
  `just` exposes Cargo commands; substantive tooling lives in `scripts/`.
  Do not publish or change release infrastructure unless requested.

Commands: `just build`, `test`, `fmt`, `clippy`, `check`, `bench`, `conformance`,
`robustness`, `all`, `ci`. See `docs/development.md` and `PERFORMANCE.md` for focused workflows.

## Local references

An optional checkout at `/Users/josh/Documents/Workspace/jo-cube/jtx` can inform test
ideas and experiments. Its structure, APIs and behavior are not authority; implement
cleanly here and resolve semantics with official sources. Do not introduce a dependency
on that checkout or mention it in product documentation without a concrete reason.
