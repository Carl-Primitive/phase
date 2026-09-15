#!/usr/bin/env bash
# verify-card.sh — the ONE Developer-track verification entrypoint for a card
# contribution (docs/AI-CONTRIBUTOR.md §6). Skills and agents call this script
# instead of listing cargo commands, so nobody can add a `--features`,
# `--profile`, `--release` or `CARGO_TARGET_DIR` that would cost a second full
# engine build.
#
# Usage:
#   scripts/verify-card.sh [--timeout SECONDS] [--no-gate-a] [--parse-diff] ["<Card Name>" ...]
#   (no card names = an engine change with no target card: steps 3-4 are skipped)
#
# What it does, in order (nothing here compiles the engine a second time):
#   1. cargo fmt --all                          (the one cargo command Tilt cannot run)
#   2. tilt-wait.sh clippy test-engine card-data (Tilt's warm engine loop; the
#                                                freshness check inside tilt-wait
#                                                guarantees the result describes
#                                                the current source tree, and
#                                                tilt-wait triggers the engine
#                                                loop's manual gate resources
#                                                itself when they are pending)
#   3. per card: supported == true and gap_count == 0 in the coverage data
#                                                that Tilt's card-data resource
#                                                just wrote (client/public/coverage-data.json)
#   4. cargo semantic-audit                     (tool profile, --features cli: the
#                                                binary gen-card-data already built)
#                                                → zero findings for each card
#   5. Gate A (scripts/check-parser-combinators.sh) unless --no-gate-a
#   6. --parse-diff: scripts/parse-diff-local.sh (published base baseline vs local
#                                                coverage; never fails the gate)
#
# Exit codes:
#   0    everything passed; last line is `verify-card PASS head=<sha> tree=<clean|dirty>`
#   1    a check failed (details above the final FAIL line)
#   2    usage error
#   3    cannot answer: Tilt is not running (or watches another checkout). Start
#        the engine loop with `tilt up -- engine` and rerun. Direct cargo is
#        deliberately NOT a fallback here.
#
# `tree=dirty` means the checks describe the working tree, not HEAD. The PR-body
# evidence line must come from a `tree=clean` run at the commit being shipped.
set -uo pipefail
cd "$(dirname "$0")/.."

TIMEOUT=900
GATE_A=1
PARSE_DIFF=0
CARDS=()
usage() {
  sed -n '2,40p' "$0" | sed 's/^# \{0,1\}//'
  exit 2
}
while [ $# -gt 0 ]; do
  case "$1" in
    --timeout) TIMEOUT="$2"; shift 2 ;;
    --no-gate-a) GATE_A=0; shift ;;
    --parse-diff) PARSE_DIFF=1; shift ;;
    -h|--help) usage ;;
    --*) echo "unknown flag: $1" >&2; usage ;;
    *) CARDS+=("$1"); shift ;;
  esac
done
command -v jq >/dev/null || { echo "verify-card: jq is required" >&2; exit 2; }

FAIL=0
step() { printf '\n==> %s\n' "$*"; }
bad()  { printf 'FAIL: %s\n' "$*" >&2; FAIL=1; }

if ! tilt get uiresource clippy >/dev/null 2>&1; then
  cat >&2 <<'MSG'
verify-card: Tilt is not running, so nothing can be measured.
  Start the engine loop in another terminal (or in the background):
      tilt up -- engine
  then rerun this script. Do not fall back to direct cargo: it builds the engine
  a second time in a second location, which is the disk problem this script exists to avoid.
MSG
  exit 3
fi

step "cargo fmt --all"
cargo fmt --all || bad "cargo fmt --all"

step "tilt-wait clippy test-engine card-data (timeout ${TIMEOUT}s)"
./scripts/tilt-wait.sh --timeout "$TIMEOUT" clippy test-engine card-data
rc=$?
case $rc in
  0) ;;
  3) echo "verify-card: tilt-wait cannot answer (exit 3): Tilt watches a different checkout, or is gone." >&2; exit 3 ;;
  124) bad "tilt-wait timed out after ${TIMEOUT}s (resources still building; rerun with a larger --timeout)" ;;
  *)
    bad "a Tilt resource is red"
    for r in clippy test-engine card-data; do
      st="$(tilt get uiresource "$r" -o json 2>/dev/null | jq -r '.status.updateStatus')"
      if [ "$st" = "error" ]; then
        printf -- '--- tilt logs %s (tail) ---\n' "$r"
        tilt logs "$r" --tail 60 2>/dev/null || true
      fi
    done ;;
esac

COV=client/public/coverage-data.json
step "coverage: $COV"
if [ ${#CARDS[@]} -eq 0 ]; then
  echo "skip (no card names given)"
elif [ ! -f "$COV" ]; then
  bad "$COV missing (the card-data resource has not produced it yet)"
else
  for card in "${CARDS[@]}"; do
    row="$(jq -c --arg c "$card" '[.cards[] | select((.card_name | ascii_downcase) == ($c | ascii_downcase))] | first // empty' "$COV")"
    if [ -z "$row" ]; then
      bad "\"$card\" is not in $COV (use the name exactly as coverage data spells it)"
      continue
    fi
    supported="$(jq -r '.supported' <<<"$row")"
    gaps="$(jq -r '.gap_count // 0' <<<"$row")"
    if [ "$supported" = "true" ] && [ "$gaps" = "0" ]; then
      echo "ok   \"$card\": supported=true gap_count=0"
    else
      bad "\"$card\": supported=$supported gap_count=$gaps"
      jq -r '.gap_details[]? | "       gap: \(.handler) — \(.source_text)"' <<<"$row" >&2
    fi
  done
fi

step "cargo semantic-audit"
if [ ${#CARDS[@]} -eq 0 ]; then
  echo "skip (no card names given)"
elif cargo semantic-audit >/dev/null; then
  for card in "${CARDS[@]}"; do
    n="$(jq -r --arg c "$card" '[.flagged_cards[] | select((.card_name | ascii_downcase) == ($c | ascii_downcase))] | map(.findings | length) | add // 0' data/semantic-audit.json)"
    if [ "$n" = "0" ]; then
      echo "ok   \"$card\": 0 semantic-audit findings"
    else
      bad "\"$card\": $n semantic-audit finding(s)"
      jq -r --arg c "$card" '.flagged_cards[] | select((.card_name | ascii_downcase) == ($c | ascii_downcase)) | .findings[] | "       \(.type): \(.oracle_line)"' data/semantic-audit.json >&2
    fi
  done
else
  bad "cargo semantic-audit exited non-zero"
fi

if [ "$GATE_A" = 1 ]; then
  step "Gate A: scripts/check-parser-combinators.sh"
  ./scripts/check-parser-combinators.sh || bad "Gate A"
fi

if [ "$PARSE_DIFF" = 1 ]; then
  step "parse diff vs published base baseline (informational)"
  ./scripts/parse-diff-local.sh || true
fi

# The MTGJSON catalogs are regenerated locally by gen-card-data whenever MTGJSON
# has moved past the committed vintage. That is expected, is refreshed upstream
# by maintainer chore PRs, and must not be committed in a card PR — so it does
# not count as a dirty tree here, but it is called out.
CATALOGS='crates/engine/data/known-tokens.toml crates/engine/data/oracle-subtypes.json crates/engine/data/mtgjson-vintage'
# shellcheck disable=SC2086
if [ -n "$(git status --porcelain --untracked-files=no -- $CATALOGS)" ]; then
  echo "note: regenerated MTGJSON catalogs are modified ($(git status --porcelain -- $CATALOGS | awk '{print $2}' | xargs)); leave them out of your card commit."
fi
HEAD_SHA="$(git rev-parse HEAD)"
if [ -n "$(git status --porcelain --untracked-files=no -- . ':(exclude)crates/engine/data/known-tokens.toml' ':(exclude)crates/engine/data/oracle-subtypes.json' ':(exclude)crates/engine/data/mtgjson-vintage')" ]; then TREE=dirty; else TREE=clean; fi
echo
if [ "$FAIL" = 0 ]; then
  echo "verify-card PASS head=$HEAD_SHA tree=$TREE cards=$(printf '%s;' "${CARDS[@]+"${CARDS[@]}"}")"
  exit 0
fi
echo "verify-card FAIL head=$HEAD_SHA tree=$TREE"
exit 1
