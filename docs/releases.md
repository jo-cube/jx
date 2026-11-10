# Installation and releases

The CLI executable and Rust library import are both `jx`. CLI binaries are distributed
through [GitHub Releases](https://github.com/jo-cube/jx/releases); registry publication
is deferred. Use [source installation](#source-builds) when the requested version
has not yet been published.

## Platforms

Use the target name matching your operating system and architecture:

| Platform | Archive target | Runtime baseline |
| --- | --- | --- |
| Linux x86-64 | `x86_64-unknown-linux-gnu` | glibc 2.35+ |
| Linux ARM64 | `aarch64-unknown-linux-gnu` | glibc 2.35+ |
| macOS Intel | `x86_64-apple-darwin` | macOS 11+ |
| macOS Apple Silicon | `aarch64-apple-darwin` | macOS 11+ |
| Windows x86-64 | `x86_64-pc-windows-msvc` | MSVC runtime |

CI is configured to run workspace tests and extracted CLI archives on all five
hosts. Verify successful jobs at the candidate revision before distributing their
assets; Windows runtime validation remains pending until its job passes. macOS binaries
are unsigned and unnotarized. Other Rust targets may support the normal engine but have
no release archive here.

## Download and install

The Unix installer supports the four Linux/macOS targets above and uses `~/.local/bin`
by default. It needs `curl`, `tar` with gzip support, `awk`, standard Unix utilities,
and either `sha256sum` or `shasum`. Rust and Python are not required to install binaries.

```sh
curl -fsSL https://raw.githubusercontent.com/jo-cube/jx/main/scripts/install.sh | sh
```

Override the destination with one argument, or select an exact published tag with
`VERSION` (default: `latest`):

```sh
curl -fsSL https://raw.githubusercontent.com/jo-cube/jx/main/scripts/install.sh | sh -s -- "$HOME/bin"
curl -fsSL https://raw.githubusercontent.com/jo-cube/jx/main/scripts/install.sh | VERSION=v0.2.0 sh
```

`latest` is resolved once to a `vX.Y.Z` tag. Both the versioned archive and its checksum
are then downloaded from that exact release over HTTPS. Unknown tags, missing releases,
unsupported OS/CPU combinations and download errors fail without replacing an existing
executable. Only stable `vX.Y.Z` tags are accepted, matching the release workflow.

Before extraction, the installer checks the SHA-256 digest and requires exactly the
seven expected regular files under `jx-vVERSION-TARGET/`. Unexpected paths, duplicates,
links and other member types are rejected. It extracts only executable bytes into a
private directory on the destination filesystem, sets mode `0755`, then renames the file
into place. Failed verification or replacement keeps the previous executable intact;
temporary files are removed on exit and handled termination signals. The destination
should be a directory controlled by the installing user. No sudo, shell-profile changes
or package-manager integration is used; add the destination to PATH yourself.

The installer selects OS and CPU; it does not check the minimum glibc/macOS version.
Observe the runtime baselines in the table. Linux archives require glibc, and macOS
archives remain unsigned. Windows uses the manual ZIP instructions below. Checksums
verify downloaded bytes against the selected release; the release repository is trusted.

### Manual downloads

Use the common-IR release binary. Choose a published version and target from the
table. Linux example:

```sh
version=0.2.0
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
$version = '0.2.0'
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

Each has an adjacent `.sha256` file and an equally named top-level directory.
The executable name is `jx` (or `jx.exe`).

## Execution features

Releases contain one common-IR interpreter. There is no native archive variant
or engine-selection feature.

## Source builds

Use Rust **1.98.1** or newer and a reviewed checkout:

```sh
git clone https://github.com/jo-cube/jx
cd jx
git checkout REVIEWED_COMMIT
cargo install --path crates/jx-cli --locked
```

Default features are empty. `cargo build --release --locked` builds the engine
and CLI. Library users should follow the [pinned dependency guide](embedding.md).
APIs are pre-release and can change.

## Preparing and publishing releases

One immutable `vX.Y.Z` tag identifies both workspace packages and their source commit.
A new tag push runs the **Release** workflow; tag-update and deletion events are ignored.
Before any builds, it checks the tag against
`workspace.package.version`, every workspace package, their `Cargo.lock` entries and
jx-cli's exact internal jx dependency. All builders use the resolved commit, reusing
**CI** and the existing five native build/archive/smoke jobs. Ordinary branch/PR CI
does not duplicate tag builds. Tagged releases always run pinned-upstream differential
checks; manual preparation keeps its existing differential checkbox.

After all jobs succeed, one publisher downloads exactly the five archives and five
checksums. It rechecks their layout, checksums, version and source revision, pins their
expected hashes, then creates a draft release. Changed local files are rejected before
upload. Every uploaded asset must have the expected name, size, completed state
and server SHA-256 digest before the draft can become public. Only this job has repository
write permission. It requires an existing tag and refuses any pre-existing release or
draft; assets are never overwritten. A failed upload leaves an unpublished draft for inspection.
A timeout during the final publish request can leave a complete published release;
inspect its state before retrying.

Latest-release selection uses GitHub's automatic version-based policy, so a release
finishing later does not force itself to become Latest.

Release procedure:

1. Update the workspace version and jx-cli's exact jx dependency together; regenerate
   `Cargo.lock`, review licenses and run the checks below. Commit the reviewed candidate.
2. Optionally dispatch **Release** with that candidate ref to test preparation without
   publishing. The ref is resolved once; verified Actions artifacts are retained for 14 days.
3. After the candidate is available upstream, create an annotated `vX.Y.Z` tag at that
   exact commit and push only that tag, for example `git push origin refs/tags/v0.2.0`.
4. Inspect the **Release** run and resulting ten assets. Do not move or recreate release
   tags. Configure repository tag rules to prevent updates/deletion of `v*` tags.

If publication failed, inspect the release first. An incomplete draft must be deleted
explicitly, keeping its tag, before rerunning the failed publication job. Published
releases are never replaced. If a tag identifies bad source, use a new version. Remote tag identity is checked
before draft creation and immediately before publication, but preventing concurrent tag
mutation depends on repository rules. The tag lookup and release API do not form an atomic
transaction; avoid concurrent manual edits to tags, drafts or assets during publication.
Both lightweight and annotated tags are accepted.
Manual dispatch can only prepare/test and never publishes, even when its ref is a tag.
No workflow publishes crates or changes repository visibility.

The source tag is also a native Cargo library dependency, without registry publication:

```toml
jx = { git = "https://github.com/jo-cube/jx.git", tag = "v0.2.0" }
```

Use an actually published version. Downstream `Cargo.lock` records the exact resolved
commit; its dependency versions are governed by the downstream lockfile, not jx's.

Release validation uses Python 3.11+; CI uses 3.12. The packager also needs Cargo.
On a matching host with a clean tree:

```sh
SOURCE_DATE_EPOCH="$(git show -s --format=%ct HEAD)"
export SOURCE_DATE_EPOCH
export RUSTFLAGS="--remap-path-prefix=$PWD=."
# macOS: export MACOSX_DEPLOYMENT_TARGET=11.0
cargo build -p jx-cli --release --locked --target aarch64-apple-darwin
archive="$(python3 scripts/package-release.py --target aarch64-apple-darwin)"
just release-smoke "$archive"
```

The tool checks binary version and execution variant before packaging. Stable member ordering,
modes, owner IDs and commit timestamps make archives deterministic for identical contents
and metadata with the same Python/zlib. Locked dependencies, path remapping, thin LTO
and one codegen unit support reproducibility; different OS/linker combinations may produce different bytes.

Run `just all`, `just build`, source package checks, documentation/examples and relevant
platform checks. Verify checksums, inspect archive contents and smoke-test extracted binaries
with `just release-smoke ARCHIVE`: version/help, stdin/files, expression files, cardinality,
limits, failure exit codes, line atomicity and compatibility-feature behavior. The checker
verifies the adjacent checksum and exact archive layout before running extracted bytes.
Review `THIRD_PARTY_LICENSES` after dependency or toolchain changes; it includes
the Rust standard library notice. Native-only dependencies and notices are absent.

## Future registry publication

Crates have independent metadata, READMEs and MIT licenses. `just package` verifies extracted
source packages without publishing; local review can use
`cargo package --workspace --locked --allow-dirty`. Registry publication will come later.
The existing crates.io package `jx` is a different project, so a future package-name decision
is still needed. That does not require changing the Rust library import or CLI name.
