//! Keyword lines that carry an argument.
//!
//! A bare keyword hoists into the card's `keywords` array as a word. A
//! PARAMETERIZED one carries something the grammar has to read — the filter an
//! Aura may attach to, the cost an Equipment charges — and the engine spells
//! each of those differently. They live here rather than beside the bare
//! vocabulary because each is a small grammar, not a list entry.

use phase_oracle_ast::{
    AbilityCost, AbilityDefinition, AbilityKind, AbilityTag, ActivationRestriction, ControllerRef,
    Effect, Keyword, KeywordCost, ManaCost, TargetFilter, TypedFilter, WrappedKeywordCost,
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

/// Keywords printed with a cost, and which payload family each one uses.
///
/// `false` means a BARE `ManaCost`; `true` means the tagged envelope.
///
/// Flashback, Evoke and Bestow were added here and REMOVED again: they never
/// appear as a card's only line, so the purity test had no evidence either way,
/// and each turned out to generate a trigger the hoist would have dropped.
/// Absence of counter-evidence is not evidence. Which
/// family a keyword belongs to is a fact about the engine's representation, not
/// something the printed text reveals — "Morph {2}{U}" and "Flashback {1}{B}"
/// read identically and serialize differently — so it is looked up rather than
/// inferred.
///
/// Only keywords whose line produces NOTHING BUT a `keywords` entry are listed,
/// and membership was checked across every card carrying the keyword rather
/// than across the handful whose entire text is that one line — a sample small
/// enough to be actively misleading. Megamorph (31 of 31 cards), Madness (61 of
/// 61) and Buyback (42 of 42) always produce a replacement or an additional
/// cost as well, and were removed after that check; Cycling, Echo and Unearth
/// generate an ability or a trigger and never made the list.
const COSTED_KEYWORDS: &[(&str, bool)] = &[
    ("morph", false),
    ("foretell", false),
    ("disturb", false),
    ("dash", false),
    ("spectacle", false),
    ("warp", false),
    ("mayhem", false),
    ("freerunning", false),
    ("ward", true),
];

/// `<Keyword> <mana cost>` — a keyword line that carries a cost.
pub fn costed_keyword_line(i: In<'_>) -> Option<Keyword> {
    let name = i.first_word()?;
    let (_, wrapped) = COSTED_KEYWORDS.iter().find(|(k, _)| *k == name)?;
    let rest = i.take_from_n(1);

    let cost = mana_cost_only(rest)?;
    let payload = if *wrapped {
        KeywordCost::Wrapped(WrappedKeywordCost::Mana(cost))
    } else {
        KeywordCost::Bare(cost)
    };

    let mut c = name.chars();
    let cap = c
        .next()
        .map(|x| x.to_uppercase().to_string())
        .unwrap_or_default();
    let printed = format!("{cap}{}", c.as_str());

    let mut map = std::collections::BTreeMap::new();
    map.insert(printed, payload);
    Some(Keyword::Costed(map))
}

/// A run of mana symbols that consumes the whole remaining line.
fn mana_cost_only(i: In<'_>) -> Option<ManaCost> {
    let AbilityCost::Mana { cost } = crate::cost::ability_cost(i)? else {
        return None;
    };
    Some(cost)
}

/// `Cycling <cost>` — CR 702.29a.
///
/// A keyword that generates BEHAVIOUR as well as an entry, so it lowers to
/// both: the `keywords` entry the engine keys by name, and the activated
/// ability the keyword stands for — "discard this card, pay the cost: draw a
/// card", activatable from the HAND rather than the battlefield.
///
/// None of that is printed outside the reminder text the grammar drops, which
/// is exactly why it is derived from the keyword rather than parsed.
pub fn cycling_line(i: In<'_>) -> Option<(Keyword, AbilityDefinition)> {
    let (r, _) = word("cycling")(i).ok()?;
    let cost = mana_cost_only(r)?;

    let mut map = std::collections::BTreeMap::new();
    map.insert(
        "Cycling".to_string(),
        KeywordCost::Wrapped(WrappedKeywordCost::Mana(cost.clone())),
    );

    let mut a = AbilityDefinition::new(
        AbilityKind::Activated,
        Effect::Draw {
            count: phase_oracle_ast::Quantity::fixed(1),
            target: TargetFilter::Controller,
        },
    );
    a.cost = Some(AbilityCost::Composite {
        costs: vec![
            AbilityCost::Mana { cost },
            AbilityCost::Discard {
                count: phase_oracle_ast::Quantity::fixed(1),
                filter: None,
                selection_random: false,
                // CR 702.29a: the card discarded is THIS one.
                self_scope: true,
            },
        ],
    });
    // CR 602.1: a cycling ability is activated from the hand, so the zone has
    // to be stated — absence would mean the battlefield.
    a.activation_zone = Some(phase_oracle_ast::Zone::Hand);
    a.ability_tag = Some(AbilityTag::Cycling);
    Some((Keyword::Costed(map), a))
}
