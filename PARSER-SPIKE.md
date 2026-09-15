# Oracle Parser Spike

Rewrite the Oracle parser as: **lexer → token stream → total grammar → versioned schema**,
in Rust, as crates with **no dependency on `phase-engine`**.

Not for upstream submission. The existing parser stays in place as the differential oracle.

## Settled decisions

| Question | Decision |
|---|---|
| Parser technology | nom 8, run over the token stream (not over `&str`) |
| Slice size | Top ~50 schema vocabulary tags ≈ 79.9% of typed nodes |
| Behavior target | Parity; do not contort to reproduce known bugs; report every divergence |
| End goal | Undecided; keep upstream and fork paths both open |

## Measured baseline (2026-09-15, Tilt down, warm target dir)

| Loop | Time |
|---|---:|
| `cargo check -p phase-engine --lib` | 10s |
| `cargo check -p phase-engine --all-targets` | 2m 40s |
| `cargo check -p phase-oracle-parse --all-targets` (spike, warm) | 0.14s |

The 16x lib-vs-all-targets gap is the ~1.03M lines of inline `#[cfg(test)]` code compiling
into the engine crate unit, plus the 1,622-module integration binary. Rust is not the
bottleneck; the test surface and the monolithic crate are.

## Corpus sizing (measured from data/card-data.json, 35,931 cards)

358,870 typed nodes, 932 distinct `type` tags.

| Vocabulary slice | Share of typed nodes |
|---|---:|
| Top 30 | 71.6% |
| Top 50 (spike target) | 79.9% |
| Top 120 | 91.6% |

812 of 932 tags cover under 9% of nodes. That tail is explicitly deferred.

## Crates

- `phase-card-schema` — versioned IR data types, serde only, zero engine coupling.
- `phase-oracle-lex` — Oracle text → tokens with spans.
- `phase-oracle-parse` — grammar over tokens → schema.
- `phase-schema-bridge` — schema → engine types. Integration checkpoints only, never in the inner loop.

## The invariant that drives the design

Every byte of input belongs to exactly one token, and every clause either consumes all of its
tokens or fails with a span. Totality is structural, not audited. If this holds, the existing
11,422-line `swallow_check.rs` auditor becomes unnecessary by construction.

## Success criteria

1. Zero unclaimed tokens across all 35,931 cards, including declined clauses.
2. On the slice: `new worse` bucket empty, `new better` non-empty.
3. Inner loop under 10s warm.
4. Grammar emittable as a printable EBNF artifact.
5. Slice line count materially below the equivalent current surface.
6. No post-hoc swallow auditor required.

## Phases

0. Loop setup and baseline. **DONE** — numbers above.
1. Lexer (1–2d).
2. Schema, top ~50 tags (2–3d). Highest overrun risk; timeboxed.
3. Grammar over tokens (~1w).
4. Differential harness vs `card-data.json`; buckets: identical / new-better / new-worse / both-decline / divergent (2–3d).
5. Verdict against criteria (1d).

## Operating notes

- Do **not** run Tilt for this work. It watches the engine source glob and fans one edit out to
  wasm, test-engine, test-ai, card-data and clippy; five of those share one target dir and one
  lock. Irrelevant to three standalone crates. Use it only at integration checkpoints.
- Dedicated `CARGO_TARGET_DIR=target-spike` (24MB, vs 35GB for the main tree).
- Isolated git worktree so the main checkout is never disturbed.
