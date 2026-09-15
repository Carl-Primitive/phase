# PR notes — streamline-card-loop (DROP THIS FILE BEFORE OPENING THE UPSTREAM PR)

Working notes for the upstream PR from `streamline-card-loop` (Carl-Primitive/phase) to
phase-rs/phase. The commit message on the branch is the PR description draft; this file
holds the evidence, the open questions, and the procedure. `git rm` it (or `git rebase -i`
the notes commit away) before `gh pr create`.

## What the branch changes (one commit)

- Tiltfile: `tilt up -- engine` (card-data + build-native + test-engine + clippy, client
  resources removed) and `tilt up -- data` (card-data only). Plain `tilt up` unchanged.
- Tiltfile: build-native / test-engine / test-ai share ONE package selection
  (`NATIVE_TEST_PACKAGES`), runners filter with nextest `-E 'package(...)'`. Root cause
  fixed: `-p phase-engine` alone vs `-p phase-engine -p phase-ai` unify tracing-core
  differently (`["once_cell","std"]` vs `["default","once_cell","std"]`, via phase-ai's
  tracing-subscriber), so every phase-engine unit had two metadata hashes and test-engine
  recompiled the whole engine (17 min, ~10 GB) right after build-native had. In the engine
  loop phase-ai is dropped from build-native entirely (~1 GB saved; clippy still checks it).
- scripts/verify-card.sh: single verification entrypoint (fmt → tilt-wait → per-card
  coverage → semantic-audit → Gate A). Exit 3 = Tilt down; no direct-cargo fallback.
  Treats the locally regenerated MTGJSON catalogs as not-dirty and tells you not to commit them.
- scripts/parse-diff-local.sh: base-vs-candidate parse diff by downloading the published
  baseline (same artifact PR CI diffs against) — replaces building base+candidate in two
  detached worktrees with their own CARGO_TARGET_DIRs.
- scripts/target-gc.sh: age-based sweep of stale units (fingerprint age → hash-matched
  deps; incremental by mtime); refuses to run under Tilt/cargo.
- scripts/setup.sh --engine (no pnpm/npm/WASM/Scryfall; generates card data INLINE before
  Tilt so the promoted catalog does not trigger a second engine round); --agent implies it.
- .cargo/config.toml: coverage / parser-gaps / preset-audit pin `--features cli` like every
  other tool-profile alias and gen-card-data → one tool-profile engine build, not two.
- .githooks/pre-push: reuses Tilt artifacts; the four extra-build checks (clippy with
  proptest, `cargo check --release`, proptest tests, tool-profile regen) moved behind
  PHASE_PREPUSH_FULL=1 (CI runs them anyway); frontend lint/type-check only when client/ or
  the wasm crates changed; phase-ai lib tests only when a test-ai resource exists.
- docs/AI-CONTRIBUTOR.md: Quickstart; Developer track = Rust + Tilt (no pnpm); §2.5/§6 use
  setup --engine / tilt up -- engine / verify-card.sh; "never verify in a second worktree";
  catalog files are never committed in a card PR (maintainer chore PRs refresh them).
- CLAUDE.md, project-reference, engine-implementer, engine-implementation-executor,
  contribute-card.js, deck-contribute.js, pr-review-comment-resolver, and 9 skills: the
  "if Tilt up … else direct cargo" template → "through Tilt; start it if down"; measurement
  worktrees removed; workflow commits exclude the catalog files.

## Measurements (this machine: M-series 8 cores, 460 GB disk; 2026-09-14)

Engine crate: 1,694,148 lines in crates/engine/src (~430k non-test, ~1.26M inline tests)
plus 535k lines of integration tests. One dev-profile compile: libengine rlib 0.86 GB,
incremental 2.2–3.4 GB per unit.

Before (stock instructions, reported by Carl): previous checkout exceeded 100 GB of target/
and was deleted. Reproduced mechanism in this checkout within one afternoon:
- 3 generations of every engine unit in target/debug (v0.82 cold build; v0.83 rebuild after
  fast-forward; the tracing-core-skewed test-engine variant), each ~1 GB rlib + ~3 GB
  incremental, none ever deleted by cargo → target/ reached 66 GB before any card work.
- The executor's parser measurement built base AND candidate in detached worktrees
  (its own comment measured 28 GB per worktree, 11 GB with CARGO_INCREMENTAL=0).
- The pre-push hook = 4 extra engine builds per push under profiles nothing else uses.
- `cargo coverage` without `--features cli` = a second tool-profile engine build.

After cleanup with the unified config: 66 GB → 11 GB (3.3 GB third-party deps in debug,
0.6 GB clippy, 7.5 GB tool). Steady state expected ≈ debug ~15 GB + clippy ~9 GB + tool
~4 GB ≈ 30 GB with incremental on. `scripts/target-gc.sh` keeps it there.

Cold-build timings (heavily contended: iOS phase-ffi build + Xcode + simulator running
alongside; load 40–140): build-native 25 min, test-engine 17 min compile + 25 min tests
(28,386 tests), clippy 38 min, card-data ~11 min (tool compile ×2 because the fresh MTGJSON
promoted a newer token catalog mid-run — hence setup --engine now generates first).
TODO before submitting: re-measure a quiet cold `tilt up -- engine` and a warm
verify-card.sh run on the Agent Frank Horrigan branch; put both in the PR body.

## Open questions for maintainers (state them in the PR)

1. `tilt up` default left unchanged on purpose (engine loop is opt-in via `-- engine`).
   Would they prefer engine-only as the default for the AI-contributor track?
2. pre-push: are the four legacy checks wanted by default for humans? They are opt-in now.
3. `setup.sh --agent` now implies `--engine` (no pnpm install) — agents touching client/
   must run plain setup.sh. OK?
4. RESOLVED on this branch: gen-card-data.sh no longer promotes the tracked catalogs locally
   (PHASE_PROMOTE_CATALOGS=1 opts in; GitHub Actions promotes by default). The first card run
   showed why: the promoted catalog turned test-engine red at base (see Measurements).
5. Long term: move the 1.26M inline test lines out of the lib, then split parser/types/game
   into crates — the only change that makes an engine edit stop recompiling everything.

## Procedure (also in the single project memory file)

- Fork main = mirror of upstream/main. Tooling lives on `streamline-card-loop`; rebase it
  on upstream/main after each fetch. Cut card branches from it.
- Ship a card PR without the tooling commit: throwaway `git worktree` (git ops only, no
  cargo), cherry-pick the card commit onto upstream/main, push, `gh pr create`. Never check
  out a stock-Tiltfile branch in the watched checkout while Tilt runs.
- Card branch `card/agent-frank-horrigan` exists at the tooling head with no commits yet;
  gap = `Static:Unrecognized(it attacked this turn)`; 7 other cards share the phrase.
## First card run through the loop: Agent Frank Horrigan (2026-09-14)

Branch `card/agent-frank-horrigan` cut from the tooling head; engine-implementer pipeline, Tilt-only
verification, no direct cargo, no second worktree for anything but git.

### What was slow or awkward (feedback that revised this branch)

1. **The committed token overlay is stale against this week's MTGJSON, so test-engine is red at base
   for every contributor.** setup --engine / gen-card-data promoted a newer known-tokens.toml
   (Reality Fracture ships the Heartwood token that overlay row 1cb84b18… hand-adds); tokens-gen
   printed the "delete the stale row" advisory, and the engine's own guard test
   `token_presets::tests::committed_overlay_rows_merge_clean_against_the_catalog` (build.rs embeds the
   working-copy catalog) fails. verify-card.sh therefore reports FAIL on test-engine no matter what the
   card change does. Upstream already has the fix in flight (chore PR #8793 refreshes the catalogs).
   Getting a true PASS locally means restoring the two promoted files to HEAD after the last card-data
   run and paying one extra engine rebuild (build.rs rerun-if-changed). Open question 4 in this file is
   now concrete: the loop should not promote tracked catalogs into a contributor's tree by default.
2. **Plan review took 8 rounds (~2 h 50 m) for a ~30-line parser change** because planners and
   reviewers were forbidden to run anything and reasoned statically about GameScenario behaviour
   (blocker prompt timing, empty-library game-over, no layer pass at build()) — three material findings
   and one real engine defect that one test run would have surfaced. Fixed on this branch: the
   probe policy in engine-planner / review-engine-plan still said "isolated CARGO_TARGET_DIR"; it now
   runs a throwaway integration test through Tilt's `test-engine-focus`; card-test gained foot-guns
   7–9; engine-planner Step 2 / review-engine-plan check 3 gained the write-before-read ordering
   check; engine-implementer enters surgical mode on the first spot-only round.
3. **Every fix round re-ran the full 28k-test suite** (~11–15 min test time each, three times).
   Fixed: manual `test-engine-focus` resource (same NATIVE_TEST_PACKAGES artifacts, nextest filterset
   from `.tilt-test-focus`), wired into the executor agent, CLAUDE.md, project-reference,
   AI-CONTRIBUTOR §6 and every skill checklist. Not available to this run (the running Tilt watches
   the card branch's stock Tiltfile).
4. **Tilt keeps only the last two builds per resource**, so cold-loop timings must be captured as they
   happen; tilt-wait/verify-card cannot report them afterwards.
5. **The real engine finding**: `commit_attack_declaration` flushed layers before writing
   `creatures_attacked_this_turn`, so any static gated on "attacked this turn" was stale for the whole
   declare-attackers priority window. Found in plan round 7, fixed in the card commit (class-level).

### Measurements (Agent Frank Horrigan, 2026-09-14, PR phase-rs/phase#8887)

Machine under contention the whole time (an iOS release build + xcodebuild from another session;
load 20–140, briefly 380–546).

| Stage | Wall | Notes |
|---|---|---|
| Start | 19:15 | target/ 21 GB; Tilt cold loop (started 17:44) still finishing |
| Cold `tilt up -- engine` | 17:44 → 19:36 | clippy ~106 min, build-native 10m28s compile, test-engine 19 min (1154 s of tests), card-data 90 s link |
| Plan loop (8 rounds, 1 design blocker found in round 7) | 19:16 → 22:06 | ~2 h 50 m; rounds 3–6 were one or two nits each |
| Executor round 1 | 22:06 → 23:01 | edits 10 min; Tilt: build-native 141 s, clippy 881 s, test-engine 932 s, card-data 258 s; test-only fix: build-native 19 s, clippy 87 s, test-engine 661 s, card-data not rerun |
| verify-card at the candidate (warm) | 21 s / 22 s | FAIL only on the pre-existing catalog guard test; everything else green |
| Implementation review 1 → fix (3 LOW applied, 1 deferred) → review 2 (1 LOW) | 23:01 → 23:50 | the fix round paid two more full test-engine cycles (721 s, 661 s) |
| Catalog restore rebuild | 23:36 → abandoned | build.rs rerun → build-native + clippy + test-engine again; the PR was opened before it finished, at the maintainer's direction |
| PR opened | 23:58 | target/ 38 GB (35 GB after Tilt stopped; target-gc found nothing older than 7 days) |

Engine compiles in the run: cold 3 (build-native, clippy, tool) + edit round 3 incremental + fix round 2
incremental + catalog restore 3 = 11 compile passes, of which 3 (restore) and ~4 (clippy/card-data on
edits that did not need them) are avoided by the manual-gate + no-promotion changes on this branch.

verify-card.sh itself: 21–22 s on a warm loop; its cost is entirely the Tilt cycles it waits on.
