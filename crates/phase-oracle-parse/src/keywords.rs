//! Keyword lines that carry an argument.
//!
//! A bare keyword hoists into the card's `keywords` array as a word. A
//! PARAMETERIZED one carries something the grammar has to read — the filter an
//! Aura may attach to, the cost an Equipment charges — and the engine spells
//! each of those differently. They live here rather than beside the bare
//! vocabulary because each is a small grammar, not a list entry.

use phase_oracle_ast::{
    AbilityCost, AbilityDefinition, AbilityKind, AbilityTag, ActivationRestriction, ControllerRef,
    Effect, Keyword, TargetFilter, TypedFilter,
};

use crate::cost::ability_cost;
use crate::prim::{word, In};
use crate::stream::Tokens;
use crate::target::typed_filter_list;

/// `Enchant <filter>` — CR 702.5a.
///
/// States what this Aura may legally be attached to, so it is a keyword with a
/// filter rather than a keyword with a name.
pub fn enchant_line(i: In<'_>) -> Option<Keyword> {
    let (r, _) = word("enchant")(i).ok()?;
    let (r, filter) = enchant_target(r)?;
    crate::line::is_exhausted(r).then_some(Keyword::Enchant { filter })
}

fn enchant_target(i: In<'_>) -> Option<(In<'_>, TargetFilter)> {
    // "Enchant player" is the one non-object case, and CR 102.1 makes a player
    // not an object, so it cannot come from the type grammar.
    if let Ok((r, _)) = word("player")(i) {
        return Some((r, TargetFilter::Player));
    }
    if let Ok((r, _)) = word("opponent")(i) {
        return Some((
            r,
            TargetFilter::Typed(TypedFilter::player(ControllerRef::Opponent)),
        ));
    }
    typed_filter_list(i).ok()
}

/// `Equip <cost>` — CR 702.6b.
///
/// Lowers to the activated ability the keyword stands for: attach this
/// Equipment to a creature you control, at sorcery speed. None of that is
/// printed on the card, so all of it is derived from the keyword rather than
/// parsed out of a sentence that does not exist.
pub fn equip_line(i: In<'_>, description: &str) -> Option<AbilityDefinition> {
    let (r, _) = word("equip")(i).ok()?;
    let cost = ability_cost(r)?;

    let mut a = AbilityDefinition::new(
        AbilityKind::Activated,
        Effect::Attach {
            target: TargetFilter::Typed(TypedFilter {
                type_filters: vec![phase_oracle_ast::TypeFilter::Creature],
                controller: Some(ControllerRef::You),
                properties: Vec::new(),
            }),
        },
    );
    a.cost = Some(cost);
    // CR 702.6b: "Equip only as a sorcery."
    a.activation_restrictions = vec![ActivationRestriction::AsSorcery];
    a.ability_tag = Some(AbilityTag::Equip);
    a.description = Some(description.to_string());
    Some(a)
}

/// Re-exported for the line dispatcher, which holds a token slice rather than a
/// stream.
pub type Stream<'a> = Tokens<'a>;

/// Keep the cost module's single-authority rule visible from here: a caller
/// never inspects an individual cost component.
const _: fn(In<'_>) -> Option<AbilityCost> = ability_cost;
