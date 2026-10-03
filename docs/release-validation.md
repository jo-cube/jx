# Release readiness validation

Local validation on **2026-10-03**, Rust/Cargo **1.98.1**. Source is the M31 working tree
based on `168bd30`; preparation remains uncommitted. No crate, tag, release or workflow
was published/dispatched. These checks establish local readiness, not hosted CI results.

| Check | Evidence |
| --- | --- |
| `just all`, `just build` | Pass: formatting, strict Clippy, default/native tests, packaging tests and allocation-asserting smoke |
| Pinned language corpus | All 1,679 classified assertions pass, including the explicit non-tail recursion guard |
| Differential suites | All 25 suites / 54,715 comparisons pass with native CLI enabled |
| Robustness | 100,000 seeded mutations plus native guard/lifetime tests pass |
| macOS arm64 | Default/native tests, strict default/native Rustdoc, both public examples, source installation and extracted archive smoke pass |
| macOS x86_64 | Full native-enabled workspace tests pass under Rosetta |
| Linux arm64 | Actual default/native tests, strict Clippy, embedding example and default/native allocation smoke pass in Debian bookworm |
| Windows x86_64 MSVC | Default/native all-target cross-checks pass; no Windows runtime execution |
| Windows aarch64 MSVC | Default all-target cross-check passes; `jit` fails with the intended unsupported-target message |
| Workflows | CI/artifact YAML passes actionlint 1.7.12, including shellcheck |
| Cargo packages | All three extracted source archives verify in default and native configurations via Cargo's temporary registry |
| Archive tests | Tar/ZIP contents, permissions, checksums and repeatability pass for all five artifact targets |

Linux used the existing local VM and `rust:1.98.1-bookworm`, image digest
`sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e`.
The VM was returned to its original stopped state. No Windows runner was available;
Windows execution and hosted Ubuntu/macOS/Windows workflow runs still need confirmation
after the maintainer commits/pushes. macOS minimum-deployment and glibc-baseline runtime
versions were not separately exercised. Release workflows do not enable native support.

Two independent clean-directory macOS arm64 release builds used the documented path
remapping/deployment flags and produced identical **2,307,072-byte** binaries:
`39ee6a7bc0864500df84f3d8fc04922bcdf835d226ab146203176d4093f84919` (SHA-256).
Identical preview archives also passed checksum/content validation and extracted-binary
execution. Their provenance explicitly marks the uncommitted working tree; they are not
published release artifacts. This verifies same-host reproducibility, not arbitrary
cross-runner bit identity. Local logs/previews are ignored under `target/m31/`.

Crates.io's `jx` name is already owned by another project. Registry publication requires
an available package name; the Rust dependency alias and CLI binary can remain `jx`.
The library needs no architectural restructuring for that metadata change.
