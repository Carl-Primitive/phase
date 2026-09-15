//! Grammar behaviour, pinned at the production level.

use phase_card_schema::effect::DeclineReason;
use phase_card_schema::{Controller, Duration, Effect, ObjectFilter, PtChange, Quantity, Target};
use phase_oracle_parse::parse_card;

fn one(name: &str, text: &str) -> Effect {
    let c = parse_card(name, text);
    assert_eq!(c.len(), 1, "expected one clause from {text:?}, got {c:#?}");
    c.into_iter().next().unwrap().effect
}

fn creature() -> ObjectFilter {
    ObjectFilter { types: vec!["creature".into()], ..Default::default() }
}

#[test]
fn destroy_target_creature() {
    assert_eq!(
        one("Doom Blade", "Destroy target creature."),
        Effect::Destroy { target: Target::Chosen { filter: creature() } }
    );
}

#[test]
fn controller_restriction_is_carried_not_dropped() {
    let want = ObjectFilter { controller: Some(Controller::Opponent), ..creature() };
    assert_eq!(
        one("Ravenous Chupacabra", "Destroy target creature an opponent controls."),
        Effect::Destroy { target: Target::Chosen { filter: want } }
    );
}

#[test]
fn damage_to_any_target() {
    assert_eq!(
        one("Lightning Bolt", "Lightning Bolt deals 3 damage to any target."),
        Effect::DealDamage { amount: Quantity::Fixed { value: 3 }, target: Target::AnyTarget }
    );
}

#[test]
fn counter_target_spell() {
    assert_eq!(
        one("Counterspell", "Counter target spell."),
        Effect::CounterSpell {
            target: Target::Chosen {
                filter: ObjectFilter { types: vec!["spell".into()], ..Default::default() }
            }
        }
    );
}

#[test]
fn pump_carries_its_duration() {
    let c = parse_card("Giant Growth", "Target creature gets +3/+3 until end of turn.");
    assert_eq!(c.len(), 1);
    assert_eq!(
        c[0].effect,
        Effect::ModifyPt {
            target: Target::Chosen { filter: creature() },
            change: PtChange { power: 3, toughness: 3, variable: false }
        }
    );
    assert_eq!(c[0].duration, Some(Duration::EndOfTurn));
}

#[test]
fn word_and_digit_counts_agree() {
    let a = one("A", "Draw two cards.");
    let b = one("B", "Draw 2 cards.");
    assert_eq!(a, b);
    assert_eq!(a, Effect::Draw { who: Target::You, amount: Quantity::Fixed { value: 2 } });
}

#[test]
fn article_is_a_count_of_one() {
    assert_eq!(
        one("Divination", "Draw a card."),
        Effect::Draw { who: Target::You, amount: Quantity::Fixed { value: 1 } }
    );
}

/// `gains 3 life` and `gains flying` share a verb; the count disambiguates.
#[test]
fn gains_life_and_gains_keyword_do_not_collide() {
    assert_eq!(
        one("Healing Salve", "You gain 3 life."),
        Effect::GainLife { who: Target::You, amount: Quantity::Fixed { value: 3 } }
    );
    assert_eq!(
        one("Jump", "Target creature gains flying until end of turn."),
        Effect::GainKeyword { target: Target::Chosen { filter: creature() }, keyword: "flying".into() }
    );
}

#[test]
fn each_opponent_loses_life() {
    assert_eq!(
        one("Blood Artist", "Each opponent loses 2 life."),
        Effect::LoseLife { who: Target::EachOpponent, amount: Quantity::Fixed { value: 2 } }
    );
}

#[test]
fn put_counter_on_target() {
    use phase_card_schema::CounterKind;
    assert_eq!(
        one("Giant Growth", "Put a +1/+1 counter on target creature."),
        Effect::PutCounter {
            target: Target::Chosen { filter: creature() },
            counter: CounterKind::PlusOnePlusOne,
            amount: Quantity::Fixed { value: 1 }
        }
    );
}

#[test]
fn self_reference_normalizes_to_this() {
    assert_eq!(
        one("Prodigal Sorcerer", "Prodigal Sorcerer deals 1 damage to any target."),
        Effect::DealDamage { amount: Quantity::Fixed { value: 1 }, target: Target::AnyTarget }
    );
}

/// Reminder text restates rules; it must not become a clause.
#[test]
fn reminder_text_is_not_a_clause() {
    let c = parse_card("Whatever", "Destroy target creature. (This is reminder text.)");
    assert_eq!(c.len(), 1, "reminder text must not produce a clause: {c:#?}");
}

// --- The totality rule -------------------------------------------------

/// THE claim. A clause whose tail the grammar cannot account for is reported
/// as unparsed, never as a partial success that silently drops printed words.
#[test]
fn trailing_words_force_a_decline_rather_than_a_silent_drop() {
    let e = one("Whatever", "Destroy target creature with flying.");
    assert_eq!(
        e,
        Effect::Unparsed {
            text: "Destroy target creature with flying.".into(),
            reason: DeclineReason::TrailingTokens
        },
        "an unhandled restrictive clause must decline, not drop"
    );
}

/// This is the 738-card class the existing parser's auditor is blind to:
/// a restriction on the target that the grammar does not model. Here it is
/// structurally impossible to lose it silently.
#[test]
fn unmodelled_target_restriction_cannot_be_silently_widened() {
    let e = one("Whatever", "Destroy target creature with mana value 3 or less.");
    assert!(
        matches!(e, Effect::Unparsed { reason: DeclineReason::TrailingTokens, .. }),
        "widening the target by dropping its restriction must be impossible, got {e:?}"
    );
}

#[test]
fn unknown_verb_is_named_as_such() {
    let e = one("Whatever", "Bolster 3.");
    assert!(
        matches!(e, Effect::Unparsed { reason: DeclineReason::UnknownVerb, .. }),
        "got {e:?}"
    );
}

#[test]
fn multi_sentence_text_yields_one_clause_each() {
    let c = parse_card("Whatever", "Destroy target creature.\nDraw a card.");
    assert_eq!(c.len(), 2);
    assert!(matches!(c[0].effect, Effect::Destroy { .. }));
    assert!(matches!(c[1].effect, Effect::Draw { .. }));
}

/// Spans must point back at the words that produced the clause.
#[test]
fn clause_spans_locate_their_source_text() {
    let text = "Destroy target creature.\nDraw a card.";
    let c = parse_card("Whatever", text);
    assert_eq!(&text[c[0].source.start..c[0].source.end], "Destroy target creature.");
    assert_eq!(&text[c[1].source.start..c[1].source.end], "Draw a card.");
}
