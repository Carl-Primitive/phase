//! Clause productions, and the totality rule the design rests on.
//!
//! A clause either consumes every token it was handed, or it is reported as
//! [`Effect::Unparsed`] naming the production that refused. There is no third
//! outcome, and in particular there is no outcome in which a production
//! succeeds while leaving printed words unaccounted for. That is the property
//! a post-hoc text auditor exists to recover in a parser that cannot state it.

use phase_card_schema::effect::DeclineReason;
use phase_card_schema::{Duration, Effect, PtChange, Quantity, Target};
use phase_oracle_lex::TokenKind;

use crate::prim::{counter_kind, fail, phrase, pt_pair, quantity, word, In, R};
use crate::target::target;

fn duration(i: In<'_>) -> R<'_, Duration> {
    for (p, d) in [
        ("until end of turn", Duration::EndOfTurn),
        ("until end of combat", Duration::EndOfCombat),
        ("until your next turn", Duration::YourNextTurn),
    ] {
        if let Ok((r, _)) = phrase(p)(i) {
            return Ok((r, d));
        }
    }
    fail(i)
}

fn cards_noun(i: In<'_>) -> R<'_, ()> {
    for w in ["card", "cards"] {
        if let Ok((r, _)) = word(w)(i) {
            return Ok((r, ()));
        }
    }
    fail(i)
}

fn counters_noun(i: In<'_>) -> R<'_, ()> {
    for w in ["counter", "counters"] {
        if let Ok((r, _)) = word(w)(i) {
            return Ok((r, ()));
        }
    }
    fail(i)
}

/// Keyword abilities printed as a bare line, alone or comma-separated:
/// "Flying", "Flying, vigilance", "First strike, trample".
///
/// One production covering the whole evergreen set, rather than one arm per
/// keyword. The list is the printed vocabulary; anything outside it declines
/// rather than being guessed at.
const KEYWORDS: &[&str] = &[
    "flying", "trample", "vigilance", "haste", "reach", "menace", "defender",
    "deathtouch", "lifelink", "hexproof", "shroud", "indestructible", "flash",
    "intimidate", "fear", "banding", "infect", "wither", "changeling",
    "persist", "undying", "exalted", "prowess", "skulk", "horsemanship",
    "shadow", "devoid", "ingest", "myriad", "melee", "mentor", "afterlife",
    "convoke", "delve", "cascade", "storm", "split", "double",
];

fn keyword_line(i: In<'_>) -> R<'_, Effect> {
    // Two-word keywords first, so "first strike" is not read as "first".
    for (a, b, name) in [
        ("first", "strike", "first strike"),
        ("double", "strike", "double strike"),
    ] {
        if let Ok((r, _)) = word(a)(i) {
            if let Ok((r2, _)) = word(b)(r) {
                let _ = name;
                return Ok((r2, Effect::GainKeyword { target: Target::This, keyword: name.to_string() }));
            }
        }
    }
    let Some(w) = i.first_word() else { return fail(i) };
    if !KEYWORDS.contains(&w.as_str()) {
        return fail(i);
    }
    Ok((i.take_from_n(1), Effect::GainKeyword { target: Target::This, keyword: w }))
}

/// Verb-initial clauses: the imperative voice, with the actor elided.
fn imperative(i: In<'_>) -> R<'_, Effect> {
    if let Ok((r, _)) = word("destroy")(i) {
        let (r, t) = target(r)?;
        return Ok((r, Effect::Destroy { target: t }));
    }
    if let Ok((r, _)) = word("exile")(i) {
        let (r, t) = target(r)?;
        return Ok((r, Effect::Exile { target: t }));
    }
    if let Ok((r, _)) = word("counter")(i) {
        let (r, t) = target(r)?;
        return Ok((r, Effect::CounterSpell { target: t }));
    }
    for (w, tapped) in [("tap", true), ("untap", false)] {
        if let Ok((r, _)) = word(w)(i) {
            let (r, t) = target(r)?;
            return Ok((r, Effect::SetTapped { target: t, tapped }));
        }
    }
    if let Ok((r, _)) = word("draw")(i) {
        let (r, q) = quantity(r)?;
        let (r, _) = cards_noun(r)?;
        return Ok((r, Effect::Draw { who: Target::You, amount: q }));
    }
    if let Ok((r, _)) = word("discard")(i) {
        let (r, q) = quantity(r)?;
        let (r, _) = cards_noun(r)?;
        return Ok((r, Effect::Discard { who: Target::You, amount: q }));
    }
    if let Ok((r, _)) = word("sacrifice")(i) {
        let (r, t) = target(r)?;
        return Ok((r, Effect::Sacrifice { who: t, amount: Quantity::Fixed { value: 1 } }));
    }
    // "put a +1/+1 counter on target creature"
    if let Ok((r, _)) = word("put")(i) {
        let (r, q) = quantity(r)?;
        let (r, c) = counter_kind(r)?;
        let (r, _) = counters_noun(r)?;
        let (r, _) = word("on")(r)?;
        let (r, t) = target(r)?;
        return Ok((r, Effect::PutCounter { target: t, counter: c, amount: q }));
    }
    // "return target creature to its owner's hand" / "... to your hand"
    if let Ok((r, _)) = word("return")(i) {
        let (r, t) = target(r)?;
        for p in ["to its owner's hand", "to their owner's hand", "to your hand", "to its owners hand"] {
            if let Ok((r2, _)) = phrase(p)(r) {
                return Ok((r2, Effect::ReturnToHand { target: t }));
            }
        }
        return fail(i);
    }
    fail(i)
}

/// Predicates that attach to a subject already parsed, so a conjunction can
/// reuse the leading subject: "Equipped creature gets +1/+1 AND HAS flying".
fn predicate_for(subj: Target, i: In<'_>) -> R<'_, Effect> {
    if let Ok((r, _)) = word("gets")(i) {
        let (r, (p, t, variable)) = pt_pair(r)?;
        return Ok((r, Effect::ModifyPt { target: subj, change: PtChange { power: p, toughness: t, variable } }));
    }
    for w in ["has", "have", "gains", "gain"] {
        if let Ok((r2, _)) = word(w)(i) {
            if let Ok((r3, q)) = quantity(r2) {
                if let Ok((r4, _)) = word("life")(r3) {
                    return Ok((r4, Effect::GainLife { who: subj, amount: q }));
                }
            }
            if let Ok((r3, kw)) = crate::prim::any_word(r2) {
                return Ok((r3, Effect::GainKeyword { target: subj, keyword: kw }));
            }
        }
    }
    fail(i)
}

/// Subject-initial clauses: `<target> <predicate>`.
fn subject_clause(i: In<'_>) -> R<'_, Effect> {
    let (r, subj) = target(i)?;

    if let Ok((mut rest, first)) = predicate_for(subj.clone(), r) {
        let mut chain = vec![first];
        // "gets +1/+1 and has flying" — the subject carries across the conjunction.
        while let Ok((r2, _)) = word("and")(rest) {
            match predicate_for(subj.clone(), r2) {
                Ok((r3, next)) => {
                    chain.push(next);
                    rest = r3;
                }
                Err(_) => break,
            }
        }
        if chain.len() == 1 {
            return Ok((rest, chain.pop().expect("one element")));
        }
        return Ok((rest, Effect::Sequence { effects: chain, ordered: false }));
    }

    for w in ["gains", "gain", "gets"] {
        if let Ok((r2, _)) = word(w)(r) {
            // "gains 3 life" before "gains flying": a quantity followed by the
            // noun `life` is unambiguous, and keyword grants never take a count.
            if let Ok((r3, q)) = quantity(r2) {
                if let Ok((r4, _)) = word("life")(r3) {
                    return Ok((r4, Effect::GainLife { who: subj, amount: q }));
                }
            }
            if let Ok((r3, kw)) = crate::prim::any_word(r2) {
                return Ok((r3, Effect::GainKeyword { target: subj, keyword: kw }));
            }
        }
    }

    for w in ["loses", "lose"] {
        if let Ok((r2, _)) = word(w)(r) {
            let (r3, q) = quantity(r2)?;
            let (r4, _) = word("life")(r3)?;
            return Ok((r4, Effect::LoseLife { who: subj, amount: q }));
        }
    }

    for w in ["draws", "draw"] {
        if let Ok((r2, _)) = word(w)(r) {
            let (r3, q) = quantity(r2)?;
            let (r4, _) = cards_noun(r3)?;
            return Ok((r4, Effect::Draw { who: subj, amount: q }));
        }
    }

    for w in ["discards", "discard"] {
        if let Ok((r2, _)) = word(w)(r) {
            let (r3, q) = quantity(r2)?;
            let (r4, _) = cards_noun(r3)?;
            return Ok((r4, Effect::Discard { who: subj, amount: q }));
        }
    }

    for w in ["mills", "mill"] {
        if let Ok((r2, _)) = word(w)(r) {
            let (r3, q) = quantity(r2)?;
            let (r4, _) = cards_noun(r3)?;
            return Ok((r4, Effect::Mill { who: subj, amount: q }));
        }
    }

    // "<source> deals N damage to <target>"
    for w in ["deals", "deal"] {
        if let Ok((r2, _)) = word(w)(r) {
            let (r3, q) = quantity(r2)?;
            let (r4, _) = word("damage")(r3)?;
            let (r5, _) = word("to")(r4)?;
            let (r6, t) = target(r5)?;
            return Ok((r6, Effect::DealDamage { amount: q, target: t }));
        }
    }

    fail(i)
}

/// Parse one clause and require it to be total.
///
/// `tokens` must already be the clause's own tokens. A trailing period is the
/// only slack allowed; anything else is [`DeclineReason::TrailingTokens`].
pub fn parse_clause(i: In<'_>, text: &str) -> (Effect, Option<Duration>) {
    // A keyword line may be a comma-separated list; chain it here so
    // "Flying, vigilance" is one clause rather than a decline.
    if let Ok((mut rest, first)) = keyword_line(i) {
        let mut chain = vec![first];
        loop {
            let after_comma = match rest.first() {
                Some(t) if t.kind == TokenKind::Comma => rest.take_from_n(1),
                _ => break,
            };
            match keyword_line(after_comma) {
                Ok((r, next)) => {
                    chain.push(next);
                    rest = r;
                }
                Err(_) => break,
            }
        }
        let leftover = rest
            .toks
            .iter()
            .filter(|t| !matches!(t.kind, TokenKind::Period | TokenKind::Newline | TokenKind::Comma))
            .count();
        if leftover == 0 {
            let effect = if chain.len() == 1 {
                chain.pop().expect("one element")
            } else {
                Effect::Sequence { effects: chain, ordered: false }
            };
            return (effect, None);
        }
    }

    let parsed = imperative(i).or_else(|_| subject_clause(i));

    let Ok((rest, effect)) = parsed else {
        let reason = if i.first_word().is_some() {
            DeclineReason::UnknownVerb
        } else {
            DeclineReason::UnparsedTarget
        };
        return (Effect::Unparsed { text: text.to_string(), reason }, None);
    };

    let (rest, dur) = match duration(rest) {
        Ok((r, d)) => (r, Some(d)),
        Err(_) => (rest, None),
    };

    // Totality. A trailing period is structural punctuation, not content.
    let leftover: Vec<_> = rest
        .toks
        .iter()
        .filter(|t| !matches!(t.kind, TokenKind::Period | TokenKind::Newline))
        .collect();

    if !leftover.is_empty() {
        return (
            Effect::Unparsed { text: text.to_string(), reason: DeclineReason::TrailingTokens },
            None,
        );
    }

    (effect, dur)
}
