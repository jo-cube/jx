#!/bin/sh
set -eu

fail() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

usage() {
    cat <<'EOF'
Usage: install.sh [destination]

Install jx to ~/.local/bin, or the specified directory.
VERSION=latest (default) or VERSION=vX.Y.Z selects the release.
Requires curl, tar, and sha256sum or shasum; never uses sudo or edits shell profiles.
EOF
}

if [ "$#" -eq 1 ]; then
    case "$1" in -h|--help) usage; exit 0 ;; esac
fi
[ "$#" -le 1 ] || { usage >&2; exit 1; }
if [ "$#" -eq 0 ]; then
    [ -n "${HOME:-}" ] || fail 'HOME is unset; provide an installation directory'
    INSTALL_DIR=$HOME/.local/bin
else
    INSTALL_DIR=$1
    [ -n "$INSTALL_DIR" ] || fail 'installation directory must not be empty'
    case "$INSTALL_DIR" in -*) fail "unknown option: $INSTALL_DIR" ;; esac
fi
case "$INSTALL_DIR" in /*) ;; *) INSTALL_DIR=./$INSTALL_DIR ;; esac
[ ! -d "$INSTALL_DIR/jx" ] || fail "installation target is a directory: $INSTALL_DIR/jx"

for TOOL in curl tar awk mktemp mkdir chmod mv rm uname; do
    command -v "$TOOL" >/dev/null 2>&1 || fail "$TOOL is required"
done
if command -v sha256sum >/dev/null 2>&1; then
    HASHER=sha256sum
elif command -v shasum >/dev/null 2>&1; then
    HASHER=shasum
else
    fail 'sha256sum or shasum is required for SHA-256 verification'
fi
LC_ALL=C
export LC_ALL
OS=$(uname -s)
ARCH=$(uname -m)
case "$ARCH" in
    x86_64) CPU=x86_64 ;;
    arm64|aarch64) CPU=aarch64 ;;
    *) fail "unsupported architecture: $ARCH" ;;
esac
case "$OS" in
    Linux) TARGET=$CPU-unknown-linux-gnu ;;
    Darwin) TARGET=$CPU-apple-darwin ;;
    *) fail "unsupported operating system: $OS" ;;
esac

valid_tag() {
    printf '%s\n' "$1" | awk '
        NR != 1 || $0 !~ /^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/ { bad = 1 }
        END { exit (bad || NR != 1) }
    '
}

BASE=https://github.com/jo-cube/jx
VERSION=${VERSION:-latest}
if [ "$VERSION" = latest ]; then
    RESOLVED=$(curl -fsSL --proto '=https' --proto-redir '=https' \
        --connect-timeout 15 --max-time 300 -o /dev/null -w '%{url_effective}' \
        "$BASE/releases/latest") || fail 'cannot resolve the latest published release; check GitHub Releases'
    case "$RESOLVED" in
        "$BASE/releases/tag/"*) VERSION=${RESOLVED#"$BASE/releases/tag/"} ;;
        *) fail 'latest release did not resolve to a jx release tag' ;;
    esac
    valid_tag "$VERSION" || fail 'latest release does not have a vX.Y.Z tag'
else
    valid_tag "$VERSION" || fail 'VERSION must be latest or vX.Y.Z without leading zeroes'
fi
NAME=jx-$VERSION-$TARGET
ARCHIVE=$NAME.tar.gz
URL=$BASE/releases/download/$VERSION
DOWNLOAD_DIR=
STAGE_DIR=
cleanup() {
    if [ -n "$STAGE_DIR" ]; then rm -rf "$STAGE_DIR"; fi
    if [ -n "$DOWNLOAD_DIR" ]; then rm -rf "$DOWNLOAD_DIR"; fi
}
trap cleanup 0
trap 'exit 1' HUP INT TERM
umask 077
DOWNLOAD_DIR=$(mktemp -d "${TMPDIR:-/tmp}/jx-install.XXXXXXXXXX") || fail 'cannot create download directory'
printf 'Downloading %s\n' "$ARCHIVE"
curl -fsSL --proto '=https' --proto-redir '=https' --connect-timeout 15 --max-time 300 \
    "$URL/$ARCHIVE" -o "$DOWNLOAD_DIR/$ARCHIVE" || fail "cannot download archive for $VERSION ($TARGET)"
curl -fsSL --proto '=https' --proto-redir '=https' --connect-timeout 15 --max-time 300 \
    "$URL/$ARCHIVE.sha256" -o "$DOWNLOAD_DIR/$ARCHIVE.sha256" || fail "cannot download checksum for $VERSION ($TARGET)"

if ! awk -v name="$ARCHIVE" '
    NR != 1 || length($1) != 64 || $1 ~ /[^0-9a-f]/ || $0 != $1 "  " name { bad = 1 }
    END { exit (bad || NR != 1) }
' "$DOWNLOAD_DIR/$ARCHIVE.sha256"; then
    fail 'invalid release checksum file'
fi
EXPECTED=$(awk '{ print $1 }' "$DOWNLOAD_DIR/$ARCHIVE.sha256")
if [ "$HASHER" = sha256sum ]; then
    ACTUAL=$(sha256sum < "$DOWNLOAD_DIR/$ARCHIVE") || fail 'cannot calculate SHA-256 checksum'
else
    ACTUAL=$(shasum -a 256 < "$DOWNLOAD_DIR/$ARCHIVE") || fail 'cannot calculate SHA-256 checksum'
fi
ACTUAL=${ACTUAL%% *}
[ "$ACTUAL" = "$EXPECTED" ] || fail 'SHA-256 checksum mismatch'

# Check both tar commands separately: a pipeline can hide a truncated archive error.
tar -tzf "$DOWNLOAD_DIR/$ARCHIVE" > "$DOWNLOAD_DIR/members" || fail 'cannot read release archive'
if ! awk -v prefix="$NAME/" '
    {
        member = substr($0, length(prefix) + 1)
        if (substr($0, 1, length(prefix)) != prefix ||
            (member != "jx" && member != "README.md" && member != "LICENSE" &&
             member != "THIRD_PARTY_LICENSES" && member != "CLI.md" &&
             member != "COMPATIBILITY.md" && member != "BUILD.txt") || seen[$0]++) bad = 1
    }
    END { exit (bad || NR != 7) }
' "$DOWNLOAD_DIR/members"; then
    fail 'unexpected, unsafe or duplicate release archive member'
fi
tar -tvzf "$DOWNLOAD_DIR/$ARCHIVE" > "$DOWNLOAD_DIR/types" || fail 'cannot inspect release archive'
if ! awk 'substr($0, 1, 1) != "-" { bad = 1 } END { exit bad }' "$DOWNLOAD_DIR/types"; then
    fail 'release archive must contain only regular files'
fi

mkdir -p "$INSTALL_DIR" || fail "cannot create installation directory: $INSTALL_DIR"
STAGE_DIR=$(mktemp -d "$INSTALL_DIR/.jx-install.XXXXXXXXXX") || fail "cannot stage installation in $INSTALL_DIR"
# Archive paths never become filesystem paths; rename from this filesystem preserves the old binary on failure.
tar -xzOf "$DOWNLOAD_DIR/$ARCHIVE" "$NAME/jx" > "$STAGE_DIR/jx" || fail 'cannot extract release executable'
[ -s "$STAGE_DIR/jx" ] || fail 'release executable is empty'
chmod 0755 "$STAGE_DIR/jx" || fail 'cannot set executable permissions'
mv -f "$STAGE_DIR/jx" "$INSTALL_DIR/" || fail "cannot install jx in $INSTALL_DIR"
printf 'Installed %s to %s/jx\n' "$VERSION" "$INSTALL_DIR"
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *) printf 'Add %s to your PATH to run jx.\n' "$INSTALL_DIR" ;;
esac
