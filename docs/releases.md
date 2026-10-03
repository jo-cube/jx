# Builds, packages and releases

Nothing is published automatically. APIs are unstable; choose a reviewed commit and
keep `Cargo.lock`. Rust/toolchain **1.98.1**, edition 2024, release thin LTO and one
codegen unit are pinned. Source installation currently uses a checkout:

```sh
cargo install --path crates/jx-cli --locked
cargo build --release --locked
```

`jx` and `jx-cli` are default workspace members. Native code is absent from these builds;
`cargo build --workspace` deliberately builds the separate native crate too. Enable it
only with `cargo build -p jx-cli --release --features jit --locked`, then pass `--jit`.
Library callers use `jx`'s `jit` feature and `enable_native()` once before sharing/cloning.
The backend is process-local host code, has no persistent executable cache, and falls
back on failed guards or compilation. Unsupported native targets fail at compilation.

## Platform policy

| Target | CI / archive |
| --- | --- |
| x86_64 Linux GNU | Ubuntu 22.04, glibc 2.35+ |
| aarch64 Linux GNU | Ubuntu 22.04 ARM, glibc 2.35+ |
| x86_64 macOS | macOS Intel runner; deployment target 11.0 |
| aarch64 macOS | macOS ARM runner; deployment target 11.0 |
| x86_64 Windows MSVC | Windows 2022 runner; runtime installed as required by the Rust MSVC target |

Default and native tests run separately on these targets. Runner labels follow the
[GitHub runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners). Portable engine builds may
work on other Rust `std` targets, but they are not claimed as tested release platforms.
Native support is bounded to the five target families above. CI checks the full corpus,
robustness tests, strict Clippy and allocation-asserting smoke; it does not download a
JavaScript language implementation. Differential checks remain an optional pinned
upstream developer workflow.

Local readiness evidence and platform limitations are recorded in
[release validation](release-validation.md). Workflow configuration is not proof of a
successful Windows run. Hosted runners can execute the workflows once these changes
are committed/pushed by the maintainer; no push or dispatch is part of preparation.

## Binary artifacts

Archive tooling uses Python 3.9+ standard library and Cargo; no installer dependencies.

The manually dispatched **Release artifacts** workflow takes an existing commit/tag,
builds the default CLI on all five targets, runs CLI tests, and uploads artifacts to the
workflow run. It does not create a tag or GitHub Release. Archives are named
`jx-vVERSION-TARGET.tar.gz` (`.zip` on Windows), with adjacent `.sha256` checksums.
They contain the executable, MIT license, third-party notices, CLI README/reference, compatibility policy
and `BUILD.txt` with revision, target, feature policy and source timestamp.

The workflow fixes toolchain/dependencies/target and remaps checkout paths. Archives
use stable member order, permissions, owner IDs and commit timestamps. This makes
archive bytes reproducible for identical input binaries with the same Python/zlib;
bit-identical executables across arbitrary OS/linker/toolchain environments are not
promised. Record runner image/toolchain when reproducing a release. macOS builds fix
`MACOSX_DEPLOYMENT_TARGET=11.0`; binaries are unsigned/unnotarized.

To reproduce on a matching host, with a clean tree:

```sh
# Substitute the current host's supported target.
SOURCE_DATE_EPOCH="$(git show -s --format=%ct HEAD)"
export SOURCE_DATE_EPOCH
export RUSTFLAGS="--remap-path-prefix=$PWD=."
# On macOS: export MACOSX_DEPLOYMENT_TARGET=11.0
cargo build -p jx-cli --release --locked --target aarch64-apple-darwin
python3 scripts/package-release.py --target aarch64-apple-darwin
```

When a maintainer explicitly chooses to publish, download the archives/checksums,
verify checksums and smoke-test extracted binaries, then attach the files to a GitHub
Release. POSIX users can verify with `sha256sum -c FILE.sha256` (macOS: `shasum -a 256 -c`);
Windows users can compare `Get-FileHash FILE.zip -Algorithm SHA256`. Extract the executable
and place it on PATH. No installer, auto-updater, signing service or registry publishing
is configured.

## Cargo source packages

All three crates have independent metadata, README and MIT license files. Dependencies
use both local paths and exact matching versions; Cargo rewrites paths to registry
versions when packaging. `jx-native` must eventually publish first, then `jx`, then
`jx-cli`; package names/availability must be reviewed before actual publication.
No publication is performed by this workflow. As checked on 2026-10-03, the
[`jx` registry name](https://crates.io/crates/jx) is already registered to a different
project; `jx-cli` and `jx-native` were unregistered, which does not reserve them.
Choose an available engine package name before registry publication. Cargo dependency
aliases can keep the Rust import `jx` and CLI name unchanged; only package/dependency
metadata needs adjustment. Repository/path/git use does not require that decision.

`just package` runs `cargo package --workspace --locked`, assembling and verifying all
packages through Cargo's temporary workspace registry. For review of uncommitted changes,
use `cargo package --workspace --locked --allow-dirty`. Archive verification exercises
extracted manifests, not just in-workspace builds. Corpus-dependent integration tests
and benchmarks stay in the repository; source packages contain library code, public
examples and their own documentation/licenses. Cargo notes the intentionally omitted
repository test/benchmark targets while packaging.

Run `just all`, `just build`, `just package`, default/native documentation/examples and
relevant platform checks before tagging. Review linked dependency notices when updating the lock/toolchain or
preparing native artifacts; the default archive carries `THIRD_PARTY_LICENSES`.
Publication itself remains a separate decision.
