#!/usr/bin/env bash
# parse-diff-local.sh — "did my change move parser output?" without a second
# engine build.
#
# Mirrors the CI step "Parse-detail diff vs base baseline" (.github/workflows/
# ci.yml): main-push CI publishes coverage-data-<engine-source-hash>.json for
# every main commit, so the BASE side of the diff is a download, not a build.
# The HEAD side is the coverage file Tilt's `card-data` resource (or
# scripts/gen-card-data.sh) already wrote for the working tree.
#
# This replaces the old procedure of checking out base and candidate into two
# detached worktrees, each with its own CARGO_TARGET_DIR, and building the tool
# binaries in both (measured at 28 GB per worktree before CARGO_INCREMENTAL=0,
# 11 GB after — per parser change).
#
# Usage: scripts/parse-diff-local.sh [base-ref]
#   base-ref defaults to the merge-base with upstream/main (or origin/main).
# Output: target/parse-diff/parse-diff.md (also printed) and parse-diff.json.
# Exit 0 always unless misused — this is a review aid, never a gate.
set -uo pipefail
cd "$(dirname "$0")/.."

BASE_REF="${1:-}"
if [ -z "$BASE_REF" ]; then
  for r in upstream/main origin/main; do
    if git rev-parse -q --verify "$r" >/dev/null 2>&1; then BASE_REF="$r"; break; fi
  done
fi
[ -n "$BASE_REF" ] || { echo "parse-diff-local: no upstream/main or origin/main ref; pass a base ref" >&2; exit 2; }

BASE_SHA="$(git merge-base HEAD "$BASE_REF")" || { echo "parse-diff-local: cannot find merge-base with $BASE_REF" >&2; exit 2; }
BASE_HASH="$(./scripts/engine-source-hash.sh "$BASE_SHA")"
HEAD_HASH="$(./scripts/engine-source-hash.sh HEAD)"
if [ -n "$(git status --porcelain --untracked-files=no -- crates/engine Cargo.lock)" ]; then
  echo "note: engine sources are modified in the working tree; the hash below describes HEAD, the coverage file describes the tree." >&2
elif [ "$BASE_HASH" = "$HEAD_HASH" ]; then
  echo "Engine source unchanged vs $BASE_REF ($BASE_SHA) — no parse change possible."
  exit 0
fi

HEAD_COV=client/public/coverage-data.json
[ -f "$HEAD_COV" ] || { echo "parse-diff-local: $HEAD_COV missing — let Tilt's card-data resource finish first" >&2; exit 2; }

OUT=target/parse-diff
mkdir -p "$OUT"
URL="https://data.phase-rs.dev/parse-baselines/coverage-data-${BASE_HASH}.json"
if ! curl -fsSL --retry 3 --retry-delay 2 "$URL" -o "$OUT/coverage-base.json"; then
  echo "No published baseline for base $BASE_SHA (hash $BASE_HASH): $URL" >&2
  echo "CI will post the parse diff on the PR instead." >&2
  exit 0
fi

# Same profile + features as gen-card-data.sh built the tool binaries with, so
# this is a fingerprint-fresh `cargo run`, not a rebuild.
cargo run --quiet --profile tool --features cli --bin coverage-parse-diff -- \
  "$OUT/coverage-base.json" "$HEAD_COV" \
  --base-sha "$BASE_SHA" \
  --markdown "$OUT/parse-diff.md" --json "$OUT/parse-diff.json" || exit 0
cat "$OUT/parse-diff.md"
