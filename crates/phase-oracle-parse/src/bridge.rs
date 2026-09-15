//! Emit schema clauses in the existing engine's JSON shape.
//!
//! This exists to answer one question with a number rather than an opinion:
//! how far is the schema from being a drop-in for the current parser's output?
//! It is deliberately NOT part of the schema contract. The schema is
//! engine-independent on purpose; this module is the adapter that proves the
//! independence is bridgeable, and it is where every shape difference between
//! the two representations is forced into the open.

use phase_card_schema::{Clause, Effect, Quantity, Target};
use serde_json::{json, Value};

/// The engine names keywords in PascalCase with no spaces ("FirstStrike"),
/// while the schema keeps the printed spelling ("first strike"). This is the
/// whole of the keyword-shape difference between the two representations.
fn engine_keyword_name(printed: &str) -> String {
    printed
        .split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => format!("{}{}", f.to_uppercase(), c.as_str()),
                None => String::new(),
            }
        })
        .collect()
}

fn quantity_json(q: &Quantity) -> Value {
    match q {
        Quantity::Fixed { value } => json!({"type": "Fixed", "value": value}),
        // The engine separates a constant from a REFERENCE to a dynamic value
        // (QuantityExpr::Fixed vs QuantityExpr::Ref). That layering is correct
        // and the schema currently flattens it; the bridge restores it here.
        Quantity::Variable => json!({"type": "Ref", "qty": {"type": "Variable", "name": "X"}}),
        Quantity::ThatMany => json!({"type": "EventContextAmount"}),
        Quantity::AnyNumber => json!({"type": "AnyNumber"}),
        Quantity::All => json!({"type": "All"}),
    }
}

/// Engine `TargetFilter` shape.
fn target_json(t: &Target) -> Value {
    match t {
        Target::AnyTarget => json!({"type": "Any"}),
        Target::You => json!({"type": "Controller"}),
        Target::This => json!({"type": "SelfRef"}),
        Target::Attached => json!({"type": "AttachedTo"}),
        Target::EachOpponent => json!({"type": "Opponent"}),
        Target::Player { .. } => json!({"type": "Player"}),
        Target::Chosen { filter } | Target::Each { filter } => {
            let types: Vec<Value> = filter
                .types
                .iter()
                .map(|t| {
                    let mut c = t.chars();
                    let up = c.next().map(|f| f.to_uppercase().to_string()).unwrap_or_default();
                    Value::String(format!("{up}{}", c.as_str()))
                })
                .collect();
            let controller = match filter.controller {
                Some(phase_card_schema::Controller::You) => json!("Controller"),
                Some(phase_card_schema::Controller::Opponent) => json!("Opponent"),
                _ => Value::Null,
            };
            json!({
                "type": "Typed",
                "type_filters": types,
                "controller": controller,
                "properties": []
            })
        }
    }
}

/// Engine `Effect` shape, or `None` where the schema has no engine counterpart
/// at this vocabulary size.
fn effect_json(e: &Effect) -> Option<Value> {
    Some(match e {
        // 23 engine effect families name their SCOPE in the variant
        // (Destroy/DestroyAll, Bounce/BounceAll, Damage/DamageEachPlayer).
        // The schema carries scope on the target instead, so the bridge has to
        // pick the variant from the target's shape.
        Effect::Destroy { target } => {
            let ty = if matches!(target, Target::Each { .. }) { "DestroyAll" } else { "Destroy" };
            json!({"type": ty, "target": target_json(target), "cant_regenerate": false})
        }
        Effect::Exile { target } => json!({"type": "Exile", "target": target_json(target)}),
        Effect::DealDamage { amount, target } => json!({
            "type": "DealDamage", "amount": quantity_json(amount), "target": target_json(target)
        }),
        Effect::Draw { who, amount } => json!({
            "type": "Draw", "count": quantity_json(amount), "target": target_json(who)
        }),
        // The engine OMITS the player field when the subject is the
        // controller, making absence mean "you". The schema is explicit; the
        // bridge reproduces the implicit default.
        Effect::GainLife { who, amount } => match who {
            Target::You => json!({"type": "GainLife", "amount": quantity_json(amount)}),
            _ => json!({"type": "GainLife", "amount": quantity_json(amount), "player": target_json(who)}),
        },
        Effect::LoseLife { who, amount } => match who {
            Target::You => json!({"type": "LoseLife", "amount": quantity_json(amount)}),
            _ => json!({"type": "LoseLife", "amount": quantity_json(amount), "player": target_json(who)}),
        },
        Effect::ModifyPt { target, change } => json!({
            "type": "Pump",
            "power": {"type": "Fixed", "value": change.power},
            "toughness": {"type": "Fixed", "value": change.toughness},
            "target": target_json(target)
        }),
        Effect::CounterSpell { target } => json!({"type": "Counter", "target": target_json(target)}),
        Effect::ReturnToHand { target } => {
            if matches!(target, Target::Each { .. }) {
                json!({"type": "BounceAll", "target": target_json(target)})
            } else {
                json!({"type": "ChangeZone", "target": target_json(target), "destination": "Hand"})
            }
        }
        // No faithful engine counterpart yet at this vocabulary size.
        Effect::SetTapped { .. }
        | Effect::PutCounter { .. }
        | Effect::Sacrifice { .. }
        | Effect::Discard { .. }
        | Effect::Mill { .. }
        | Effect::CreateToken { .. }
        | Effect::GainKeyword { .. }
        | Effect::Sequence { .. }
        | Effect::Unparsed { .. } => return None,
    })
}

/// Result of bridging one card.
pub struct Bridged {
    pub keywords: Vec<String>,
    pub abilities: Vec<Value>,
    /// Clauses the bridge could not express in engine shape.
    pub unbridged: usize,
}

/// Map parsed clauses into the engine's `{keywords, abilities}` split.
///
/// The split itself is a real difference between the representations: the
/// engine hoists a bare keyword line into a `keywords` array and leaves
/// `abilities` empty, while the schema models it as an effect on the source.
pub fn bridge(clauses: &[Clause]) -> Bridged {
    let mut keywords = Vec::new();
    let mut abilities = Vec::new();
    let mut unbridged = 0usize;

    for c in clauses {
        // A keyword granted to the source with no duration is a printed keyword.
        if let Effect::GainKeyword { target: Target::This, keyword } = &c.effect {
            if c.duration.is_none() {
                keywords.push(engine_keyword_name(keyword));
                continue;
            }
        }
        if let Effect::Sequence { effects, .. } = &c.effect {
            let all_kw = effects.iter().all(|e| {
                matches!(e, Effect::GainKeyword { target: Target::This, .. })
            });
            if all_kw && c.duration.is_none() {
                for e in effects {
                    if let Effect::GainKeyword { keyword, .. } = e {
                        keywords.push(engine_keyword_name(keyword));
                    }
                }
                continue;
            }
        }

        match effect_json(&c.effect) {
            Some(effect) => {
                let duration = match c.duration {
                    Some(phase_card_schema::Duration::EndOfTurn) => json!("UntilEndOfTurn"),
                    Some(phase_card_schema::Duration::EndOfCombat) => json!("UntilEndOfCombat"),
                    _ => Value::Null,
                };
                abilities.push(json!({
                    "kind": "Spell",
                    "effect": effect,
                    "cost": Value::Null,
                    "sub_ability": Value::Null,
                    "duration": duration,
                    "target_prompt": Value::Null,
                    "condition": Value::Null,
                    "optional_targeting": false,
                    "optional": false,
                    "forward_result": false
                }));
            }
            None => unbridged += 1,
        }
    }

    Bridged { keywords, abilities, unbridged }
}
