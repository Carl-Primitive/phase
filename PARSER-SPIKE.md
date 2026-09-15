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

---

# Phase 1–3 results (2026-09-15)

## Totality: CONFIRMED at corpus scale

| Measure | Result |
|---|---:|
| Cards lexed | 35,564 |
| Tokens emitted | 968,616 |
| **Unclaimed bytes** | **0** |
| Clauses parsed through the grammar | 80,299 |
| **Clauses that silently dropped printed text** | **0** |

Every decline names the production that refused and carries the span. The
738-card "dropped relative clause on target" class is pinned as structurally
impossible by test: `Destroy target creature with mana value 3 or less`
declines with `TrailingTokens` rather than widening the target.

## Leverage: CONFIRMED, and it compounds

Each row adds ONE production covering a class, never a card:

| Grammar state | Clauses parsed | Share |
|---|---:|---:|
| 16 effect productions | 1,599 | 2.0% |
| + subject-sharing conjunction (~30 lines) | 1,931 | 2.4% |
| + keyword-line production (~50 lines) | 10,693 | 13.3% |

The keyword-line production alone moved 8,762 clauses. All 17 grammar tests
stayed green across both additions.

## Differential vs the existing parser

Within the targeted slice (plain effect sentences, no trigger/cost/static):

| Bucket | Count |
|---|---:|
| Slice cards | 11,025 |
| Fully parsed by the new grammar | 830 |
| Agree with existing parser | 830 |
| **New worse (existing parsed, new is WRONG)** | **0** |
| New better | 0 |
| Not yet implemented | 8,970 |

No regressions. No wins yet either: the grammar is a strict subset so far.

## Where the remaining work is

Declines cluster by category, not by card. Top heads:

| Head | Clauses | Category |
|---|---:|---|
| whenever / when / at | 16,401 | triggered abilities (21% of declines) |
| if | 4,099 | conditions |
| `{T}` | 3,067 | activated abilities |
| this / cardname / it | 4,571 | self-reference predicates |
| enchant / equip | 1,930 | keyword lines with arguments |

1,665 distinct declining heads, but the mass is in five unbuilt *categories*,
each of known shape. That is a work list over the grammar, not a grind over cards.

## Honest limits

- 13.3% clause coverage against the existing parser's 89% card coverage. The gap
  is scope, not architecture: ~16 of 932 vocabulary tags are implemented.
- Nested quoting is unresolvable at the lexer level (1 card, pinned by test).
- Reaching parity needs triggers, activated abilities, statics, replacements and
  conditions. The measured shape of the declines supports the original 2–3 week
  estimate for the top-50 slice.
