//! Triggered-ability productions. CR 603.
//!
//! A trigger line is `<trigger event>, <effect>`: the comma after the event is
//! the structural boundary, and everything after it is an ordinary effect
//! chain. That split is why triggers cost one module rather than a duplicate of
//! the whole effect grammar.

use phase_oracle_ast::{PhaseName, TargetFilter, TriggerMode, ZoneName};
use serde_json::json;

use crate::prim::{any_of, fail, phrase, phrase_alt, self_ref, word, In, R};
use crate::target::subject;

/// Everything the trigger head establishes, before the effect.
#[derive(Debug, Clone, PartialEq)]
pub struct TriggerHead {
    pub mode: TriggerMode,
    pub valid_card: Option<TargetFilter>,
    pub origin: Option<ZoneName>,
    pub destination: Option<ZoneName>,
    pub phase: Option<PhaseName>,
    pub valid_source: Option<TargetFilter>,
    pub valid_target: Option<TargetFilter>,
    /// CR 603.2: "only during your turn" riders carried by a phase trigger's
    /// possessive ("your upkeep" versus "each player's upkeep").
    pub constraint: Option<serde_json::Value>,
}

impl TriggerHead {
    fn mode(mode: TriggerMode) -> Self {
        Self {
            mode,
            valid_card: None,
            origin: None,
            destination: None,
            phase: None,
            valid_source: None,
            valid_target: None,
            constraint: None,
        }
    }
}

/// The step named by an "at the beginning of …" head.
fn phase_name(i: In<'_>) -> R<'_, PhaseName> {
    const TABLE: &[(&str, PhaseName)] = &[
        ("untap step", PhaseName::Untap),
        ("upkeep step", PhaseName::Upkeep),
        ("upkeep", PhaseName::Upkeep),
        ("draw step", PhaseName::Draw),
        ("precombat main phase", PhaseName::PreCombatMain),
        ("postcombat main phase", PhaseName::PostCombatMain),
        ("end step", PhaseName::End),
        ("end of turn", PhaseName::End),
        ("cleanup step", PhaseName::Cleanup),
        ("combat", PhaseName::BeginCombat),
    ];
    phrase_alt(TABLE)(i)
}

/// `at the beginning of <possessive> <step>`.
///
/// The possessive is not decoration: "your upkeep" restricts the trigger to the
/// controller's turn and the engine records that as a `constraint`, while "each
/// player's upkeep" fires every turn and carries none.
fn phase_head(i: In<'_>) -> R<'_, TriggerHead> {
    let (r, _) = phrase("at the beginning of")(i)?;

    // Possessive axis, as ONE alternation rather than one arm per step. The
    // possessive is not decoration: it is what makes the trigger fire on one
    // player's turn rather than on everyone's.
    let (r, mut constraint) = match whose(r) {
        Some(v) => v,
        None => (r, None),
    };

    let (r, phase) = phase_name(r)?;

    // "at the beginning of combat ON YOUR TURN" — the possessive can also be
    // printed AFTER the step, and it means the same thing. Read here so both
    // orders reach one representation instead of two.
    let r = match trailing_whose(r) {
        Some((r2, c)) => {
            if constraint.is_none() {
                constraint = c;
            }
            r2
        }
        None => r,
    };

    let mut h = TriggerHead::mode(TriggerMode::Phase);
    h.phase = Some(phase);
    h.constraint = constraint;
    Ok((r, h))
}

/// A possessive printed BEFORE the step: "your upkeep", "each player's upkeep".
fn whose(i: In<'_>) -> Option<(In<'_>, Option<serde_json::Value>)> {
    if let Ok((r, _)) = word("your")(i) {
        return Some((r, Some(json!({"type": "OnlyDuringYourTurn"}))));
    }
    if let Ok((r, _)) = phrase("each opponent's")(i) {
        return Some((r, Some(json!({"type": "OnlyDuringOpponentsTurn"}))));
    }
    const ANY_TURN: &[(&str, ())] = &[("each player's", ()), ("the", ())];
    if let Ok((r, _)) = phrase_alt(ANY_TURN)(i) {
        return Some((r, None));
    }
    None
}

/// A possessive printed AFTER the step: "combat on your turn".
fn trailing_whose(i: In<'_>) -> Option<(In<'_>, Option<serde_json::Value>)> {
    let (r, _) = phrase("on")(i).ok()?;
    if let Ok((r2, _)) = phrase("your turn")(r) {
        return Some((r2, Some(json!({"type": "OnlyDuringYourTurn"}))));
    }
    if let Ok((r2, _)) = phrase("each opponent's turn")(r) {
        return Some((r2, Some(json!({"type": "OnlyDuringOpponentsTurn"}))));
    }
    if let Ok((r2, _)) = phrase("each turn")(r) {
        return Some((r2, None));
    }
    None
}

/// The object a `when`/`whenever` head watches.
///
/// A trigger's watched object is a CLASS, not a target — "whenever another
/// creature dies" watches every one of them — so scope is discarded here
/// rather than being carried into a `*All` effect variant.
fn watched(i: In<'_>) -> R<'_, TargetFilter> {
    if let Ok((r, _)) = self_ref(i) {
        return Ok((r, TargetFilter::SelfRef));
    }
    let (r, s) = subject(i)?;
    Ok((r, s.filter))
}

/// `when[ever] <object> <event>`.
fn event_head(i: In<'_>) -> R<'_, TriggerHead> {
    let (r, _) = any_of(&["when", "whenever"])(i)?;

    // "whenever you attack" watches the CONTROLLER's attack as a whole, not any
    // one creature, so it has its own mode and no watched object. Read before
    // the general path, where "you" would otherwise parse as an ordinary
    // subject and "attack" as that subject's event.
    if let Ok((r2, _)) = phrase("you attack")(r) {
        return Ok((r2, TriggerHead::mode(TriggerMode::YouAttack)));
    }

    // "you cast" / "a player casts" is a spell-cast trigger, whose watched
    // object is the SPELL rather than the subject that cast it.
    if let Ok((r2, _)) = phrase("you cast")(r) {
        let (r3, s) = subject(r2)?;
        let mut h = TriggerHead::mode(TriggerMode::SpellCast);
        h.valid_card = Some(s.filter);
        return Ok((r3, h));
    }

    let (r, who) = watched(r)?;

    // CR 603.6: an enters/dies trigger is a zone change, distinguished by its
    // origin and destination rather than by a separate mode.
    if let Ok((r2, _)) = any_of(&["enters", "enter"])(r) {
        let mut h = TriggerHead::mode(TriggerMode::ChangesZone);
        h.valid_card = Some(who);
        h.destination = Some(ZoneName::Battlefield);
        return Ok((r2, h));
    }
    if let Ok((r2, _)) = any_of(&["dies", "die"])(r) {
        let mut h = TriggerHead::mode(TriggerMode::ChangesZone);
        h.valid_card = Some(who);
        h.origin = Some(ZoneName::Battlefield);
        h.destination = Some(ZoneName::Graveyard);
        return Ok((r2, h));
    }

    const SIMPLE: &[(&str, TriggerMode)] = &[
        ("attacks", TriggerMode::Attacks),
        ("attack", TriggerMode::Attacks),
        ("blocks", TriggerMode::Blocks),
        ("block", TriggerMode::Blocks),
        ("taps", TriggerMode::Taps),
    ];
    if let Ok((r2, mode)) = phrase_alt(SIMPLE)(r) {
        let mut h = TriggerHead::mode(mode);
        h.valid_card = Some(who);
        return Ok((r2, h));
    }

    // "<source> deals damage to <victim>" — the victim is the trigger's target
    // slot, not part of the watched object.
    if let Ok((r2, _)) = any_of(&["deals", "deal"])(r) {
        let (r3, _) = word("damage")(r2)?;
        let (r3, _) = word("to")(r3)?;
        let (r4, victim) = subject(r3)?;
        let mut h = TriggerHead::mode(TriggerMode::DamageDone);
        h.valid_source = Some(who);
        h.valid_target = Some(victim.filter);
        return Ok((r4, h));
    }

    if let Ok((r2, _)) = phrase("becomes blocked")(r) {
        let mut h = TriggerHead::mode(TriggerMode::BecomesBlocked);
        h.valid_card = Some(who);
        return Ok((r2, h));
    }

    if let Ok((r2, _)) = phrase("leaves the battlefield")(r) {
        let mut h = TriggerHead::mode(TriggerMode::LeavesBattlefield);
        h.valid_card = Some(who);
        return Ok((r2, h));
    }

    fail(i)
}

/// The full trigger-head grammar.
pub fn trigger_head(i: In<'_>) -> R<'_, TriggerHead> {
    if let Ok(v) = phase_head(i) {
        return Ok(v);
    }
    event_head(i)
}

/// Does this line open with a trigger word at all?
///
/// Used only to route a line to the right production; the head grammar still
/// has to succeed for the line to parse.
pub fn looks_like_trigger(i: In<'_>) -> bool {
    matches!(i.first_word().as_deref(), Some("when" | "whenever" | "at"))
}
