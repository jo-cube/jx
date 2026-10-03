# Installation and releases

The CLI executable and Rust library import are both `jx`. CLI binaries are distributed
through [GitHub Releases](https://github.com/jo-cube/jx/releases); registry publication
is deferred. No release is published yet: the download examples apply once assets are
available. [Source installation](#source-builds) works now.

## Platforms

Use the target name matching your operating system and architecture:

| Platform | Archive target | Runtime baseline |
| --- | --- | --- |
| Linux x86-64 | `x86_64-unknown-linux-gnu` | glibc 2.35+ |
| Linux ARM64 | `aarch64-unknown-linux-gnu` | glibc 2.35+ |
| macOS Intel | `x86_64-apple-darwin` | macOS 11+ |
| macOS Apple Silicon | `aarch64-apple-darwin` | macOS 11+ |
| Windows x86-64 | `x86_64-pc-windows-msvc` | MSVC runtime |

CI tests normal/native builds on these five platforms. Linux/macOS have local execution
evidence; Windows runtime validation remains a CI check. macOS binaries are unsigned
and unnotarized. Other Rust targets may support the normal engine but have no release
archive here.

## Download and install

The **normal build is recommended**. Choose a published version and target from the
table. Linux example:

```sh
version=0.1.0
target=x86_64-unknown-linux-gnu
name="jx-v${version}-${target}"
archive="$name.tar.gz"
url="https://github.com/jo-cube/jx/releases/download/v${version}"
curl -fLO "$url/$archive" &&
  curl -fLO "$url/$archive.sha256" &&
  sha256sum -c "$archive.sha256" &&
  tar -xzf "$archive" &&
  mkdir -p "$HOME/.local/bin" &&
  install -m 755 "$name/jx" "$HOME/.local/bin/jx"
```

On macOS, select an `apple-darwin` target and replace `sha256sum -c` with
`shasum -a 256 -c`. Ensure `~/.local/bin` is on PATH, then run `jx --version`.
The checksum verifies downloaded bytes; obtain both files from the intended release.

Windows PowerShell example; add the extracted directory to your user PATH:

```powershell
$version = '0.1.0'
$name = "jx-v$version-x86_64-pc-windows-msvc"
$archive = "$name.zip"
$url = "https://github.com/jo-cube/jx/releases/download/v$version"
curl.exe -fLO "$url/$archive"
if ($LASTEXITCODE -ne 0) { throw 'Archive download failed' }
curl.exe -fLO "$url/$archive.sha256"
if ($LASTEXITCODE -ne 0) { throw 'Checksum download failed' }
$expected = (Get-Content "$archive.sha256").Split(' ')[0]
if ((Get-FileHash $archive -Algorithm SHA256).Hash -ne $expected) {
    throw 'Checksum mismatch'
}
Expand-Archive $archive -DestinationPath .
& ".\$name\jx.exe" --version
```

Archives contain the executable, usage/compatibility guides, licenses and `BUILD.txt`
with version, revision, target, variant, features and source timestamp. Names are:

| Variant | Linux/macOS | Windows |
| --- | --- | --- |
| Normal | `jx-vVERSION-TARGET.tar.gz` | `jx-vVERSION-TARGET.zip` |
| Native-capable | `jx-vVERSION-TARGET-native.tar.gz` | `jx-vVERSION-TARGET-native.zip` |

Each has an adjacent `.sha256` file and an equally named top-level directory.
Both variants install the same executable name, `jx` (or `jx.exe`).

## Optional native acceleration

The normal build contains the complete general runtime and has no Cranelift dependency.
The native-capable build adds acceleration for eligible numeric/boolean regions and
selected folds. It remains opt-in at execution time:

```sh
jx --jit 'price * quantity' records.ndjson
```

For that variant, append `-native` to `name` in the download examples. Unsupported regions
continue through ordinary execution. Native acceleration is most useful for heavily reused
numeric expressions; scanning-heavy workloads may gain little. Use the normal build when
you do not need it. See [performance](performance.md) for measured examples and limits.

## Source builds

Use Rust **1.98.1** or newer and a reviewed checkout:

```sh
git clone https://github.com/jo-cube/jx
cd jx
git checkout REVIEWED_COMMIT
cargo install --path crates/jx-cli --locked
```

Default features are empty. A native-capable source install uses
`cargo install --path crates/jx-cli --features jit --locked`, then runtime `--jit`.
Unsupported native targets fail with a compile-time diagnostic.
`cargo build --release --locked` builds the default engine/CLI; `cargo build --workspace`
also builds the separate native crate. Library users should follow the
[pinned Git dependency guide](embedding.md), enabling `jit` and calling `enable_native()`
only when desired. APIs are pre-release and can change.

## Preparing release archives

The manually dispatched **Release artifacts** workflow builds/tests both variants at
an existing commit/tag and uploads archives/checksums to its run. Maintainers can attach
validated files to a GitHub Release separately. The workflow never creates tags, publishes
releases or publishes crates.

The packager uses Python 3.9+ and Cargo. On a matching host with a clean tree:

```sh
SOURCE_DATE_EPOCH="$(git show -s --format=%ct HEAD)"
export SOURCE_DATE_EPOCH
export RUSTFLAGS="--remap-path-prefix=$PWD=."
# macOS: export MACOSX_DEPLOYMENT_TARGET=11.0
cargo build -p jx-cli --release --locked --target aarch64-apple-darwin
python3 scripts/package-release.py --target aarch64-apple-darwin
# Native variant:
cargo build -p jx-cli --release --features jit --locked --target aarch64-apple-darwin
python3 scripts/package-release.py --target aarch64-apple-darwin --variant native
```

The tool checks binary version/native capability before packaging. Stable member ordering,
modes, owner IDs and commit timestamps make archives deterministic for identical binaries
with the same Python/zlib. Locked dependencies, path remapping, thin LTO and one codegen
unit support reproducibility; different OS/linker combinations may produce different bytes.

Run `just all`, `just build`, source package checks, documentation/examples and relevant
platform checks. Verify checksums, inspect archive contents and smoke-test extracted binaries
with stdin, files, expression files and a failure case. Review notices after dependency or
toolchain changes: `THIRD_PARTY_LICENSES` covers normal releases, and
`python3 scripts/update-native-notices.py` refreshes `THIRD_PARTY_LICENSES_NATIVE` from the
locked native dependency graph. Both include the Rust standard library notice.

## Future registry publication

Crates have independent metadata, READMEs and MIT licenses. `just package` verifies extracted
source packages without publishing; local review can use
`cargo package --workspace --locked --allow-dirty`. Registry publication will come later.
The existing crates.io package `jx` is a different project, so a future package-name decision
is still needed. That does not require changing the Rust library import or CLI name.
