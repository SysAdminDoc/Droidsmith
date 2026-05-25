#!/usr/bin/env bash
# Fetch Google's Android Platform-Tools and stage adb / fastboot as
# Tauri sidecars under src-tauri/binaries/<target-triple>/.
#
# POSIX side of fetch-platform-tools.ps1 — handles Linux + macOS targets.
# Binaries are NOT committed to the repo; release CI fetches per build.
#
# Pinned channel: stable. Bumping the version is a deliberate PR; never silent.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="$REPO_ROOT/src-tauri/binaries"

usage() {
    cat <<'EOF'
usage: fetch-platform-tools.sh [--force] [--channel stable|preview]

Stages adb + fastboot for every supported POSIX target triple:

  - x86_64-unknown-linux-gnu
  - aarch64-unknown-linux-gnu  (uses the x86_64 binary; native arm64
                                Linux build doesn't exist upstream yet)
  - x86_64-apple-darwin
  - aarch64-apple-darwin

Pass --force to re-fetch even if staged binaries exist.
EOF
}

FORCE=0
CHANNEL=stable
while [[ $# -gt 0 ]]; do
    case "$1" in
        --force)   FORCE=1; shift ;;
        --channel) CHANNEL="$2"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *)         echo "unknown arg: $1" >&2; usage; exit 2 ;;
    esac
done

case "$(uname -s)" in
    Linux*)
        OS=linux
        TRIPLES=("x86_64-unknown-linux-gnu" "aarch64-unknown-linux-gnu")
        ADB_NAME=adb
        FB_NAME=fastboot
        ;;
    Darwin*)
        OS=darwin
        TRIPLES=("x86_64-apple-darwin" "aarch64-apple-darwin")
        ADB_NAME=adb
        FB_NAME=fastboot
        ;;
    *)
        echo "[fetch-platform-tools] unsupported host: $(uname -s)" >&2
        exit 1
        ;;
esac

# Pinned upstream metadata. Bump by following
# https://developer.android.com/tools/releases/platform-tools and
# fetching the matching .sha256.
case "$CHANNEL" in
    stable)
        URL_LINUX="https://dl.google.com/android/repository/platform-tools_r35.0.2-linux.zip"
        SHA_LINUX="PLACEHOLDER-replace-with-real-hash-on-release-PR"
        URL_DARWIN="https://dl.google.com/android/repository/platform-tools_r35.0.2-darwin.zip"
        SHA_DARWIN="PLACEHOLDER-replace-with-real-hash-on-release-PR"
        VERSION="35.0.2"
        ;;
    preview)
        URL_LINUX="https://dl.google.com/android/repository/platform-tools-latest-linux.zip"
        SHA_LINUX="SKIP"
        URL_DARWIN="https://dl.google.com/android/repository/platform-tools-latest-darwin.zip"
        SHA_DARWIN="SKIP"
        VERSION="rolling"
        ;;
    *)
        echo "[fetch-platform-tools] unknown channel: $CHANNEL" >&2
        exit 2
        ;;
esac

if [[ "$OS" == "linux" ]]; then
    URL="$URL_LINUX"
    SHA="$SHA_LINUX"
else
    URL="$URL_DARWIN"
    SHA="$SHA_DARWIN"
fi

staged_complete=1
for triple in "${TRIPLES[@]}"; do
    [[ -x "$BIN_DIR/adb-$triple" ]] || staged_complete=0
    [[ -x "$BIN_DIR/fastboot-$triple" ]] || staged_complete=0
done
if [[ "$staged_complete" == "1" && "$FORCE" == "0" ]]; then
    echo "[fetch-platform-tools] All $OS sidecars already staged. Use --force to re-fetch."
    exit 0
fi

mkdir -p "$BIN_DIR"
TMP="$(mktemp -d -t droidsmith-pt-XXXXXX)"
trap 'rm -rf "$TMP"' EXIT

echo "[fetch-platform-tools] Downloading $URL ..."
ZIP="$TMP/platform-tools.zip"
curl --fail --location --silent --show-error "$URL" -o "$ZIP"

if [[ "$SHA" != "SKIP" ]]; then
    if command -v sha256sum >/dev/null 2>&1; then
        ACTUAL="$(sha256sum "$ZIP" | awk '{print $1}')"
    elif command -v shasum >/dev/null 2>&1; then
        ACTUAL="$(shasum -a 256 "$ZIP" | awk '{print $1}')"
    else
        echo "[fetch-platform-tools] no sha256 tool available; refusing to proceed" >&2
        exit 1
    fi
    EXPECTED="$(echo "$SHA" | tr '[:upper:]' '[:lower:]')"
    if [[ "${ACTUAL,,}" != "$EXPECTED" ]]; then
        echo "[fetch-platform-tools] SHA-256 mismatch! expected $EXPECTED got $ACTUAL" >&2
        exit 1
    fi
    echo "[fetch-platform-tools] SHA-256 verified."
else
    echo "[fetch-platform-tools] $CHANNEL channel: SHA-256 verification skipped (rolling)."
fi

EXTRACT="$TMP/extracted"
mkdir -p "$EXTRACT"
unzip -q "$ZIP" -d "$EXTRACT"

ADB_SRC="$EXTRACT/platform-tools/$ADB_NAME"
FB_SRC="$EXTRACT/platform-tools/$FB_NAME"
[[ -f "$ADB_SRC" ]] || { echo "adb not found in archive" >&2; exit 1; }
[[ -f "$FB_SRC"  ]] || { echo "fastboot not found in archive" >&2; exit 1; }

for triple in "${TRIPLES[@]}"; do
    install -m 0755 "$ADB_SRC" "$BIN_DIR/adb-$triple"
    install -m 0755 "$FB_SRC"  "$BIN_DIR/fastboot-$triple"
    echo "[fetch-platform-tools] Staged adb+fastboot for $triple"
done

echo "[fetch-platform-tools] Done. Channel=$CHANNEL Version=$VERSION OS=$OS"
