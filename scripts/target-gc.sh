#!/usr/bin/env bash
# target-gc.sh — reclaim stale build artifacts under target/ without a full
# `cargo clean` (which would throw away the warm engine builds Tilt depends on).
#
# What grows and why: cargo never deletes an artifact whose metadata hash went
# stale. Every time a dependency of the engine crate changes (Cargo.lock moved
# 83 times in the 60 days before this script was written), or the toolchain in
# rust-toolchain.toml is bumped, every engine unit gets a new hash and the old
# rlib (~1 GB) and its incremental directory (1-3 GB) are simply orphaned — per
# unit, per profile root. Incremental *sessions* are not the problem: rustc
# keeps one finalized session per unit and deletes the previous one itself.
#
# What this does: for every profile root (target/<profile>, and the extra
# roots Tilt uses such as target/clippy/<profile>), a compile unit is "used"
# if its .fingerprint/<pkg>-<hash>/ directory was invoked within --days. The
# unit's outputs in deps/ share that <hash> suffix, so a stale unit's rlib,
# rmeta, dep-info and binaries are removed together with its fingerprint dir.
# Incremental session directories carry a different id, so they are swept by
# their own mtime (a session dir is rewritten every time its unit recompiles).
# Deleting a live artifact by mistake is safe — cargo notices the missing
# output and rebuilds that unit.
#
# Refuses to run while Tilt or cargo/rustc is running: deleting artifacts under
# an in-flight build corrupts fingerprints and forces a full rebuild.
#
# Usage: scripts/target-gc.sh [--days N] [--dry-run] [--yes]
set -euo pipefail
cd "$(dirname "$0")/.."

DAYS=7; DRY=0; YES=0
while [ $# -gt 0 ]; do
  case "$1" in
    --days) DAYS="$2"; shift 2 ;;
    --dry-run) DRY=1; shift ;;
    --yes|-y) YES=1; shift ;;
    -h|--help) sed -n '2,24p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

if tilt get uiresource clippy >/dev/null 2>&1; then
  echo "target-gc: Tilt is running. Stop it first (Ctrl-C in its terminal, or \`tilt down\`); never GC under a live build." >&2
  exit 3
fi
if pgrep -x cargo >/dev/null || pgrep -x rustc >/dev/null || pgrep -x clippy-driver >/dev/null; then
  echo "target-gc: cargo/rustc processes are running; wait for them to finish." >&2
  exit 3
fi
[ -d target ] || { echo "target-gc: no target/ directory here"; exit 0; }

echo "target-gc: before"
du -sh target target/* 2>/dev/null | sed 's/^/  /'

# Plain bash 3.2 (macOS): no mapfile/associative arrays; lists live in files.
tmp="$(mktemp -t target-gc)"; trap 'rm -f "$tmp"' EXIT
find target -maxdepth 3 -type d -name .fingerprint 2>/dev/null |
while IFS= read -r fp; do
  root="${fp%/.fingerprint}"
  # stale units: fingerprint dirs not invoked within $DAYS
  find "$fp" -mindepth 1 -maxdepth 1 -type d -mtime +"$DAYS" |
  while IFS= read -r unit; do
    echo "$unit"
    h="${unit##*-}"
    # its outputs: deps/<name>-<hash>[.ext] and deps/lib<name>-<hash>.<ext>
    find "$root/deps" -mindepth 1 -maxdepth 1 \( -name "*-$h" -o -name "*-$h.*" \) 2>/dev/null
  done
  # incremental sessions: rewritten on every recompile of their unit
  find "$root/incremental" -mindepth 1 -maxdepth 1 -type d -mtime +"$DAYS" 2>/dev/null
done > "$tmp"
count=$(wc -l < "$tmp" | tr -d ' ')
if [ "$count" = 0 ]; then
  echo "target-gc: nothing unused for more than ${DAYS} days"; exit 0
fi
total=$(tr '\n' '\0' < "$tmp" | xargs -0 du -sk 2>/dev/null | awk '{s+=$1} END{printf "%.1f GB", s/1048576}')
echo "target-gc: ${count} stale entries unused for more than ${DAYS} days, ${total}"
if [ "$DRY" = 1 ]; then
  head -40 "$tmp" | sed 's/^/  /'
  [ "$count" -gt 40 ] && echo "  ... (${count} total)"
  exit 0
fi
if [ "$YES" != 1 ]; then
  printf 'Delete them? [y/N] '
  read -r ans
  [ "$ans" = y ] || [ "$ans" = Y ] || { echo "aborted"; exit 1; }
fi
tr '\n' '\0' < "$tmp" | xargs -0 rm -rf
echo "target-gc: after"
du -sh target target/* 2>/dev/null | sed 's/^/  /'
