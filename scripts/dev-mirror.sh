#!/usr/bin/env bash
# POSIX-side dev-mirror. The PowerShell version (dev-mirror.ps1)
# handles the VMware Shared Folders -> C:\tmp\Droidsmith case; this
# script is the equivalent for WSL / Linux / macOS where the repo lives
# on a slow path (Dropbox, network mount, /mnt/c) and Vite/Cargo would
# pay a real fsync tax.
#
# Default destination: $HOME/.droidsmith-mirror
#
# Safety:
#   - rsync --delete will REMOVE files in the destination that aren't
#     in the source. To prevent the script wiping an unrelated folder,
#     a destination that exists and is non-empty must contain the
#     sentinel file .droidsmith-mirror. Use --force to bootstrap.
#
# Usage:
#   ./scripts/dev-mirror.sh                    one-shot mirror
#   ./scripts/dev-mirror.sh --watch            mirror, then keep in sync
#   ./scripts/dev-mirror.sh --reverse          copy build outputs back
#   ./scripts/dev-mirror.sh --dest /tmp/foo    custom destination
#   ./scripts/dev-mirror.sh --force            override the sentinel guard

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="${HOME:-/tmp}/.droidsmith-mirror"
WATCH=0
REVERSE=0
FORCE=0
SENTINEL=".droidsmith-mirror"

usage() {
    cat <<'EOF'
usage: dev-mirror.sh [--watch] [--reverse] [--force] [--dest <path>]

Mirrors this repo to a non-HGFS / non-NFS local path so Vite + Cargo
work at full speed. The mirror is strict — files removed in the source
disappear from the destination.

Excludes: node_modules/, target/, .git/, src-tauri/{target,gen}
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --watch)   WATCH=1; shift ;;
        --reverse) REVERSE=1; shift ;;
        --force)   FORCE=1; shift ;;
        --dest)    DEST="$2"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *)         echo "unknown arg: $1" >&2; usage; exit 2 ;;
    esac
done

if [[ "$REVERSE" == "1" ]]; then
    SRC="$DEST"
    DST="$REPO_ROOT"
    # Reverse-mirror requires the repo root to be a git tree so we
    # don't accidentally rsync into the wrong directory.
    if [[ ! -d "$DST/.git" ]]; then
        echo "[dev-mirror] reverse target $DST is not a git repo; refusing" >&2
        exit 1
    fi
else
    SRC="$REPO_ROOT"
    DST="$DEST"
fi

# Sentinel guard. Only applied in forward mode (reverse mode targets
# the repo itself, which we already verified is a git tree).
if [[ "$REVERSE" == "0" && -d "$DST" ]]; then
    if [[ "$(ls -A "$DST" 2>/dev/null)" != "" && ! -f "$DST/$SENTINEL" && "$FORCE" == "0" ]]; then
        echo "[dev-mirror] $DST is not empty and has no $SENTINEL sentinel." >&2
        echo "             Refusing to rsync --delete. Use --force if you're sure." >&2
        exit 1
    fi
fi

mkdir -p "$DST"

# Drop the sentinel BEFORE rsync so --delete keeps it (it's also in
# the exclude list to ensure the source isn't asked to provide it).
if [[ "$REVERSE" == "0" && ! -f "$DST/$SENTINEL" ]]; then
    cat > "$DST/$SENTINEL" <<EOF
Droidsmith dev-mirror destination. Safe to delete.
Source: $REPO_ROOT
EOF
fi

if ! command -v rsync >/dev/null 2>&1; then
    echo "[dev-mirror] rsync not found — install rsync (apt/brew/pacman)" >&2
    exit 1
fi

# Excludes mirror dev-mirror.ps1.
RSYNC_EXCLUDES=(
    --exclude="node_modules/"
    --exclude="target/"
    --exclude=".git/"
    --exclude=".dev-mirror/"
    --exclude="src-tauri/target/"
    --exclude="src-tauri/gen/"
    --exclude=".droidsmith-mirror"
)

do_mirror() {
    echo "[dev-mirror] $SRC -> $DST"
    rsync -a --delete "${RSYNC_EXCLUDES[@]}" "$SRC/" "$DST/"
    echo "[dev-mirror] done"
}

do_mirror

if [[ "$WATCH" == "0" ]]; then
    exit 0
fi

# Polling fallback: inotify-tools is Linux-only, fswatch is macOS-only.
# A 1-second poll is good enough for dev-mirror and keeps the script
# dependency-free.
echo "[dev-mirror] watching $SRC (Ctrl+C to stop) — 1s poll"
PREV_FINGERPRINT=""
while true; do
    # Coarse fingerprint: latest mtime under the source tree, excluding
    # the same paths rsync skips. find -newer is per-file; a single
    # `find -printf` of the max mtime is fast even on big trees.
    CUR=$(find "$SRC" \
        -path "$SRC/node_modules" -prune -o \
        -path "$SRC/target" -prune -o \
        -path "$SRC/.git" -prune -o \
        -path "$SRC/src-tauri/target" -prune -o \
        -path "$SRC/src-tauri/gen" -prune -o \
        -type f -printf '%T@\n' 2>/dev/null | sort -n | tail -1)
    if [[ "$CUR" != "$PREV_FINGERPRINT" ]]; then
        if [[ -n "$PREV_FINGERPRINT" ]]; then
            do_mirror
        fi
        PREV_FINGERPRINT="$CUR"
    fi
    sleep 1
done
