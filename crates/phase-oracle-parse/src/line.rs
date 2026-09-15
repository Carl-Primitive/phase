//! Line classification and ability assembly.
//!
//! A printed LINE is the unit the engine turns into one `AbilityDefinition` or
//! one `TriggerDefinition`, so the line — not the sentence — is this module's
//! unit of work. Sentences inside a line become a `sub_ability` chain, in
//! written order (CR 608.2c).

use phase_oracle_ast::{
    AbilityCondition, AbilityCost, AbilityDefinition, AbilityKind, Duration, Effect, PlayerScope,
    StaticAbility, SubAbilityLink,
};
use phase_oracle_lex::{Token, TokenKind};

use crate::effect::ClauseFacts;
use crate::prim::{phrase_alt, In};
use crate::stream::Tokens;

/// Why a line did not parse. Structural: each variant names the production that
/// refused, so a census of declines is a work list over the grammar rather than
/// a list of card names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclineReason {
    /// No production matched the clause's head.
    UnknownVerb,
    /// The head matched but its object did not parse.
    UnparsedTarget,
    /// Everything matched, but tokens were left over. This is the decline the
    /// totality rule produces, and the one a post-hoc text auditor exists to
    /// recover in a parser that cannot state it directly.
    TrailingTokens,
    /// The line is a shape the grammar has no production for at all.
    UnknownLineShape,
    /// A cost was present but not fully understood. Declining is mandatory
    /// here: a half-parsed cost would make the ability activatable for less
    /// than it prints.
    UnparsedCost,
}

/// A refusal, always carrying the span that caused it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decline {
    pub production: &'static str,
    pub reason: DeclineReason,
    pub text: String,
    pub start: usize,
    pub end: usize,
}

/// One printed line of Oracle text, as tokens plus its rendered description.
pub struct Line<'a> {
    pub toks: &'a [Token],
    /// The line's printed text with reminder spans removed and `~` for the
    /// card's own name — exactly the engine's `description` field.
    pub description: String,
    pub start: usize,
    pub end: usize,
}

/// Split a token slice into printed lines at newlines.
pub fn lines<'a>(toks: &'a [Token], src: &str) -> Vec<Line<'a>> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut push = |slice: &'a [Token]| {
        if slice.iter().any(|t| !matches!(t.kind, TokenKind::Newline)) {
            let (s, e) = (
                slice.first().expect("non-empty").span.start,
                slice.last().expect("non-empty").span.end,
            );
            out.push(Line {
                toks: slice,
                description: render(slice, src),
                start: s,
                end: e,
            });
        }
    };
    for (idx, t) in toks.iter().enumerate() {
        if t.kind == TokenKind::Newline {
            if idx > start {
                push(&toks[start..idx]);
            }
            start = idx + 1;
        }
    }
    if start < toks.len() {
        push(&toks[start..]);
    }
    out
}

/// Rebuild a line's printed text from the tokens that survived reminder
/// stripping.
///
/// Reconstructed from SOURCE SLICES between kept tokens rather than by joining
/// token texts, so printed spacing and punctuation are preserved exactly and
/// only the removed reminder spans collapse.
pub fn render(toks: &[Token], src: &str) -> String {
    let mut out = String::new();
    let mut prev_end: Option<usize> = None;
    for t in toks {
        if matches!(t.kind, TokenKind::Reminder { .. } | TokenKind::Newline) {
            continue;
        }
        if let Some(pe) = prev_end {
            // Any gap between two kept tokens was whitespace or a dropped
            // reminder; both render as a single space.
            if t.span.start > pe && !out.is_empty() {
                out.push(' ');
            }
        }
        out.push_str(t.text(src));
        prev_end = Some(t.span.end);
    }
    out.trim().to_string()
}

/// Is this sentence the "can't be regenerated" rider? CR 701.15b.
///
/// Both printed subjects mean the same thing: "it" after a single-object
/// destruction, "they" after a mass one.
pub fn is_cant_regenerate(toks: &[Token], src: &str) -> bool {
    const FORMS: &[(&str, ())] = &[
        ("it can't be regenerated", ()),
        ("they can't be regenerated", ()),
        ("that creature can't be regenerated", ()),
    ];
    let stream = Tokens::new(toks, src);
    match phrase_alt(FORMS)(stream) {
        Ok((rest, _)) => is_exhausted(rest),
        Err(_) => false,
    }
}

/// Lift a leading "If you do," off a sentence.
///
/// CR 608.2d: the clause performs the sentence it introduces only when the
/// OPTIONAL effect before it actually happened. It is a gate, not an
/// instruction, so removing it here keeps the effect grammar from needing an
/// arm for a word that does nothing on its own.
pub fn strip_if_you_do<'a>(
    toks: &'a [Token],
    src: &'a str,
) -> (&'a [Token], Option<AbilityCondition>) {
    let stream = Tokens::new(toks, src);
    let Ok((rest, _)) = phrase_alt(&[("if you do", ()), ("if you don't", ())])(stream) else {
        return (toks, None);
    };
    // Only the positive form is built: "if you don't" gates on the opposite
    // outcome and the engine records a different signal for it.
    if phrase_alt(&[("if you don't", ())])(stream).is_ok() {
        return (toks, None);
    }
    let after = match rest.first() {
        Some(t) if t.kind == TokenKind::Comma => rest.take_from_n(1),
        _ => return (toks, None),
    };
    if after.is_empty() {
        return (toks, None);
    }
    (
        after.toks,
        Some(AbilityCondition::EffectOutcome {
            signal: phase_oracle_ast::EffectSignal::OptionalEffectPerformed,
        }),
    )
}

/// Lift a leading "You may" off a sentence.
///
/// CR 608.2d: a permission, not an instruction. The flag it sets is what a
/// later "if you do" reads, which is why both are lifted in the same place.
pub fn strip_you_may<'a>(toks: &'a [Token], src: &'a str) -> (&'a [Token], bool) {
    let stream = Tokens::new(toks, src);
    match phrase_alt(&[("you may", ())])(stream) {
        Ok((rest, _)) if !rest.is_empty() => (rest.toks, true),
        _ => (toks, false),
    }
}

/// A trailing duration phrase. CR 611.2.
pub fn duration(i: In<'_>) -> Option<(In<'_>, Duration)> {
    const TABLE: &[(&str, Duration)] = &[
        ("until end of turn", Duration::UntilEndOfTurn),
        ("until end of combat", Duration::UntilEndOfCombat),
    ];
    phrase_alt(TABLE)(i).ok()
}

/// Everything after the clause that is structural punctuation rather than
/// content. A trailing period is slack; anything else is a decline.
pub fn is_exhausted(i: In<'_>) -> bool {
    i.toks.iter().all(|t| {
        matches!(
            t.kind,
            TokenKind::Period | TokenKind::Newline | TokenKind::Semicolon
        )
    })
}

/// Split a token slice into sentences at periods and semicolons.
///
/// The boundary KIND is kept, because it decides `sub_link`: a sentence break
/// is a `SequentialSibling`, while a comma or "then" inside a sentence is a
/// `ContinuationStep` (CR 608.2c).
pub fn sentences<'a>(toks: &'a [Token]) -> Vec<&'a [Token]> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for (idx, t) in toks.iter().enumerate() {
        if matches!(t.kind, TokenKind::Period | TokenKind::Semicolon) {
            if idx > start {
                out.push(&toks[start..idx]);
            }
            start = idx + 1;
        }
    }
    if start < toks.len() && toks[start..].iter().any(|t| t.kind != TokenKind::Newline) {
        out.push(&toks[start..]);
    }
    out
}

/// What one sentence lowered to.
pub struct SentenceParse {
    pub effects: Vec<Effect>,
    pub duration: Option<Duration>,
    pub facts: ClauseFacts,
    /// Present when the sentence is a continuous effect with no printed end,
    /// which in spell position is the permanent's own static ability rather
    /// than something a resolving spell does.
    pub standalone: Option<StaticAbility>,
}

/// Which effect in a sentence the sentence's duration belongs to.
///
/// A duration is printed once and governs the clause it trails, which is the
/// FIRST effect the sentence produced — "target creature gets +1/+1 and gains
/// flying until end of turn" is one continuous effect, not two.
pub const DURATION_OWNER: usize = 0;

/// Parse one sentence into a chain of effects, splitting at "then".
///
/// "then" is the printed marker for a continuation step, so a sentence that
/// carries one produces two linked definitions rather than declining.
pub fn sentence_effects(toks: &[Token], src: &str, in_trigger: bool) -> Option<SentenceParse> {
    let stream = Tokens::new(toks, src);
    let stream = if in_trigger {
        stream.in_trigger()
    } else {
        stream
    };
    let (rest, effects, facts, inner_dur, standalone) = parse_effect_chain(stream)?;
    // A duration already consumed by a subject clause governs the whole
    // sentence; a trailing one applies to an imperative that had none.
    let (rest, dur) = match (inner_dur, duration(rest)) {
        (Some(d), _) => (rest, Some(d)),
        (None, Some((r, d))) => (r, Some(d)),
        (None, None) => (rest, None),
    };
    if !is_exhausted(rest) || effects.is_empty() {
        return None;
    }
    Some(SentenceParse {
        effects,
        duration: dur,
        facts,
        standalone,
    })
}

/// `<clause> [(, then | and | ,) <clause>]*`
type ChainOut<'a> = (
    In<'a>,
    Vec<Effect>,
    ClauseFacts,
    Option<Duration>,
    Option<StaticAbility>,
);

fn parse_effect_chain(i: In<'_>) -> Option<ChainOut<'_>> {
    let (mut rest, mut chain, mut facts, mut dur, standalone) = one_clause(i)?;
    loop {
        // A continuation is marked by "then" or by "and", with or without a
        // leading comma. "and" reaches here only when the clause before it has
        // already refused to reuse its own subject — "target creature gets
        // +1/+1 and gains flying" is consumed inside one clause, while "target
        // player loses 4 life AND YOU GAIN 4 life" names a new subject and so
        // is a second clause.
        let after_comma = match rest.first() {
            Some(t) if t.kind == TokenKind::Comma => rest.take_from_n(1),
            _ => rest,
        };
        let Ok((after_join, _)) = crate::prim::any_of(&["then", "and"])(after_comma) else {
            break;
        };
        match one_clause(after_join) {
            Some((r, mut more, f, d, _)) => {
                chain.append(&mut more);
                facts = facts.merge(f);
                if dur.is_none() {
                    dur = d;
                }
                rest = r;
            }
            None => break,
        }
    }
    Some((rest, chain, facts, dur, standalone))
}

/// One clause, imperative or subject-initial.
type ClauseOut<'a> = (
    In<'a>,
    Vec<Effect>,
    ClauseFacts,
    Option<Duration>,
    Option<StaticAbility>,
);

fn one_clause(i: In<'_>) -> Option<ClauseOut<'_>> {
    if let Ok((r, (e, f))) = crate::effect::imperative(i) {
        return Some((r, vec![e], f, None, None));
    }
    if let Ok((r, c)) = crate::effect::subject_clause(i) {
        return Some((r, c.effects, c.facts, c.duration, c.standalone));
    }
    None
}

/// One link in an ability's chain: an effect, how it attaches to the previous
/// one, and the duration printed with it.
///
/// Duration is PER PART, not per ability: "Target creature gets -3/-0 until end
/// of turn.\nTarget creature gets -0/-3 until end of turn." prints one on each
/// sentence, and the engine records both.
pub type Part = (
    Effect,
    SubAbilityLink,
    Option<Duration>,
    Option<PlayerScope>,
    Option<AbilityCondition>,
);

/// Assemble a chain of parts into one definition.
///
/// Chaining appends at the tail so printed order and resolution order agree
/// (CR 608.2c).
pub fn assemble(
    kind: AbilityKind,
    cost: Option<AbilityCost>,
    parts: Vec<Part>,
    description: String,
) -> Option<AbilityDefinition> {
    let mut it = parts.into_iter();
    let (first, _, first_dur, first_scope, first_cond) = it.next()?;
    let mut root = AbilityDefinition::new(kind, first);
    root.refresh_mana_ability();
    root.cost = cost;
    root.duration = first_dur;
    root.player_scope = first_scope;
    root.condition = first_cond;
    root.description = Some(description);

    for (e, link, dur, scope, cond) in it {
        let mut next = AbilityDefinition::spell(e);
        next.refresh_mana_ability();
        next.sub_link = link;
        next.duration = dur;
        next.player_scope = scope;
        next.condition = cond;
        root.chain(next);
    }
    Some(root)
}
