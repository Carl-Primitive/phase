# Oracle Parser Rewrite — Handoff

**Branch:** `parser-spike` · **Worktree:** `/Users/carl/coding/phase-parser-spike`
**Main checkout:** `/Users/carl/coding/phase` (read-only: `data/card-data.json` and engine
sources, never written)
**Status:** emitting the engine's format directly. Grammar build-out in progress.

---

## What this is

A lexer-first Oracle parser that produces the EXACT current engine card format,
verified by JSON equality against `data/card-data.json`, intended as a
like-for-like replacement the existing parser can be swapped for.

The engine-independent schema the spike started with is **retired**. It bridged
to the engine format at 95.2%, which proved independence bought nothing and cost
upstreamability. The grammar now produces engine-shaped values directly, so a
divergence is always a parser bug and never a format disagreement.

---

## Measured state (all 35,564 cards with Oracle text)

| | |
|---|---:|
| Tokens emitted | 968,616 |
| **Lexer coverage failures** | **0** |
| Cards with every line parsed | 6,286 |
| — of those, **byte-identical to the engine** | **6,067** |
| — of those, disagreeing with the engine | 213 |
| — of those, where the ENGINE declined and we parsed | 6 |
| Match rate among fully-parsed cards | **96.5%** |
| Whole-corpus exact-match rate | 17.1% |
| Lines declined (the work list) | 42,145 |
| Tests | 157 |
| Full test cycle | **0.33s** |
| Source lines across the three crates | 10825 |

Comparison is over EVERY bucket the parser fills — `keywords`, `abilities`,
`triggers`, `static_abilities` — INCLUDING each ability's `description` prose.
The spike's 838 figure excluded `description` and compared two buckets, so the
two numbers are not on the same scale.

Two deliberate exceptions, both stated by the harness rather than hidden:

* **Keyword ORDER** is compared as a multiset. The reference data is not
  order-stable for that field — the same printed "Flying, deathtouch" is
  `["Deathtouch","Flying"]` on A-Midnight Assassin and `["Flying","Deathtouch"]`
  on Aurora of Emrakul. Demanding sequence equality would measure their
  instability, not this parser's correctness.
* **Cards where the ENGINE emitted `Unimplemented` and this parser produced a
  real parse are counted as WINS, not regressions.** Six so far. `+2 Mace` is
  the clearest: the engine's name normalization eats "+2/+2" into "~/~" and its
  static parser then fails the line.
* **Cards whose abilities come from the card's TYPE LINE are counted apart.**
  A basic land's mana ability and a dual land's are not in its Oracle text —
  its whole printed text is reminder text — and this parser is given only the
  text. Synthesizing them belongs to the card-data pipeline that assembles a
  card record, not to a grammar over sentences. 71 cards today and the number
  grows with coverage, so it is reported on its own line rather than folded
  into the stop-the-line count.
* **21 cards have no Oracle-derived content.** A dual land's mana abilities come
  from its TYPE LINE, not from any sentence; its whole printed text is reminder
  text. The parser is not given the type line, so this is an input limit rather
  than a grammar gap. The harness names that bucket separately.

---

## Crates

```
crates/phase-oracle-lex/     text -> tokens with byte spans
crates/phase-oracle-ast/     engine-shaped output types (serde mirror)
crates/phase-oracle-parse/   grammar over tokens -> engine-shaped values
```

None of them depends on `phase-engine`. That is what keeps the loop at 0.33s
where `cargo check -p phase-engine --all-targets` costs 2m40s, and it is worth
protecting: the whole method below is "measure, classify, fix a class, measure
again", which only works when a full corpus pass is seconds rather than minutes.

| File | What lives here |
|---|---|
| `lex/lexer.rs` | The scanner. `verify_coverage()` is the totality invariant. |
| `lex/token.rs` | Vocabulary, each variant citing its corpus occurrence count. |
| `parse/stream.rs` | nom `Input`/`Compare` over a token slice. **Load-bearing:** `tag("destroy target creature")` means "three word tokens spelled thus", so a phrase can never match across a word boundary. |
| `parse/normalize.rs` | Self-reference → `~`, before the lexer. |
| `parse/prim.rs` | Leaf productions: words, phrases, numbers, quantities, P/T, mana symbols. |
| `parse/target.rs` | `Subject` (filter + scope + targeted-ness) and the whole filter grammar. |
| `parse/effect.rs` | Effect productions and the continuous/instant split. |
| `parse/cost.rs` | The single cost resolver. No caller inspects a component. |
| `parse/keywords.rs` | Keyword lines that carry an argument (Enchant, Equip). |
| `parse/trigger.rs` | Trigger heads. |
| `parse/line.rs` | Line splitting, sentence chaining, declines. |
| `parse/lib.rs` | `parse_card`, line classification, bucket routing. |
| `ast/*` | The serde mirror. Field sets were taken from a CENSUS of card-data.json, not from reading the engine's derives. |

---

## The three commands

```bash
cd /Users/carl/coding/phase-parser-spike
export CARGO_TARGET_DIR=$PWD/target-spike

# Inner loop (0.33s)
cargo test -p phase-oracle-lex -p phase-oracle-ast -p phase-oracle-parse

# Totality over the corpus
cargo run -q --release --example corpus_coverage --features corpus \
  -p phase-oracle-lex -- oracle-corpus.json

# Parity, the number that matters
cargo run -q --release --example parity --features corpus \
  -p phase-oracle-parse -- oracle-corpus.json
#   --show N                   print N disagreements in full
#   PARITY_SAMPLE_DECLINES=1   sample declined lines per production
#   PARITY_HEAD=destroy        sample only lines starting with that word
#   PARITY_BLOCKING=1          THE work list: most frequent declining LINES,
#                              near misses by production, and the engine's own
#                              effect vocabulary on cards we could not finish

# ONE card, when a corpus number needs turning back into a production
cargo run -q --example explain -p phase-oracle-parse -- "Card Name" "Oracle text"
```

Refresh the corpus export (needs the main checkout's `data/`, which is gitignored):

```bash
cd /Users/carl/coding/phase && python3 -c "
import json
d=json.load(open('data/card-data.json'))
cov=json.load(open('data/coverage-data.json'))
sup={c['card_name']: c['supported'] for c in cov['cards']}
BUCKETS=['keywords','abilities','triggers','static_abilities','replacements',
         'modal','additional_cost','casting_options','casting_restrictions']
out=[]
for c in d.values():
    if not c.get('oracle_text'): continue
    r={'n':c['name'],'t':c['oracle_text'],'sup':sup.get(c['name'])}
    for b in BUCKETS:
        if c.get(b): r[b]=c[b]
    out.append(r)
json.dump(out,open('/Users/carl/coding/phase-parser-spike/oracle-corpus.json','w'))
print('exported',len(out))
"
```

---

## The method that produced every gain so far

Not "read cards and add arms". Every round was:

1. Run `parity --show 600`, pipe it through a structured differ that groups
   mismatches **by JSON path** rather than by card.
2. Take the top group. It is always a CLASS — a field the engine spells
   differently, a coordination rule, a position-dependent shape.
3. Verify the rule against `card-data.json` with a census before writing code.
   Several "obvious" rules turned out to be 60/40 splits and were left alone.
4. Fix it in the one place it belongs. Re-measure.

The differ is worth rebuilding if lost; it is what turns 250 disagreements into
six actionable lines. `explain` is the companion: it turns one line of that
output back into a concrete production.

For COVERAGE rather than correctness, `PARITY_BLOCKING=1` is the tool, and its
sharpest output is the frequency census of declining LINES. Magic reuses whole
sentences across hundreds of printings, so "Devoid" appearing 134 times and
"~ can't block" 96 times is a far better target list than any census of words —
the head census scattered those same cards across a dozen unrelated buckets.

**Verify before generalizing.** Three rules that looked like judgement calls
turned out to be measurable, and each was settled by a census:

* Ability word versus keyword argument → em-dash SPACING (3,518 spaced vs 439
  unspaced, nothing between).
* `Pump` versus `PumpAll` → TARGETING, not plurality (898 vs 536).
* Which keywords may be hoisted as bare strings → checked card by card that the
  engine emits nothing else for them (12 candidates failed).

And one that was NOT: `Bounce`'s `selection` field correlates with
`controller: You` only 93/38. That is a parser quirk in the engine, not a rule,
so it was left unmodelled rather than guessed at.

---

## Where the remaining work is (measured)

| Production | Declines | What it is |
|---|---:|---|
| `spell_effect` | 19,585 | effect vocabulary — the real bottleneck |
| `trigger_effect` | 6,777 | trigger head parses, body does not |
| `trigger_head` | 5,910 | unbuilt trigger events |
| `activated_effect` | 5,051 | same body grammar, after a cost |
| `trigger_body` | 1,046 | head parses but no comma boundary follows |
| `modal_bullet` | 953 | a mode whose own body does not parse |
| `ability_cost` | 646 | remaining cost shapes |

Three quarters of all declines are the EFFECT BODY grammar, reached through four
different doors. Work there pays four times.

Named classes visible in the decline samples, roughly by mass:

1. **Keywords that generate BEHAVIOUR as well as an entry** — cycling,
   flashback, evoke, bestow, madness, echo, unearth, buyback, megamorph. Each
   needs the ability or replacement it stands for built before its keyword can
   be hoisted; until then they decline on purpose. This is now the largest
   identified block.
2. **Conditions beyond "as long as you control X"** — intervening-if,
   quantity comparisons, "if you've cast", counters-on checks.
3. **Effect verbs not yet built**: look at, prevent damage, copy, dig, put onto
   the battlefield, exile-top, reveal hand, delayed triggers.
4. **`up to N target`** (`multi_target`), and the `~'s` possessive references.
5. **Pronoun referents inside triggers** — needs the trigger's own event object
   threaded into the parse context, which the `Ctx` struct already exists for.

---

## Invariants — do not break these

1. **The lexer claims every byte.** Run `corpus_coverage` after any lexer change.
   It has been 0 failures / 968,616 tokens throughout.
2. **A line either parses completely or declines with a span and a production
   name.** There is no third outcome, and in particular none in which a
   production succeeds while leaving printed words unaccounted for. This is the
   property the existing parser's 11,422-line `swallow_check.rs` auditor exists
   to recover, and it holds here by construction.
3. **Never regress a card the engine gets right.** `parity` prints that count on
   its own line labelled stop-the-line. It has never been allowed to stay up.
4. **Decline rather than guess.** Two live examples worth preserving: a
   half-understood COST declines the whole ability, because a partially-read
   cost would make it activatable for less than it prints; and an unrecognized
   "Activate only …" is left in the effect body so the line declines, rather
   than silently losing a timing restriction.
5. **No catch-all variant anywhere in the AST.** Text the grammar cannot express
   must decline, so coverage stays measurable.

---

## Traps, in the order they cost time

1. **`cargo fmt` reflows multi-line enum variants, arrays and calls.** A patch
   written against pre-format text then applies to NOTHING and reports success.
   This silently lost five separate fixes across two sessions, including one
   that made "deals damage to each opponent" still wrong after being "fixed".
   Assert the match count before substituting.
2. **A patch script that asserts and aborts skips every later substitution.**
   Report per-substitution and continue instead.
3. **Reminder text must be dropped before the GRAMMAR, not just out of the
   rendered description.** A reminder after a sentence's period reads as a
   second, unparseable sentence and fails the whole line. This one bug was worth
   1,400 cards.
4. **In a git worktree `.git` is a FILE.** `.git/info/exclude` writes fail
   silently. Use `.gitignore`.
5. **`touch` does not force a cargo rebuild.** Timing runs that only touch a file
   prove nothing. Make a real content change and plant a compile error.
6. **`--lib` vs `--all-targets` is a 16x difference.** Always say which.
7. **Leave Tilt OFF.** It watches the engine source glob and fans one edit out to
   five resources sharing one target dir and lock. Irrelevant to three
   standalone crates.
8. **Verify with the code, not a throwaway script.** Several Python cross-checks
   were wrong and briefly contradicted a correct parser.
9. **Nested quoting is unresolvable at the lexer level** (`"` is its own open and
   close). One card, Mijo the Bull. Pinned by test; do not "fix" it.

---

## Engine format facts worth knowing before touching the AST

Each of these was verified against the corpus, and several look like engine
inconsistencies. They are reproduced rather than tidied: a like-for-like
replacement must not smuggle in a shape change.

* `GainLife` OMITS the controller (absence means "you"); `LoseLife` PRINTS it.
* An effect iterated by `player_scope` leaves its own player slot empty.
* `ChangeZoneAll` carries FOUR fields where `ChangeZone` carries eight —
  the mass form has no battlefield-entry riders. `BounceAll` has no
  `destination` where `Bounce` does.
* A granted keyword is NOT an effect. It is a `StaticAbility` inside a
  `GenericEffect`, whose `target` holds the chosen object and whose `affected`
  points back through `ParentTarget`.
* Only a static ability's LEADING verb is put in the infinitive, so
  "get +1/+1 and gains flying" keeps the second verb as printed.
* "Enchanted creature" is `AttachedTo` when a trigger WATCHES it and a typed
  filter with `EnchantedBy` when a static ability AFFECTS it.
* A spell-cast trigger does NOT wrap its filter in `StackSpell`; a targeting
  clause must.
* `is_mana_ability` and `consumes_source` are COMPUTED riders, not parsed.
* "card" names the object, not a type: "target Spirit card" is a Spirit.
* CR 102.1 runs through the whole format — a player is not an object, so an
  empty `type_filters` is the ONLY spelling of a player-shaped `Typed` filter.

---

## Format changes to propose LATER, as a separate PR

Deliberately deferred so a shape change cannot be confused with a parser
regression.

1. **Absence encodes a value.** `GainLife` omits `player` to mean "the
   controller", so a consumer treating absent as unknown is silently wrong. The
   strongest candidate, and now demonstrably inconsistent with `LoseLife`.
2. **Scope named in the variant.** `Destroy`/`DestroyAll`, `Bounce`/`BounceAll`,
   `Pump`/`PumpAll`, `ChangeZone`/`ChangeZoneAll` — and the mass forms do not
   even carry the same fields. The parser already carries scope on the subject
   and translates in one place; the engine could too.
3. **Keywords have two representations** — an array entry for bare lines, an
   effect for granted ones.

**Do NOT propose changing** `QuantityExpr::Fixed` vs `Ref{QuantityRef}`. That
layering is correct and the spike's flattened version was worse.

---

## Success criteria

| | |
|---|---|
| Zero unclaimed tokens, all 35,564 cards | **holds** (968,616 tokens, 0 failures) |
| Every decline names a production and carries a span | **holds** |
| Inner loop under 10s | **holds** (0.33s) |
| Never a card the old parser got right and the new one gets wrong | **246 outstanding**, tracked and classified |
| Exact match rate rising toward parity | 13.4% of corpus, 95.1% of parsed cards |
| Grammar emittable as a printable EBNF artifact | not built |
