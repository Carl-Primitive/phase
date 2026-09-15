//! Activation-cost productions: everything left of the `:`.
//!
//! CR 118.3: a cost is a list of components separated by commas. This module is
//! the single authority that turns that list into an [`AbilityCost`]; no caller
//! ever inspects an individual component.

use phase_oracle_ast::{
    AbilityCost, CounterMatch, CounterSelection, ManaCost, Quantity, SacrificeCost, TapRequirement,
    TapRequirementKind, TargetFilter,
};
use phase_oracle_lex::{PtPart, Sign, TokenKind};

use crate::prim::{any_of, fail, mana_symbol, number, phrase, word, In, ManaSym, R};
use crate::target::subject;

/// A maximal run of mana symbols, folded into one `Mana` cost.
///
/// `{2}{R}{R}` is ONE cost component, not three: generic amounts accumulate and
/// coloured symbols become shards, which is the engine's `ManaCost::Cost`.
fn mana_cost(i: In<'_>) -> R<'_, AbilityCost> {
    let (mut rest, first) = mana_symbol(i)?;
    let mut shards = Vec::new();
    let mut generic = 0u32;
    let mut push = |s: ManaSym| match s {
        ManaSym::Shard(sh) => shards.push(sh),
        ManaSym::Generic(n) => generic += n,
    };
    push(first);
    while let Ok((r, s)) = mana_symbol(rest) {
        push(s);
        rest = r;
    }
    // `{T}` and `{Q}` are not mana; `mana_symbol` already declines them, so a
    // run that produced nothing payable cannot occur here.
    Ok((
        rest,
        AbilityCost::Mana {
            cost: ManaCost::Cost { shards, generic },
        },
    ))
}

/// Strip the `Untapped` property a tap cost states redundantly.
fn drop_untapped(f: &mut TargetFilter) {
    match f {
        TargetFilter::Typed(t) => t
            .properties
            .retain(|p| !matches!(p, phase_oracle_ast::FilterProp::Untapped)),
        TargetFilter::Or { filters } | TargetFilter::And { filters } => {
            for inner in filters {
                drop_untapped(inner);
            }
        }
        _ => {}
    }
}

/// Which counter kind a removal cost names.
fn counter_match(i: In<'_>) -> R<'_, CounterMatch> {
    // "Remove A counter from ~" with no kind is the untyped form, resolved to
    // one concrete kind at payment time.
    if matches!(i.first_word().as_deref(), Some("counter" | "counters")) {
        return Ok((i, CounterMatch::Any));
    }
    let (r, kind) = crate::prim::counter_type(i)?;
    Ok((
        r,
        CounterMatch::OfType {
            data: kind.key().to_string(),
        },
    ))
}

/// The tap and untap symbols. CR 107.5 / CR 107.6.
fn tap_symbol(i: In<'_>) -> R<'_, AbilityCost> {
    let (r, body) = crate::prim::symbol_body(i)?;
    match body.to_ascii_uppercase().as_str() {
        "T" => Ok((r, AbilityCost::Tap)),
        "Q" => Ok((r, AbilityCost::Untap)),
        _ => fail(i),
    }
}

/// CR 606.3: a bracketed planeswalker loyalty cost.
fn loyalty_cost(i: In<'_>) -> R<'_, AbilityCost> {
    match i.first() {
        Some(t) => match t.kind {
            TokenKind::Loyalty {
                cost: PtPart::Number { sign, value },
            } => {
                let v = value as i32;
                let amount = if sign == Sign::Minus { -v } else { v };
                Ok((i.take_from_n(1), AbilityCost::Loyalty { amount }))
            }
            _ => fail(i),
        },
        None => fail(i),
    }
}

/// One cost component.
fn component(i: In<'_>) -> R<'_, AbilityCost> {
    if let Ok(v) = tap_symbol(i) {
        return Ok(v);
    }
    if let Ok(v) = loyalty_cost(i) {
        return Ok(v);
    }
    if let Ok(v) = mana_cost(i) {
        return Ok(v);
    }
    if let Ok((r, _)) = word("sacrifice")(i) {
        // "Sacrifice three other creatures" — the count is printed before the
        // noun, so it is read here rather than assumed to be one.
        let (r, count) = match crate::prim::number(r) {
            Ok((r2, n)) if n > 0 => (r2, n as u32),
            _ => (r, 1),
        };
        let (r, s) = subject(r)?;
        return Ok((
            r,
            AbilityCost::Sacrifice(SacrificeCost {
                target: s.filter,
                count,
            }),
        ));
    }

    // "Remove a +1/+1 counter from ~" / "Remove two charge counters from ~"
    if let Ok((r, _)) = word("remove")(i) {
        let (r, count) = match crate::prim::number(r) {
            Ok((r2, n)) if n > 0 => (r2, n as u32),
            _ => (r, 1),
        };
        let (r, kind) = counter_match(r)?;
        let (r, _) = any_of(&["counter", "counters"])(r)?;
        let (r, _) = word("from")(r)?;
        let (r, s) = subject(r)?;
        // A removal from the source itself leaves the slot empty: the cost is
        // already anchored to the ability's own permanent.
        let target = match s.filter {
            TargetFilter::SelfRef => None,
            other => Some(other),
        };
        return Ok((
            r,
            AbilityCost::RemoveCounter {
                count,
                counter_type: kind,
                target,
                selection: CounterSelection::SingleObject,
            },
        ));
    }

    // "Tap two untapped artifacts you control" — CR 601.2b. Not the source's
    // own `{T}`, which `tap_symbol` already handled.
    if let Ok((r, _)) = word("tap")(i) {
        let (r, count) = match crate::prim::number(r) {
            Ok((r2, n)) if n > 0 => (r2, n as u32),
            _ => (r, 1),
        };
        let (r, mut s) = subject(r)?;
        // "Tap an UNTAPPED creature you control": being untapped is what the
        // cost requires, not what it selects for, so the engine leaves it out
        // of the filter. Keeping it would double the constraint.
        drop_untapped(&mut s.filter);
        return Ok((
            r,
            AbilityCost::TapCreatures {
                requirement: TapRequirement {
                    requirement: TapRequirementKind::Count,
                    count,
                },
                filter: s.filter,
            },
        ));
    }
    if let Ok((r, _)) = word("discard")(i) {
        let (r, q) = match crate::prim::quantity(r) {
            Ok(v) => v,
            Err(_) => (r, Quantity::fixed(1)),
        };
        let (r, _) = any_of(&["card", "cards"])(r)?;
        // "Discard a card AT RANDOM" — the manner of choosing is part of the
        // cost, not a separate clause.
        let (r, random) = match phrase("at random")(r) {
            Ok((r2, _)) => (r2, true),
            Err(_) => (r, false),
        };
        return Ok((
            r,
            AbilityCost::Discard {
                count: q,
                filter: None,
                selection_random: random,
                self_scope: false,
            },
        ));
    }
    if let Ok((r, _)) = word("pay")(i) {
        let (r, n) = number(r)?;
        let (r, _) = word("life")(r)?;
        return Ok((
            r,
            AbilityCost::PayLife {
                amount: Quantity::fixed(n),
            },
        ));
    }
    if let Ok((r, _)) = phrase("untap")(i) {
        let (r, _) = crate::prim::self_ref(r)?;
        return Ok((r, AbilityCost::Untap));
    }
    fail(i)
}

/// The whole cost list, left of the colon.
///
/// Must consume every token it is handed: a cost the grammar only half
/// understands is worse than a decline, because the ability would then be
/// activatable for less than it prints.
pub fn ability_cost(i: In<'_>) -> Option<AbilityCost> {
    let mut rest = i;
    let mut parts = Vec::new();

    loop {
        let (r, c) = component(rest).ok()?;
        parts.push(c);
        rest = r;
        match rest.first() {
            Some(t) if t.kind == TokenKind::Comma => rest = rest.take_from_n(1),
            None => break,
            _ => return None,
        }
    }

    match parts.len() {
        0 => None,
        1 => parts.pop(),
        _ => Some(AbilityCost::Composite { costs: parts }),
    }
}
