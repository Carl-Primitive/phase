//! Activation-cost productions: everything left of the `:`.
//!
//! CR 118.3: a cost is a list of components separated by commas. This module is
//! the single authority that turns that list into an [`AbilityCost`]; no caller
//! ever inspects an individual component.

use phase_oracle_ast::{AbilityCost, ManaCost, Quantity, SacrificeCost};
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
        let (r, s) = subject(r)?;
        return Ok((
            r,
            AbilityCost::Sacrifice(SacrificeCost {
                target: s.filter,
                count: 1,
            }),
        ));
    }
    if let Ok((r, _)) = word("discard")(i) {
        let (r, q) = match crate::prim::quantity(r) {
            Ok(v) => v,
            Err(_) => (r, Quantity::fixed(1)),
        };
        let (r, _) = any_of(&["card", "cards"])(r)?;
        return Ok((
            r,
            AbilityCost::Discard {
                count: q,
                filter: None,
                selection_random: false,
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
