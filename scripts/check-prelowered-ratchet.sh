#!/usr/bin/env bash
# Plan 05b ratchet: the `OracleNodeIr::PreLowered*` producer count may only go
# down.
#
# Why this is a grep and not a type-level check: the point is to catch a
# producer being *written*, not a variant being *reachable*. A type-level check
# would go green the moment the last variant is deleted — that is the end
# state, not the tracking mechanism. Before this gate existed, "is Plan 05b
# done?" was answerable only by archaeology, which is how the `#[allow(dead_code)]`
# on `OracleNodeIr::Static` stayed in the tree for nine days after the work that
# was supposed to retire it had already landed.
#
# Contract (per plan §6.1):
#   * ceiling, not equality — reducing a count passes, raising it fails
#   * a parser file carrying `PreLowered` with no ledger entry fails, so a new
#     producer cannot be added in a third file and escape the ratchet
#   * slack (count below ceiling) passes, but prints the exact edit to tighten
#     it, because the burn-down is only visible in `git log` if each tranche
#     lowers its own number
#
# Generated snapshots under `parser/**/snapshots/` are excluded: they are
# output, not source, and their counts move for reasons unrelated to producers.
#
# Usage: scripts/check-prelowered-ratchet.sh
#
# Deliberately avoids `declare -A` (bash 4+): macOS ships bash 3.2 as
# `/bin/bash` (and often as the first `bash` on PATH), where associative
# arrays aren't available and this script would silently no-op every check.
# Parallel indexed arrays + linear lookup replace the two maps; ledgers here
# are small (dozens of entries at most), so the O(n) lookup cost is immaterial.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
LEDGER="$SCRIPT_DIR/prelowered-ratchet.txt"
SCOPE="crates/engine/src/parser"
NEEDLE="PreLowered"

cd "$REPO_ROOT"

if [[ ! -f "$LEDGER" ]]; then
  echo "prelowered-ratchet: ledger not found at $LEDGER" >&2
  exit 1
fi

# --- ledger -> parallel arrays ----------------------------------------------
ceiling_paths=()
ceiling_limits=()
while read -r path limit _rest; do
  [[ -z "${path:-}" || "$path" == \#* ]] && continue
  if ! [[ "$limit" =~ ^[0-9]+$ ]]; then
    echo "prelowered-ratchet: malformed ledger line for '$path' (ceiling '$limit')" >&2
    exit 1
  fi
  ceiling_paths+=("$path")
  ceiling_limits+=("$limit")
done < "$LEDGER"

ceiling_index_of() {
  local target="$1" i
  for i in "${!ceiling_paths[@]}"; do
    [[ "${ceiling_paths[$i]}" == "$target" ]] && { echo "$i"; return 0; }
  done
  return 1
}

# --- measured counts ---------------------------------------------------------
actual_paths=()
actual_counts=()
while IFS=: read -r path count; do
  [[ -z "${path:-}" ]] && continue
  # Native Windows ripgrep emits backslashes; ledger paths use Git's slashes.
  path="${path//\\//}"
  actual_paths+=("$path")
  actual_counts+=("$count")
done < <(
  rg --count-matches --glob '*.rs' --glob '!**/snapshots/**' "$NEEDLE" "$SCOPE" 2>/dev/null || true
)

actual_index_of() {
  local target="$1" i
  for i in "${!actual_paths[@]}"; do
    [[ "${actual_paths[$i]}" == "$target" ]] && { echo "$i"; return 0; }
  done
  return 1
}

status=0
slack=()

# Over ceiling, or present with no ledger entry.
for i in "${!actual_paths[@]}"; do
  path="${actual_paths[$i]}"
  count="${actual_counts[$i]}"
  if ! j="$(ceiling_index_of "$path")"; then
    echo "prelowered-ratchet: FAIL — $path has $count '$NEEDLE' occurrence(s) but no ledger entry." >&2
    echo "    A new PreLowered producer in a new file is exactly what this gate exists to catch." >&2
    echo "    If this is intentional, add it to scripts/prelowered-ratchet.txt with a reason." >&2
    status=1
    continue
  fi
  limit="${ceiling_limits[$j]}"
  if (( count > limit )); then
    echo "prelowered-ratchet: FAIL — $path has $count '$NEEDLE' occurrence(s), ceiling is $limit." >&2
    echo "    Plan 05b converts PreLowered producers to IR nodes; the count may only decrease." >&2
    status=1
  elif (( count < limit )); then
    slack+=("$path $limit -> $count")
  fi
done

# A ledger entry that has reached zero should be removed, not left at 0.
for j in "${!ceiling_paths[@]}"; do
  path="${ceiling_paths[$j]}"
  limit="${ceiling_limits[$j]}"
  if ! actual_index_of "$path" >/dev/null && [[ "$limit" != "0" ]]; then
    slack+=("$path $limit -> 0  (entry can be deleted)")
  fi
done

if (( ${#slack[@]} > 0 )); then
  echo "prelowered-ratchet: ${#slack[@]} entry(ies) below ceiling — tighten scripts/prelowered-ratchet.txt in this commit:"
  printf '    %s\n' "${slack[@]}"
fi

if (( status != 0 )); then
  exit 1
fi

echo "Gate P PASS (PreLowered ratchet: no producer count increased)"
