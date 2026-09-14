#!/usr/bin/env bash
set -euo pipefail

# phase.rs onboarding bootstrap.
#
# Two axes:
#
#   Mode axis — what gets fetched/built:
#     * Full mode (default): everything an interactive human dev needs to run
#       the app in a browser, including the five Scryfall sidecars —
#       image/printing data and the set-icon catalog — consumed at runtime by
#       the React frontend.
#     * Engine mode (--engine, env PHASE_SETUP_ENGINE=1): the card / rules-
#       engine contributor bootstrap. MTGJSON + card data + Comprehensive
#       Rules + git hooks, and nothing else: no Scryfall sidecars, no
#       `pnpm install` / `npm install`, no WASM build. pnpm is not required.
#       This is everything docs/AI-CONTRIBUTOR.md §6 verification consumes.
#     * Agent mode (--agent, env PHASE_SETUP_AGENT=1): engine mode, plus the
#       card data is generated inline (not deferred to Tilt) so the files
#       exist when the script exits. Historically this only skipped the Scryfall
#       sidecars. They are runtime-only image data — no Rust or frontend test
#       depends on them (the one vitest test that names them mocks `fetch`).
#       Use this for LLM-driven contributors running the docs/AI-CONTRIBUTOR.md
#       developer track — saves a multi-hundred-MB Scryfall bulk download with
#       zero impact on cargo / clippy / test / gen-card-data / coverage signal.
#
#   Build axis — whether to eagerly build WASM + card-data:
#     * Tilt mode (default when `tilt` is on PATH): skips the eager builds
#       because `tilt up` rebuilds both via the `wasm` and `card-data`
#       resources on first start. Avoids fighting Tilt for the cargo target
#       lock the moment the user starts the dev loop.
#     * Manual mode (--no-tilt or PHASE_SETUP_NO_TILT=1, or simply no `tilt`
#       on PATH): runs `build-wasm.sh` + `gen-card-data.sh` inline so the repo
#       is test-ready without Tilt.
#
# Caddy/SSL is intentionally NOT invoked — it's only needed for LAN HTTPS
# (WebRTC P2P guesting) and is gated behind `tilt up -- https`. See
# scripts/setup-ssl.sh + Caddyfile if you want it.

NO_TILT="${PHASE_SETUP_NO_TILT:-0}"
AGENT="${PHASE_SETUP_AGENT:-0}"
ENGINE="${PHASE_SETUP_ENGINE:-0}"
for arg in "$@"; do
  case "$arg" in
    --no-tilt)         NO_TILT=1 ;;
    --engine)          ENGINE=1 ;;
    --agent|--no-scryfall) AGENT=1 ;;
    -h|--help)
      sed -n '3,30p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "unknown arg: $arg" >&2
      echo "  --engine            card/engine contributor mode: MTGJSON + card data + CR only (no pnpm, no WASM)" >&2
      echo "  --agent             --engine, plus generate card data inline (LLM contributor mode)" >&2
      echo "  --no-tilt           skip Tilt detection; eager-build WASM + card-data" >&2
      echo "  -h, --help          this message" >&2
      exit 2
      ;;
  esac
done

# Normalize env-var booleans so PHASE_SETUP_AGENT=true / yes / on all work,
# not just the literal "1".
for var in NO_TILT AGENT ENGINE; do
  case "$(eval echo \$$var)" in
    1|true|TRUE|yes|YES|on|ON) eval "$var=1" ;;
    *)                          eval "$var=0" ;;
  esac
done

# Agent mode means "produce the data files cargo coverage / semantic-audit /
# integration tests need, on a one-shot basis." The Tilt skip-eager-builds
# optimization assumes the user is about to run `tilt up` and would otherwise
# fight Tilt for the cargo target lock — neither is true for an LLM
# contributor. Force NO_TILT in agent mode so card-data.json is guaranteed to
# exist when setup.sh exits.
if [ "$AGENT" = 1 ]; then
  ENGINE=1
fi
# Engine mode always generates card data inline, even when Tilt is installed.
# gen-card-data.sh promotes a newer MTGJSON token/subtype catalog into
# crates/engine/data/ — an engine input. If Tilt's card-data resource does that
# AFTER clippy/test-engine have already built the engine against the committed
# catalog, every engine root rebuilds a second time on first start. Promoting
# first means `tilt up -- engine` builds the engine once.
if [ "$ENGINE" = 1 ]; then
  NO_TILT=1
fi

echo "=== phase.rs Setup ==="
if [ "$AGENT" = 1 ]; then
  echo "    (mode: agent — engine only, card data generated inline)"
elif [ "$ENGINE" = 1 ]; then
  echo "    (mode: engine — MTGJSON + card data + CR; no Scryfall, no pnpm, no WASM)"
fi
echo ""

# --- Preflight: hard tools ---
# curl is required by gen-card-data.sh, fetch-comp-rules.sh, and every
# gen-scryfall-*.sh — preflight here so missing-curl fails with a tidy
# message instead of a deep stack trace from inside a child script.
missing=()
for tool in cargo jq curl; do
  command -v "$tool" >/dev/null 2>&1 || missing+=("$tool")
done
# pnpm only feeds the client; engine mode never touches it.
if [ "$ENGINE" != 1 ]; then
  command -v pnpm >/dev/null 2>&1 || missing+=("pnpm")
fi
if [ "${#missing[@]}" -ne 0 ]; then
  echo "ERROR: missing required tools: ${missing[*]}" >&2
  echo "  cargo: https://rustup.rs/" >&2
  echo "  pnpm:  https://pnpm.io/installation" >&2
  echo "  jq:    https://stedolan.github.io/jq/" >&2
  echo "  curl:  preinstalled on macOS/Linux; Windows: winget install cURL.cURL" >&2
  exit 1
fi

# --- Preflight: pnpm major must match client/package.json's packageManager ---
# pnpm >= 10 stopped reading the "pnpm" field in package.json. Running it in
# client/ silently drops that file's `pnpm.overrides` (the supply-chain pins for
# serialize-javascript, ws, brace-expansion, postcss, …) from pnpm-lock.yaml,
# writes a stray client/pnpm-workspace.yaml, and then fails `pnpm install` with
# ERR_PNPM_IGNORED_BUILDS because `onlyBuiltDependencies` is ignored too. That
# leaves a dirty tree and a lockfile that would ship without its security
# overrides, so treat a major mismatch as fatal.
#
# The check resolves pnpm from client/, not from here: `packageManager` is
# directory-scoped, so corepack and pnpm >= 10 pick it up only when the working
# directory is under client/. The root can legitimately resolve a different,
# newer pnpm while client/ resolves the pin — measuring the root would reject
# that valid setup before installing anything. See scripts/lib/pnpm-preflight.sh.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/lib/pnpm-preflight.sh
source "$SCRIPT_DIR/lib/pnpm-preflight.sh"
if [ "$ENGINE" != 1 ]; then
  pnpm_preflight_check client || exit 1
fi

# --- Preflight: soft tool (tilt-dev/tilt, NOT other CLIs named "tilt") ---
# Multiple unrelated binaries ship as `tilt` (e.g. Go template tools). The
# tilt-dev/tilt binary is the only one whose help text references the
# `Tiltfile` manifest, which has been the stable identifier across every
# release since 0.x. Use that as a positive identity check — `command -v
# tilt` alone is not sufficient.
USE_TILT=0
if [ "$NO_TILT" != 1 ] && command -v tilt >/dev/null 2>&1; then
  if tilt --help 2>&1 | grep -q "Tiltfile"; then
    USE_TILT=1
  else
    echo "Note: found a 'tilt' binary on PATH but it isn't tilt-dev/tilt"
    echo "      ('Tiltfile' not mentioned in --help). Falling back to inline"
    echo "      WASM + card-data build. Install from https://tilt.dev if you"
    echo "      want the watched-rebuild dev loop."
  fi
fi

FAIL=0

# --- Scryfall sidecars (skipped in agent mode) ---
# These are runtime-only image data for the React frontend. No Rust or vitest
# test depends on them — see docs/AI-CONTRIBUTOR.md and CLAUDE.md.
if [ "$ENGINE" = 1 ]; then
  echo "Step 1: Skipping Scryfall sidecars (engine mode — frontend-only image data)."
else
  echo "Step 1: Fetching Scryfall sidecars (parallel)..."
  ./scripts/gen-scryfall-images.sh         & PID_IMAGES=$!
  ./scripts/gen-scryfall-token-images.sh   & PID_TOKEN_IMAGES=$!
  ./scripts/gen-scryfall-printings.sh      & PID_PRINTINGS=$!
  # Locale card-art maps. Sourced from MTGJSON rather than Scryfall bulk, but
  # the same category of artifact: runtime-only frontend image data, needed
  # only when a non-English UI language is selected.
  ./scripts/gen-scryfall-locale-images.sh  & PID_LOCALE_IMAGES=$!
  # Set-icon catalog (icon_svg_uri + release dates) for the draft/Sealed set
  # picker — metadata that drives set-icon images. Fetched from the small
  # /sets endpoint, not the bulk download.
  ./scripts/gen-scryfall-sets.sh           & PID_SETS=$!

  wait $PID_IMAGES        || FAIL=1
  wait $PID_TOKEN_IMAGES  || FAIL=1
  wait $PID_PRINTINGS     || FAIL=1
  wait $PID_LOCALE_IMAGES || FAIL=1
  wait $PID_SETS          || FAIL=1
  if [ $FAIL -ne 0 ]; then
    echo "ERROR: Scryfall sidecar fetch failed." >&2
    exit 1
  fi
fi

# Comprehensive Rules — gitignored, non-fatal on failure.
if [ ! -f docs/MagicCompRules.txt ]; then
  echo ""
  echo "Fetching MTG Comprehensive Rules (local dev reference only)..."
  ./scripts/fetch-comp-rules.sh || echo "  (skipped — run ./scripts/fetch-comp-rules.sh later)"
fi

# --- Frontend deps (parallel-safe with cargo work below) ---
PID_PNPM=""
PID_WORKER=""
if [ "$ENGINE" = 1 ]; then
  echo ""
  echo "Step 2: Skipping frontend dependencies (engine mode)."
else
  echo ""
  echo "Step 2: Installing frontend dependencies..."
  (cd client && pnpm install) &
  PID_PNPM=$!

  # The lobby worker is a separate npm project (its own package-lock.json), and
  # Tilt's 'lobby-worker' resource runs `npm run dev` from it. Without this the
  # resource comes up red on a fresh clone and deck URL import stays broken.
  # Guarded on presence for the same reason release.yml guards its deploy job:
  # commits older than the Worker have no lobby-worker/, and setup must not hard
  # fail there.
  if [ -d lobby-worker ]; then
    (cd lobby-worker && npm install) &
    PID_WORKER=$!
  fi
fi

# --- Card-data (+ WASM outside engine mode) ---
if [ "$USE_TILT" = 1 ]; then
  echo ""
  echo "Step 3: Tilt detected — skipping eager WASM + card-data build."
  echo "        \`tilt up\` will run both on first start via the"
  echo "        'wasm' and 'card-data' resources."
elif [ "$ENGINE" = 1 ]; then
  echo ""
  echo "Step 3: Building card-data (inline, before Tilt, so the engine is built once)..."
  ./scripts/gen-card-data.sh || FAIL=1
  if [ -n "$(git status --porcelain -- crates/engine/data/known-tokens.toml crates/engine/data/oracle-subtypes.json crates/engine/data/mtgjson-vintage)" ]; then
    echo "        Note: gen-card-data promoted a newer MTGJSON catalog into crates/engine/data/."
    echo "        That is expected and correct for local builds. Do NOT commit those files in a"
    echo "        card PR — a maintainer refreshes them in dedicated chore PRs."
  fi
else
  echo ""
  echo "Step 3: Building WASM + card-data (parallel)..."
  ./scripts/gen-card-data.sh & PID_CARDS=$!
  ./scripts/build-wasm.sh    & PID_WASM=$!

  wait $PID_CARDS || FAIL=1
  wait $PID_WASM  || FAIL=1
fi

if [ -n "$PID_PNPM" ]; then
  wait $PID_PNPM || FAIL=1
fi
if [ -n "$PID_WORKER" ]; then
  wait $PID_WORKER || FAIL=1
fi
if [ $FAIL -ne 0 ]; then
  echo "ERROR: setup step failed (see logs above)." >&2
  exit 1
fi

# --- Git hooks ---
echo ""
echo "Step 4: Configuring git hooks..."
git config --local include.path ../.gitconfig

echo ""
echo "Done!"
echo ""
if [ "$AGENT" = 1 ]; then
  echo "Agent mode complete. Card data is generated. Next: \`tilt up -- engine\` (engine loop:"
  echo "card-data + clippy + test-engine), then \`./scripts/verify-card.sh \"<Card>\"\` to"
  echo "verify. See docs/AI-CONTRIBUTOR.md."
elif [ "$ENGINE" = 1 ]; then
  echo "Engine mode complete. Next: run \`tilt up -- engine\` to start the engine loop"
  echo "(card-data + clippy + test-engine; first start is a cold build, ~15-30 min)."
  echo "Verify a card with \`./scripts/verify-card.sh \"<Card Name>\"\`."
  echo "Budget: the warm engine loop holds ~30-40 GB under target/. Anything more"
  echo "means a second engine build was started somewhere — see docs/AI-CONTRIBUTOR.md §2.5."
elif [ "$USE_TILT" = 1 ]; then
  echo "Next: run \`tilt up\` to start the dev loop (wasm + card-data + frontend)."
  echo "      Add \`-- server\` / \`-- test\` / \`-- lint\` to start optional groups;"
  echo "      \`tilt up -- engine\` is the engine-only loop (card-data + clippy + test-engine)."
else
  echo "Next: run \`cd client && pnpm dev\` to start the dev server."
fi
echo ""
echo "Optional: LAN HTTPS / WebRTC P2P guesting requires a Caddy reverse proxy."
echo "          See scripts/setup-ssl.sh, then \`tilt up -- https\`. Not needed for"
echo "          single-machine dev."
