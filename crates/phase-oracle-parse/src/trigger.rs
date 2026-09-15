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
    const ANY_TURN: &[(&str, ())] = &[("each player's", ()), ("each", ()), ("the", ())];
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

/// What a spell-cast trigger watches.
///
/// Unlike a targeting clause, this position does NOT wrap the filter in
/// `StackSpell`: the trigger already knows it is watching a cast, so the filter
/// only has to say which spells count. A bare "a spell" is therefore an empty
/// type constraint rather than a missing one.
fn cast_filter(i: In<'_>) -> R<'_, TargetFilter> {
    let (r, _) = crate::prim::any_of(&["a", "an"])(i)?;
    if let Ok((r2, _)) = crate::prim::any_of(&["spell", "spells"])(r) {
        return Ok((
            r2,
            TargetFilter::Typed(phase_oracle_ast::TypedFilter::default()),
        ));
    }
    let (r, types) = crate::target::typed_filter_list(r)?;
    let (r, _) = crate::prim::any_of(&["spell", "spells"])(r)?;
    Ok((r, types))
}

/// The object a `when`/`whenever` head watches.
///
/// A trigger's watched object is a CLASS, not a target — "whenever another
/// creature dies" watches every one of them — so scope is discarded here
/// rather than being carried into a `*All` effect variant.
fn watched(i: In<'_>) -> R<'_, TargetFilter> {
    let (mut rest, first) = watched_one(i)?;
    let mut parts = vec![first];

    // "Whenever ~ OR ANOTHER CREATURE dies" watches either, which the engine
    // spells as a disjunction rather than as two triggers.
    while let Ok((r, _)) = word("or")(rest) {
        match watched_one(r) {
            Ok((r2, next)) => {
                parts.push(next);
                rest = r2;
            }
            Err(_) => break,
        }
    }

    if parts.len() == 1 {
        return Ok((rest, parts.pop().expect("one element")));
    }
    Ok((rest, TargetFilter::Or { filters: parts }))
}

/// One alternative in a watched-object list.
fn watched_one(i: In<'_>) -> R<'_, TargetFilter> {
    if let Ok((r, _)) = self_ref(i) {
        return Ok((r, TargetFilter::SelfRef));
    }
    let (r, s) = subject(i)?;

    // "When ENCHANTED CREATURE dies" watches the one object this Aura is on,
    // which the engine names directly rather than through the filter it uses in
    // a static ability's `affected` slot. Same printed words, two positions,
    // two spellings — so the translation happens here, once.
    if let Some(attached) = as_attached_host(&s.filter) {
        return Ok((r, attached));
    }
    Ok((r, s.filter))
}

/// Recognize the "this source's own host" filter shape.
fn as_attached_host(f: &TargetFilter) -> Option<TargetFilter> {
    let TargetFilter::Typed(t) = f else {
        return None;
    };
    t.properties
        .iter()
        .any(|p| {
            matches!(
                p,
                phase_oracle_ast::FilterProp::EnchantedBy
                    | phase_oracle_ast::FilterProp::EquippedBy
            )
        })
        .then_some(TargetFilter::AttachedTo)
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
    // "Whenever YOU GAIN LIFE" watches an event about the controller, not an
    // object, so it has no watched-object slot.
    if let Ok((r2, _)) = phrase("you gain life")(r) {
        return Ok((r2, TriggerHead::mode(TriggerMode::LifeGained)));
    }

    if let Ok((r2, _)) = phrase("you cast")(r) {
        let (r3, f) = cast_filter(r2)?;
        let mut h = TriggerHead::mode(TriggerMode::SpellCast);
        h.valid_card = Some(f);
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

    if let Ok((r2, _)) = phrase("becomes the target of")(r) {
        // The spell or ability doing the targeting is not recorded here; the
        // trigger watches the OBJECT that became a target.
        let (r3, _) = subject(r2)?;
        let mut h = TriggerHead::mode(TriggerMode::BecomesTarget);
        h.valid_card = Some(who);
        return Ok((r3, h));
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
