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
pub mod line;
pub mod normalize;
pub mod prim;
pub mod stream;
pub mod target;
pub mod trigger;

use phase_oracle_ast::{
    AbilityCost, AbilityDefinition, AbilityKind, CardOutput, SubAbilityLink, TriggerDefinition,
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
    let toks = lex(&src);

    let mut out = CardOutput::default();
    let mut declines = Vec::new();

    // Consecutive spell lines fold into ONE ability with a `SequentialSibling`
    // chain, because the engine treats a spell's whole printed body as a single
    // definition whose description carries the newlines (CR 608.2c, written
    // order). Keyword, activated and triggered lines each stand alone.
    let mut spell_run: Vec<(Vec<line::Part>, String)> = Vec::new();

    for l in line::lines(&toks, &src) {
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
    Keywords(Vec<String>),
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

    if let Some(colon) = top_level_colon(l.toks) {
        return activated_line(l, src, colon).map(|a| Lowered::Ability(Box::new(a)));
    }

    if trigger::looks_like_trigger(stream) {
        return triggered_line(l, src).map(|t| Lowered::Trigger(Box::new(t)));
    }

    spell_line(l, src)
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
fn keyword_line(i: Tokens<'_>) -> Option<Vec<String>> {
    let mut rest = i;
    let mut out = Vec::new();
    loop {
        let (r, (pascal, _printed)) = effect::keyword_word(rest).ok()?;
        out.push(pascal);
        rest = r;
        match rest.first() {
            Some(t) if t.kind == TokenKind::Comma => rest = rest.take_from_n(1),
            _ => break,
        }
    }
    line::is_exhausted(rest).then_some(out)
}

/// `<cost> : <effect>` — an activated ability. CR 602.
fn activated_line(l: &Line<'_>, src: &str, colon: usize) -> Result<AbilityDefinition, Decline> {
    let cost_toks = &l.toks[..colon];
    let body = &l.toks[colon + 1..];

    let Some(cost) = cost::ability_cost(Tokens::new(cost_toks, src)) else {
        return Err(decline(l, "ability_cost", DeclineReason::UnparsedCost));
    };

    let parts = effect_chain(body, src)
        .ok_or_else(|| decline(l, "activated_effect", DeclineReason::UnknownVerb))?;

    line::assemble(
        AbilityKind::Activated,
        Some(cost),
        parts.effects,
        l.description.clone(),
    )
    .ok_or_else(|| decline(l, "assemble", DeclineReason::UnknownLineShape))
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

    let parts = effect_chain(body.toks, src)
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
                sa.description = Some(l.description.clone());
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
    let mut effects: Vec<line::Part> = Vec::new();
    let mut facts = effect::ClauseFacts::default();
    let mut statics = Vec::new();
    let mut sentence_count = 0usize;

    for (n, sent) in line::sentences(toks).into_iter().enumerate() {
        let p = line::sentence_effects(sent, src)?;
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
