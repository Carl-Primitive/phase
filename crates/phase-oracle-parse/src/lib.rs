//! A lexer-first Oracle parser that emits the engine's card-definition format.
//!
//! The pipeline is: normalize self-references, lex to tokens, split into
//! printed lines, classify each line, and lower it to an engine-shaped
//! [`phase_oracle_ast`] value.
//!
//! Two invariants hold at every stage and are checked over the whole corpus:
//!
//! 1. **The lexer claims every byte.** `phase_oracle_lex::verify_coverage`.
//! 2. **A line either parses completely or declines with a span.** There is no
//!    third outcome, and in particular no outcome in which a production
//!    succeeds while leaving printed words unaccounted for. That is the
//!    property a post-hoc text auditor exists to recover in a parser that
//!    cannot state it directly.

pub mod cost;
pub mod effect;
pub mod keywords;
pub mod line;
pub mod normalize;
pub mod prim;
pub mod stream;
pub mod subtypes;
pub mod target;
pub mod trigger;

use phase_oracle_ast::{
    AbilityCost, AbilityDefinition, AbilityKind, AbilityTag as ActivationTag,
    ActivationRestriction, CardOutput, Effect, Keyword, SubAbilityLink, TriggerDefinition,
};
use phase_oracle_lex::{lex, Token, TokenKind};

use crate::line::{Decline, DeclineReason, Line};
use crate::stream::Tokens;

/// One card's Oracle text, lowered.
pub struct CardParse {
    pub out: CardOutput,
    /// Every line the grammar refused, with the production that refused it.
    pub declines: Vec<Decline>,
}

impl CardParse {
    pub fn is_complete(&self) -> bool {
        self.declines.is_empty()
    }
}

/// Parse one card.
pub fn parse_card(name: &str, oracle: &str) -> CardParse {
    let src = normalize::normalize(name, oracle);

    // Reminder text restates rules rather than creating them, and it is the one
    // span the grammar may discard — the lexer has already proved each one is a
    // complete parenthesised unit, so dropping it cannot lose a partial clause.
    // It must go BEFORE the grammar rather than only out of the rendered
    // description: a reminder sitting after a sentence's period would otherwise
    // read as a second, unparseable sentence and fail the whole line.
    let toks: Vec<Token> = lex(&src)
        .into_iter()
        .filter(|t| !matches!(t.kind, TokenKind::Reminder { .. }))
        .collect();

    let mut out = CardOutput::default();
    let mut declines = Vec::new();

    // Consecutive spell lines fold into ONE ability with a `SequentialSibling`
    // chain, because the engine treats a spell's whole printed body as a single
    // definition whose description carries the newlines (CR 608.2c, written
    // order). Keyword, activated and triggered lines each stand alone.
    let mut spell_run: Vec<(Vec<line::Part>, String)> = Vec::new();

    // CR 700.2: a modal header governs the bullet lines that FOLLOW it, so the
    // header's counts are held until the modes have been collected.
    let mut modal: Option<(usize, usize)> = None;
    let mut modes: Vec<String> = Vec::new();

    for l in line::lines(&toks, &src) {
        if let Some(counts) = modal_header(&l, &src) {
            modal = Some(counts);
            continue;
        }
        if let Some(body) = strip_bullet(&l, &src) {
            if modal.is_none() {
                declines.push(decline(&l, "modal_bullet", DeclineReason::UnknownLineShape));
                continue;
            }
            match spell_line(&body, &src) {
                // A mode is an ability of its own, with no description: the
                // printed text lives in `mode_descriptions` instead.
                Ok(Lowered::SpellBody(chain, _)) => {
                    match line::assemble(AbilityKind::Spell, None, chain, String::new()) {
                        Some(mut a) => {
                            a.description = None;
                            modes.push(body.description.clone());
                            out.abilities.push(a);
                        }
                        None => declines.push(decline(
                            &l,
                            "modal_bullet",
                            DeclineReason::UnknownLineShape,
                        )),
                    }
                }
                Ok(_) => {
                    declines.push(decline(&l, "modal_bullet", DeclineReason::UnknownLineShape))
                }
                Err(d) => declines.push(d),
            }
            continue;
        }

        match parse_line(&l, &src) {
            Ok(Lowered::Keywords(mut kws)) => out.keywords.append(&mut kws),
            Ok(Lowered::SpellBody(chain, desc)) => spell_run.push((chain, desc)),
            Ok(Lowered::Statics(mut sa)) => out.static_abilities.append(&mut sa),
            Ok(Lowered::Ability(a)) => out.abilities.push(*a),
            Ok(Lowered::Trigger(t)) => out.triggers.push(*t),
            Err(d) => declines.push(d),
        }
    }

    if let Some(a) = fold_spell_run(spell_run) {
        out.abilities.push(a);
    }

    if let Some((min_choices, max_raw)) = modal {
        // "Choose one or more" caps at the number of modes printed, which is
        // only known once they have all been read.
        let mode_count = modes.len();
        if mode_count == 0 {
            out.modal = None;
        } else {
            out.modal = Some(phase_oracle_ast::ModalChoice {
                min_choices,
                max_choices: max_raw.min(mode_count).max(min_choices),
                mode_count,
                mode_descriptions: modes,
                allow_repeat_modes: false,
                chooser: phase_oracle_ast::TargetFilter::Controller,
            });
        }
    }

    CardParse { out, declines }
}

/// Fold every spell line of a card into one definition.
///
/// The FIRST line's parts keep their own links; each later line opens with a
/// `SequentialSibling`, and the description is the lines joined by the newline
/// that separated them, which is exactly what the engine prints.
fn fold_spell_run(run: Vec<(Vec<line::Part>, String)>) -> Option<AbilityDefinition> {
    if run.is_empty() {
        return None;
    }
    let mut parts: Vec<line::Part> = Vec::new();
    let mut descriptions: Vec<String> = Vec::new();

    for (n, (chain, desc)) in run.into_iter().enumerate() {
        descriptions.push(desc);
        for (k, (e, link, dur, scope)) in chain.into_iter().enumerate() {
            let link = if n > 0 && k == 0 {
                SubAbilityLink::SequentialSibling
            } else {
                link
            };
            parts.push((e, link, dur, scope));
        }
    }

    line::assemble(AbilityKind::Spell, None, parts, descriptions.join("\n"))
}

enum Lowered {
    Keywords(Vec<Keyword>),
    /// A spell line, left unassembled so consecutive ones can fold together.
    SpellBody(Vec<line::Part>, String),
    /// A line that is the permanent's own continuous ability. CR 611.2: an
    /// effect with no printed end lasts as long as its source, so it is not
    /// something a spell does — it is something the permanent IS.
    Statics(Vec<phase_oracle_ast::StaticAbility>),
    Ability(Box<AbilityDefinition>),
    Trigger(Box<TriggerDefinition>),
}

fn decline(l: &Line<'_>, production: &'static str, reason: DeclineReason) -> Decline {
    Decline {
        production,
        reason,
        text: l.description.clone(),
        start: l.start,
        end: l.end,
    }
}

/// Classify one printed line and lower it.
///
/// Order matters and is structural, not heuristic: a keyword line has no verb,
/// an activated ability is the only shape with a top-level colon, and a trigger
/// is the only shape that opens with a trigger word.
fn parse_line(l: &Line<'_>, src: &str) -> Result<Lowered, Decline> {
    let stream = Tokens::new(l.toks, src);

    if let Some(kws) = keyword_line(stream) {
        return Ok(Lowered::Keywords(kws));
    }

    // CR 207.2c: an ability word is flavour. It has no rules meaning, the
    // engine does not keep it even in the description, and stripping it here
    // means every production below reads the sentence it introduces rather than
    // needing its own leading-label arm.
    if let Some((inner, tag)) = strip_ability_word(l, src) {
        let lowered = parse_line(&inner, src)?;
        // A few of these labels are not pure flavour: they name a CLASS of
        // ability that other cards refer to ("activate a boast ability"), so
        // the engine keeps a tag even though it drops the word.
        return Ok(match (lowered, tag) {
            (Lowered::Ability(mut a), Some(t)) => {
                // The timing rider these keywords carry is printed only in
                // their reminder text, which the grammar drops — so it is
                // derived from the keyword, the way Equip's sorcery speed is.
                // Boast is deliberately absent: it also carries a condition
                // ("only if this creature attacked"), and emitting half of its
                // restrictions would be worse than emitting none.
                if matches!(t, ActivationTag::PowerUp | ActivationTag::Exhaust)
                    && a.activation_restrictions.is_empty()
                {
                    a.activation_restrictions = vec![ActivationRestriction::OnlyOnce];
                }
                a.ability_tag = Some(t);
                Lowered::Ability(a)
            }
            (other, _) => other,
        });
    }

    // CR 702.5 / CR 702.6: a keyword line that carries an ARGUMENT.
    if let Some(kw) = keywords::enchant_line(stream) {
        return Ok(Lowered::Keywords(vec![kw]));
    }
    if let Some(a) = keywords::equip_line(stream, &l.description) {
        return Ok(Lowered::Ability(Box::new(a)));
    }

    if let Some(colon) = top_level_colon(l.toks) {
        return activated_line(l, src, colon).map(|a| Lowered::Ability(Box::new(a)));
    }

    if trigger::looks_like_trigger(stream) {
        return triggered_line(l, src).map(|t| Lowered::Trigger(Box::new(t)));
    }

    spell_line(l, src)
}

/// A static ability's description, as the engine prints it.
///
/// The leading "Other" is dropped. The exclusion it states survives in the
/// filter's `Another` property, so the word is redundant in the prose — and
/// this is what the engine does, verified across the corpus.
fn static_description(line: &str) -> String {
    for prefix in ["Other ", "other "] {
        let Some(rest) = line.strip_prefix(prefix) else {
            continue;
        };
        // Measured, and not a rule anyone would guess: the engine keeps the
        // word immediately before a bare "creatures" (76 of 100 keeps) and
        // drops it before anything else — a subtype, a colour, "permanents"
        // (257 drops).
        //
        // The obvious generalization — keep it whenever the noun phrase's head
        // is "creature(s)" and no colour qualifies it, so that "Other legendary
        // creatures" keeps it too — was tried TWICE, at different coverage
        // levels, and scored worse both times (5,452 against 5,474, then 5,650
        // against 5,672). It is not a near miss; do not re-attempt it without
        // new evidence about what actually drives the engine's choice.
        //
        // The exclusion itself survives in the filter's `Another` property
        // either way; only the prose differs.
        if rest.starts_with("creatures") || rest.starts_with("creature ") {
            return line.to_string();
        }
        return rest.to_string();
    }
    line.to_string()
}

/// Drop a leading ability word, yielding the line it introduces.
///
/// An ability word is one or more capitalized words before an em dash, with a
/// real sentence after it. Two shapes are deliberately NOT stripped: a chapter
/// head ("I —", "II, III —"), whose numeral is structural, and a modal header
/// ("Choose one —"), which has nothing after the dash on its own line.
/// `Choose one —` and its relatives. CR 700.2.
///
/// Returns the minimum and the maximum number of modes. The maximum for
/// "one or more" is the mode COUNT, which the caller resolves once the bullets
/// have been read; `usize::MAX` stands for it here.
fn modal_header(l: &Line<'_>, src: &str) -> Option<(usize, usize)> {
    let stream = Tokens::new(l.toks, src);
    let (rest, _) = prim::word("choose")(stream).ok()?;

    const COUNTS: &[(&str, (usize, usize))] = &[
        ("one or both", (1, 2)),
        ("one or more", (1, usize::MAX)),
        ("one", (1, 1)),
        ("two", (2, 2)),
        ("three", (3, 3)),
    ];
    let (rest, counts) = prim::phrase_alt(COUNTS)(rest).ok()?;

    // The em dash is what makes this a modal header rather than an instruction
    // that happens to start with "choose".
    let dash = rest.first()?;
    (dash.kind == TokenKind::EmDash && line::is_exhausted(rest.take_from_n(1))).then_some(counts)
}

/// One printed mode, with its `•` removed.
fn strip_bullet<'a>(l: &Line<'a>, src: &str) -> Option<Line<'a>> {
    let first = l.toks.first()?;
    if first.kind != TokenKind::Bullet {
        return None;
    }
    let rest = &l.toks[1..];
    if rest.is_empty() {
        return None;
    }
    Some(Line {
        toks: rest,
        description: line::render(rest, src),
        start: rest.first().expect("non-empty").span.start,
        end: rest.last().expect("non-empty").span.end,
    })
}

/// Labels that are printed like an ability word but name a referable class.
const TAGGED_ABILITY_WORDS: &[(&str, ActivationTag)] = &[
    ("boast", ActivationTag::Boast),
    ("exhaust", ActivationTag::Exhaust),
    ("power-up", ActivationTag::PowerUp),
];

fn strip_ability_word<'a>(l: &Line<'a>, src: &str) -> Option<(Line<'a>, Option<ActivationTag>)> {
    let dash = l.toks.iter().position(|t| t.kind == TokenKind::EmDash)?;

    // Magic's templating separates an ability WORD from its sentence with a
    // SPACED em dash, and joins a keyword to its argument with an unspaced one
    // ("Cumulative upkeep—Put a -1/-1 counter on this creature"). The corpus is
    // bimodal on this: 3,518 spaced against 439 unspaced, with no middle. That
    // typographic distinction is the only reliable way to tell flavour from a
    // keyword whose argument follows, so it is read rather than guessed at from
    // a list of ability words that every set adds to.
    let d = l.toks[dash].span;
    let spaced = src[..d.start].ends_with(' ') && src[d.end..].starts_with(' ');
    if !spaced {
        return None;
    }

    let (label, rest) = (&l.toks[..dash], &l.toks[dash + 1..]);

    if label.is_empty() || rest.is_empty() {
        return None;
    }
    // Every label token must be a word or a comma; a numeral or a symbol means
    // this dash is doing structural work, not introducing flavour.
    if !label
        .iter()
        .all(|t| matches!(t.kind, TokenKind::Word | TokenKind::Comma))
    {
        return None;
    }
    if label.iter().any(|t| is_roman_numeral(t.text(src))) {
        return None;
    }

    let tag = (label.len() == 1)
        .then(|| label[0].text(src).to_lowercase())
        .and_then(|w| {
            TAGGED_ABILITY_WORDS
                .iter()
                .find(|(name, _)| *name == w)
                .map(|(_, t)| *t)
        });

    Some((
        Line {
            toks: rest,
            description: line::render(rest, src),
            start: rest.first().expect("non-empty").span.start,
            end: rest.last().expect("non-empty").span.end,
        },
        tag,
    ))
}

/// CR 714.2: a Saga chapter head, which must not be mistaken for flavour.
fn is_roman_numeral(w: &str) -> bool {
    !w.is_empty() && w.chars().all(|c| matches!(c, 'I' | 'V' | 'X'))
}

/// The index of the colon that separates cost from effect, if the line has one.
///
/// "Top level" means outside a quoted span; a quoted granted ability is a
/// single token by construction, so no scan is needed to skip one.
fn top_level_colon(toks: &[Token]) -> Option<usize> {
    toks.iter().position(|t| t.kind == TokenKind::Colon)
}

/// A bare keyword line: "Flying", "Flying, vigilance", "First strike, trample".
///
/// One production covering the whole evergreen set, rather than one arm per
/// keyword. Anything outside the printed vocabulary declines here and is tried
/// as an ability instead.
fn keyword_line(i: Tokens<'_>) -> Option<Vec<Keyword>> {
    let mut rest = i;
    let mut out = Vec::new();
    loop {
        let (r, kw) = bare_keyword(rest)?;
        out.push(kw);
        rest = r;
        match rest.first() {
            Some(t) if t.kind == TokenKind::Comma => rest = rest.take_from_n(1),
            _ => break,
        }
    }
    line::is_exhausted(rest).then_some(out)
}

/// One keyword in bare-line position, simple or parameterized.
fn bare_keyword(i: Tokens<'_>) -> Option<(Tokens<'_>, Keyword)> {
    // CR 702.14: landwalk is one keyword carrying a land type, so it is read
    // before the simple vocabulary rather than being five lookalike entries.
    if let Some(w) = i.first_word() {
        if let Some(land) = effect::landwalk(&w) {
            return Some((
                i.take_from_n(1),
                Keyword::Landwalk {
                    land_type: land.to_string(),
                },
            ));
        }
    }
    let (r, (pascal, _printed)) = effect::keyword_word(i).ok()?;
    Some((r, Keyword::Simple(pascal)))
}

/// `<cost> : <effect>` — an activated ability. CR 602.
fn activated_line(l: &Line<'_>, src: &str, colon: usize) -> Result<AbilityDefinition, Decline> {
    let cost_toks = &l.toks[..colon];
    let body = &l.toks[colon + 1..];

    let Some(cost) = cost::ability_cost(Tokens::new(cost_toks, src)) else {
        return Err(decline(l, "ability_cost", DeclineReason::UnparsedCost));
    };

    // CR 602.5d and friends: a trailing "Activate only ..." sentence states
    // WHEN the ability may be activated, not what it does, so it is lifted off
    // the body before the effect grammar sees it.
    let (body, restrictions) = split_activation_restrictions(body, src);

    let parts = effect_chain(body, src)
        .ok_or_else(|| decline(l, "activated_effect", DeclineReason::UnknownVerb))?;

    let mut a = line::assemble(
        AbilityKind::Activated,
        Some(cost),
        parts.effects,
        l.description.clone(),
    )
    .ok_or_else(|| decline(l, "assemble", DeclineReason::UnknownLineShape))?;
    // CR 606.3: a loyalty ability may be activated only when its controller
    // could cast a sorcery. The restriction is inherent to the cost, not
    // printed on the card, so it is derived rather than parsed.
    let mut restrictions = restrictions;
    if matches!(a.cost, Some(AbilityCost::Loyalty { .. })) && restrictions.is_empty() {
        restrictions.push(ActivationRestriction::AsSorcery);
    }
    a.activation_restrictions = restrictions;
    Ok(a)
}

/// Split a trailing "Activate only ..." sentence off an ability's body.
///
/// An UNRECOGNIZED "Activate only ..." is deliberately left in the body, so the
/// line declines rather than quietly losing a timing restriction. That is the
/// totality rule applied to a field rather than to a clause.
fn split_activation_restrictions<'a>(
    body: &'a [Token],
    src: &str,
) -> (&'a [Token], Vec<ActivationRestriction>) {
    const TABLE: &[(&str, ActivationRestriction)] = &[
        (
            "activate only as a sorcery",
            ActivationRestriction::AsSorcery,
        ),
        (
            "activate this ability only as a sorcery",
            ActivationRestriction::AsSorcery,
        ),
        (
            "activate only once each turn",
            ActivationRestriction::OnlyOnceEachTurn,
        ),
        (
            "activate only during your turn",
            ActivationRestriction::DuringYourTurn,
        ),
        (
            "activate only during your upkeep",
            ActivationRestriction::DuringYourUpkeep,
        ),
    ];

    let sentences = line::sentences(body);
    let Some(last) = sentences.last() else {
        return (body, Vec::new());
    };
    let Ok((rest, restriction)) = prim::phrase_alt(TABLE)(Tokens::new(last, src)) else {
        return (body, Vec::new());
    };
    if !line::is_exhausted(rest) {
        return (body, Vec::new());
    }

    let cut = last.first().map(|t| t.span.start).unwrap_or(0);
    let keep = body
        .iter()
        .position(|t| t.span.start >= cut)
        .unwrap_or(body.len());
    (&body[..keep], vec![restriction])
}

/// `<trigger event>, <effect>` — a triggered ability. CR 603.
fn triggered_line(l: &Line<'_>, src: &str) -> Result<TriggerDefinition, Decline> {
    let stream = Tokens::new(l.toks, src);
    let Ok((rest, head)) = trigger::trigger_head(stream) else {
        return Err(decline(l, "trigger_head", DeclineReason::UnknownLineShape));
    };

    // The comma after the event is the structural boundary between head and
    // effect. Without it the line is some other shape the grammar has not built.
    let Some(t) = rest.first() else {
        return Err(decline(l, "trigger_body", DeclineReason::TrailingTokens));
    };
    if t.kind != TokenKind::Comma {
        return Err(decline(l, "trigger_body", DeclineReason::TrailingTokens));
    }
    let body = rest.take_from_n(1);

    // CR 603.2c: "you may" makes the whole trigger optional, and is not part of
    // the effect it governs.
    let (body, optional) = match prim::phrase("you may")(body) {
        Ok((r, _)) => (r, true),
        Err(_) => (body, false),
    };

    let parts = effect_chain_in(body.toks, src, true)
        .ok_or_else(|| decline(l, "trigger_effect", DeclineReason::UnknownVerb))?;

    let mut execute = line::assemble(AbilityKind::Spell, None, parts.effects, String::new())
        .ok_or_else(|| decline(l, "assemble", DeclineReason::UnknownLineShape))?;

    // A trigger's `execute` carries no description; the line's prose lives on
    // the trigger itself.
    execute.description = None;
    // CR 603.2c: "you may" is recorded in BOTH places — on the trigger, which
    // decides whether it is put on the stack at all, and on the ability it
    // executes, which decides whether the controller performs the action.
    execute.optional = optional;

    let mut td = TriggerDefinition::new(head.mode, execute);
    td.valid_card = head.valid_card;
    td.origin = head.origin;
    td.destination = head.destination;
    td.phase = head.phase;
    td.valid_source = head.valid_source;
    // CR 603.3d: a trigger's targets are chosen as it goes on the stack, so a
    // "target player" inside the effect fills the TRIGGER's target slot. The
    // head's own slot wins when it named one ("whenever ~ deals damage to …").
    td.valid_target = head.valid_target.or_else(|| {
        parts
            .facts
            .targeted_player
            .then_some(phase_oracle_ast::TargetFilter::Player)
    });
    td.constraint = head.constraint;
    td.optional = optional;
    td.trigger_zones = trigger_zones(&td);
    td.description = Some(l.description.clone());
    Ok(td)
}

/// Where the source must be for this trigger to function. CR 603.6.
///
/// A trigger watching its OWN death fires from the graveyard, because the
/// source is already there when the ability triggers; one watching another
/// creature's death fires from the battlefield, where the source still is.
/// Reading this off the trigger's own shape is what keeps it one rule instead
/// of a per-card annotation.
fn trigger_zones(td: &TriggerDefinition) -> Vec<phase_oracle_ast::ZoneName> {
    use phase_oracle_ast::{TargetFilter, TriggerMode, ZoneName};
    match td.mode {
        TriggerMode::LeavesBattlefield => {
            vec![ZoneName::Battlefield, ZoneName::Graveyard, ZoneName::Exile]
        }
        TriggerMode::ChangesZone
            if td.origin == Some(ZoneName::Battlefield)
                && td.destination == Some(ZoneName::Graveyard)
                && matches!(td.valid_card, Some(TargetFilter::SelfRef)) =>
        {
            vec![ZoneName::Graveyard]
        }
        _ => vec![ZoneName::Battlefield],
    }
}

/// A line with no cost and no trigger word: a spell's own instructions.
///
/// Returned unassembled, because the card's spell lines are one ability
/// together and only the caller knows whether more follow.
fn spell_line(l: &Line<'_>, src: &str) -> Result<Lowered, Decline> {
    let parts = effect_chain(l.toks, src)
        .ok_or_else(|| decline(l, "spell_effect", DeclineReason::UnknownVerb))?;

    // A line whose every sentence is a continuous effect with no printed end is
    // the permanent's own static ability, not an instruction a spell carries
    // out. The engine files those in their own bucket, and the whole printed
    // line is their description.
    if !parts.statics.is_empty() && parts.statics.len() == parts.sentence_count {
        let statics = parts
            .statics
            .into_iter()
            .map(|mut sa| {
                sa.description = Some(static_description(&l.description));
                sa
            })
            .collect();
        return Ok(Lowered::Statics(statics));
    }

    Ok(Lowered::SpellBody(parts.effects, l.description.clone()))
}

struct Chain {
    effects: Vec<line::Part>,
    facts: effect::ClauseFacts,
    /// One per sentence that lowered to a standalone continuous ability.
    statics: Vec<phase_oracle_ast::StaticAbility>,
    sentence_count: usize,
}

/// Lower a run of tokens into a linked chain of effects.
///
/// A sentence boundary links as `SequentialSibling`; a "then" or comma inside a
/// sentence links as `ContinuationStep` (CR 608.2c). Every sentence must parse:
/// one refusal fails the line, because a partially-understood ability is worse
/// than an honestly declined one.
fn effect_chain(toks: &[Token], src: &str) -> Option<Chain> {
    effect_chain_in(toks, src, false)
}

/// Lower a run of tokens, saying which structure they came from.
fn effect_chain_in(toks: &[Token], src: &str, in_trigger: bool) -> Option<Chain> {
    let mut effects: Vec<line::Part> = Vec::new();
    let mut facts = effect::ClauseFacts::default();
    let mut statics = Vec::new();
    let mut sentence_count = 0usize;

    for (n, sent) in line::sentences(toks).into_iter().enumerate() {
        // CR 701.15b: "It can't be regenerated" is printed as its own sentence
        // but is a RIDER on the destruction before it, not an instruction of
        // its own. The engine records it as a field, so it is folded back here.
        if line::is_cant_regenerate(sent, src) {
            match effects.last_mut().map(|p| &mut p.0) {
                Some(Effect::Destroy {
                    cant_regenerate, ..
                })
                | Some(Effect::DestroyAll {
                    cant_regenerate, ..
                }) => {
                    *cant_regenerate = true;
                    continue;
                }
                // A rider with nothing to attach to is not something the
                // grammar understands; declining keeps that visible.
                _ => return None,
            }
        }

        let p = line::sentence_effects(sent, src, in_trigger)?;
        sentence_count += 1;
        facts = facts.merge(p.facts);
        if let Some(sa) = p.standalone {
            statics.push(sa);
        }
        let dur = p.duration;
        for (k, e) in p.effects.into_iter().enumerate() {
            // The first effect of a later SENTENCE is a sibling; everything
            // else continues the step it was printed inside.
            let link = if n > 0 && k == 0 {
                SubAbilityLink::SequentialSibling
            } else {
                SubAbilityLink::ContinuationStep
            };
            let owned = (k == line::DURATION_OWNER).then_some(dur.clone()).flatten();
            let scope = (k == line::DURATION_OWNER)
                .then_some(p.facts.player_scope)
                .flatten();
            effects.push((e, link, owned, scope));
        }
    }

    (!effects.is_empty()).then_some(Chain {
        effects,
        facts,
        statics,
        sentence_count,
    })
}

/// Re-exported so a caller can talk about costs without depending on the AST
/// crate directly.
pub use phase_oracle_ast::AbilityCost as Cost;
const _: fn() -> Option<AbilityCost> = || None;
