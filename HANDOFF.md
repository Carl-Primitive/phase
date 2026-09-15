# Oracle Parser Rewrite — Handoff

**Branch:** `parser-spike` · **Worktree:** `/Users/carl/coding/phase-parser-spike`
**Main checkout:** `/Users/carl/coding/phase` (never modified by this work)
**Status:** spike complete, approach validated, direction changed. Ready for build-out.

---

## The decision that changed

The spike started with an engine-independent schema. **That is now dropped.**
A measured bridge from that schema to the engine's JSON reached **95.2% exact
whole-card match** against `card-data.json` with about twenty minutes of fixes,
which proved the formats are close enough that independence buys nothing and
costs upstreamability.

**New goal: a lexer-first parser that emits the EXACT current engine format,**
verified by byte-for-byte JSON equality against `card-data.json`, submittable
upstream as a like-for-like replacement.

Format improvements are a **separate, later proposal** (see "Format changes to
propose"), because changing shape and changing parser at the same time makes any
regression impossible to attribute.

---

## Why this approach is worth building (measured, not asserted)

**Totality holds at corpus scale.** The lexer claims every byte:

| | |
|---|---:|
| Cards lexed | 35,564 |
| Tokens emitted | 968,616 |
| **Unclaimed bytes** | **0** |
| Clauses through the grammar | 80,299 |
| **Clauses that silently dropped text** | **0** |

This is the property the existing parser cannot state, and why it needs an
11,422-line post-hoc auditor (`swallow_check.rs`) that is still in
"observability only" mode. A test pins the 738-card class the auditor is blind
to: `Destroy target creature with mana value 3 or less` declines with
`TrailingTokens` rather than silently widening the target to all creatures.

**Leverage compounds.** Each row adds ONE production covering a class:

| Grammar state | Clauses parsed | Share |
|---|---:|---:|
| 16 effect productions | 1,599 | 2.0% |
| + subject-sharing conjunction (~30 lines) | 1,931 | 2.4% |
| + keyword-line production (~50 lines) | 10,693 | 13.3% |

**The loop is fast** because the crates do not depend on `phase-engine`:

| Loop | Time |
|---|---:|
| `cargo check -p phase-engine --lib` | 10s |
| `cargo check -p phase-engine --all-targets` | 2m 40s |
| spike crates, full test cycle | **0.5s** |

The 16x engine gap is ~1.03M lines of inline `#[cfg(test)]` compiling into the
crate unit plus the 1,622-module integration binary. Rust is not the bottleneck.

---

## What exists now

```
crates/phase-oracle-lex/      446 lines src, 256 tests   — text -> tokens+spans
crates/phase-oracle-parse/    ~900 lines src, 186 tests  — nom grammar over tokens
crates/phase-card-schema/     ~190 lines                 — TO BE RETIRED, see below
```

- `lexer.rs` — the scanner. `verify_coverage()` is the totality invariant.
- `token.rs` — vocabulary, every variant citing its corpus occurrence count.
- `stream.rs` — nom `Input` + `Compare` over a token slice. **This is the load-bearing
  piece**: `tag("destroy target creature")` means "three word tokens spelled thus",
  so a phrase can never match across a word boundary.
- `prim.rs` / `target.rs` / `clause.rs` — leaf, target, and clause productions.
- `bridge.rs` — schema→engine JSON. **Becomes the primary output path.**
- `examples/corpus_coverage.rs` — lexer totality over all cards.
- `examples/differential.rs` — clause census + decline diagnosis.
- `examples/bridge_diff.rs` — exact-match rate vs `card-data.json`.

4 commits, clean tree.

---

## Architecture change for the next session

Retire `phase-card-schema` as a separate vocabulary. Instead:

1. Define the parser's output types to **mirror the engine's serde shape exactly**
   (field names, tags, optionality). Keep them in their own crate so the fast loop
   survives — the crate still must not depend on `phase-engine`.
2. Verify by JSON equality against `card-data.json`. That is the whole test
   strategy and it is cheap and total.
3. Defer the integration question (how the parser finally lives inside
   `phase-engine`) until parity is close. Two options, decide later:
   - extract engine AST types into a shared crate both depend on
     (`types/ability.rs` has 24 production refs into `crate::game::` — bounded but real), or
   - port the modules into `crates/engine/src/parser/` at the end and accept the slow loop then.

---

## Where the remaining work is (measured)

Declines cluster by category, not by card. 1,665 distinct heads, but the mass is
in five unbuilt categories of known shape:

| Head | Clauses | Category |
|---|---:|---|
| whenever / when / at | 16,401 | triggered abilities (21% of declines) |
| if | 4,099 | conditions / intervening-if |
| `{T}` | 3,067 | activated abilities (cost : effect) |
| this / cardname / it | 4,571 | self-reference predicates |
| enchant / equip | 1,930 | keyword lines with arguments |

Build order recommendation: **activated abilities → triggers → conditions →
statics → replacements**, because the cost/effect split is the simplest
structural addition and unlocks the `{T}` mass immediately.

---

## Traps found the hard way

1. **In a git worktree `.git` is a FILE, not a directory.** `.git/info/exclude`
   writes fail silently and 178 build artifacts landed in the first commit.
   Use `.gitignore`.
2. **`touch` alone does not force a cargo rebuild.** Timing runs that only touch a
   file report 0.1s and prove nothing. Make a real content change, and verify the
   loop by planting a deliberate compile error.
3. **`--lib` vs `--all-targets` is a 16x difference.** Always say which you measured.
4. **Leave Tilt OFF.** It watches the engine source glob and fans one edit out to
   wasm, test-engine, test-ai, card-data and clippy; five share one target dir and lock.
5. **`data/*` is gitignored**, so the worktree has no `data/`. Export the corpus
   from the main checkout (see commands below).
6. **Verify with the code, not with a throwaway script.** Two of my Python
   cross-checks were wrong and briefly contradicted a correct lexer. When they
   disagree, trust the thing under test and re-derive the script.
7. **Engine format gotchas** the bridge had to reproduce:
   - keywords are PascalCase with no spaces: `FirstStrike`, not `First strike`
   - `QuantityExpr` wraps dynamic values: `{"type":"Ref","qty":{"type":"Variable","name":"X"}}`
   - `GainLife`/`LoseLife` **omit** the player field when the subject is the controller
   - scope is named in the variant: `Destroy`/`DestroyAll`, `Bounce`/`BounceAll`
8. **Nested quoting is unresolvable at the lexer level** (`"` is its own open and
   close). One card, Mijo the Bull. Pinned by test, do not try to "fix" it.

---

## Working commands

```bash
# Always, from the worktree:
cd /Users/carl/coding/phase-parser-spike
export CARGO_TARGET_DIR=$PWD/target-spike

# Fast loop
cargo test -p phase-oracle-lex -p phase-oracle-parse

# Refresh the corpus export (needs the main checkout's data/)
cd /Users/carl/coding/phase && python3 -c "
import json
d=json.load(open('data/card-data.json'))
cov=json.load(open('data/coverage-data.json'))
sup={c['card_name']: c['supported'] for c in cov['cards']}
out=[{'n':c['name'],'t':c['oracle_text'],'sup':sup.get(c['name']),
      'kw':c.get('keywords') or [],'ab':c.get('abilities') or []}
     for c in d.values() if c.get('oracle_text')]
json.dump(out,open('/Users/carl/coding/phase-parser-spike/oracle-corpus.json','w'))
print('exported',len(out))
"

# The three measurements
cargo run -q --release --example corpus_coverage --features corpus -p phase-oracle-lex -- oracle-corpus.json
cargo run -q --release --example differential --features corpus -- oracle-corpus.json
cargo run -q --release --example bridge_diff --features corpus -- oracle-corpus.json
```

---

## Success criteria for the build-out

1. Zero unclaimed tokens across all 35,564 cards, including declined clauses. (holds today)
2. Exact JSON match rate against `card-data.json` rising toward parity; **never a
   card that the old parser got right and the new one gets wrong.**
3. Every decline names a production and carries a span. (holds today)
4. Inner loop stays under 10s.
5. Grammar emittable as a printable EBNF artifact. (not yet built)

---

## Format changes to propose LATER, as a separate PR

Deliberately deferred so a shape change cannot be confused with a parser regression.

1. **Absence encodes a value.** `GainLife` omits `player` to mean "the controller",
   so a consumer treating absent as unknown is silently wrong. This is the one
   real correctness hazard and the strongest candidate to fix first.
2. **Scope named in the variant.** 23 sibling clusters, 48 tags, 5% of the
   vocabulary: `Destroy`/`DestroyAll`, `Damage`/`DamageAll`/`DamageEachPlayer`.
   Carry scope on the target instead. The project's own CLAUDE.md names this smell.
3. **Keywords have two representations** — a `keywords` array for bare lines, an
   effect for granted ones. Every consumer handles the concept twice.

**Do NOT propose changing** `QuantityExpr::Fixed` vs `Ref{QuantityRef}`. That
layering is correct, the spike's flattened version was worse, and the bridge had
to restore it.
