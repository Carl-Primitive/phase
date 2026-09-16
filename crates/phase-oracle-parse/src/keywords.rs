//! Keyword lines that carry an argument.
//!
//! A bare keyword hoists into the card's `keywords` array as a word. A
//! PARAMETERIZED one carries something the grammar has to read — the filter an
//! Aura may attach to, the cost an Equipment charges — and the engine spells
//! each of those differently. They live here rather than beside the bare
//! vocabulary because each is a small grammar, not a list entry.

use phase_oracle_ast::{
    AbilityCost, AbilityDefinition, AbilityKind, AbilityTag, ActivationRestriction, ControllerRef,
    CrewCost, Effect, Keyword, KeywordCost, ManaColor, ManaCost, PartnerVariant, ProtectionQuality,
    StaticAbility, TargetFilter, TypedFilter, WrappedKeywordCost,
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

/// `<Keyword> <mana cost>` occupying a WHOLE line.
pub fn costed_keyword_line(i: In<'_>) -> Option<Keyword> {
    let (rest, kw) = costed_keyword(i)?;
    crate::line::is_exhausted(rest).then_some(kw)
}

/// `<Keyword> <mana cost>` as one element of a keyword list.
pub fn costed_keyword(i: In<'_>) -> Option<(In<'_>, Keyword)> {
    let name = i.first_word()?;
    let (_, wrapped) = COSTED_KEYWORDS.iter().find(|(k, _)| *k == name)?;
    let rest = i.take_from_n(1);

    let (rest, cost) = mana_cost_prefix(rest)?;
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
    Some((rest, Keyword::Costed(map)))
}

/// A leading run of mana symbols, leaving whatever follows.
fn mana_cost_prefix(i: In<'_>) -> Option<(In<'_>, ManaCost)> {
    let (rest, sym) = crate::prim::mana_symbol(i).ok()?;
    let mut shards = Vec::new();
    let mut generic = 0u32;
    let mut push = |s: crate::prim::ManaSym| match s {
        crate::prim::ManaSym::Shard(sh) => shards.push(sh),
        crate::prim::ManaSym::Generic(n) => generic += n,
    };
    push(sym);
    let mut rest = rest;
    while let Ok((r, s)) = crate::prim::mana_symbol(rest) {
        push(s);
        rest = r;
    }
    Some((rest, ManaCost::Cost { shards, generic }))
}

/// A run of mana symbols that consumes the whole remaining line.
fn mana_cost_only(i: In<'_>) -> Option<ManaCost> {
    let (rest, cost) = mana_cost_prefix(i)?;
    crate::line::is_exhausted(rest).then_some(cost)
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

/// `protection from <quality>` — CR 702.16.
///
/// The quality is the keyword's argument, so it is keyed by name the way a cost
/// is. Only the colour and multicolour forms are built; "protection from
/// artifacts" and the card-type forms carry a different payload and decline.
pub fn protection_keyword(i: In<'_>) -> Option<(In<'_>, Keyword)> {
    let (r, _) = crate::prim::phrase("protection from")(i).ok()?;
    let quality = protection_quality(r)?;
    Some((quality.0, Keyword::Protection { quality: quality.1 }))
}

fn protection_quality(i: In<'_>) -> Option<(In<'_>, ProtectionQuality)> {
    const COLORS: &[(&str, ManaColor)] = &[
        ("white", ManaColor::White),
        ("blue", ManaColor::Blue),
        ("black", ManaColor::Black),
        ("red", ManaColor::Red),
        ("green", ManaColor::Green),
    ];
    if let Ok((r, c)) = crate::prim::phrase_alt(COLORS)(i) {
        return Some((r, ProtectionQuality::Color(c)));
    }
    let (r, _) = crate::prim::phrase("multicolored")(i).ok()?;
    Some((r, ProtectionQuality::Multicolored))
}

/// Keywords that ARE a characteristic-defining static ability. CR 604.3.
///
/// Both are printed as a bare word, so the bare-keyword vocabulary rejects them
/// — hoisting the word alone would drop the characteristic it sets. Building
/// the static is the way back in, the same route Cycling took.
pub fn characteristic_keyword_line(i: In<'_>) -> Option<(Keyword, StaticAbility)> {
    use phase_oracle_ast::Modification;

    let name = i.first_word()?;
    let (printed, modification) = match name.as_str() {
        // CR 702.73a.
        "changeling" => ("Changeling", Modification::AddAllCreatureTypes),
        // CR 702.114a: no colour at all, which an empty list expresses.
        "devoid" => ("Devoid", Modification::SetColor { colors: Vec::new() }),
        _ => return None,
    };
    if !crate::line::is_exhausted(i.take_from_n(1)) {
        return None;
    }

    let mut sa = StaticAbility::continuous(TargetFilter::SelfRef, vec![modification]);
    // CR 604.3: a characteristic-defining ability applies in every zone and
    // does not use the stack, which the engine records with this flag.
    sa.characteristic_defining = true;
    sa.description = None;
    Some((Keyword::Simple(printed.to_string()), sa))
}

/// `Crew <n>` — CR 702.122a.
///
/// The argument is a POWER threshold rather than a cost, so it carries its own
/// shape rather than joining the costed-keyword table.
pub fn crew_keyword(i: In<'_>) -> Option<(In<'_>, Keyword)> {
    let (r, _) = word("crew")(i).ok()?;
    let (r, power) = crate::prim::number(r).ok()?;
    let power = u32::try_from(power).ok()?;
    Some((
        r,
        Keyword::Crew {
            crew: CrewCost {
                power,
                once_per_turn: None,
            },
        },
    ))
}

/// The partner family — CR 702.124.
///
/// Hoisted as a bare entry because it is a DECK-CONSTRUCTION rule with no
/// gameplay behaviour: nothing is dropped by recording only the keyword. That
/// is what separates it from Storm, Exalted and Flanking, which print as bare
/// words too but each stand for a trigger.
pub fn partner_keyword(i: In<'_>) -> Option<(In<'_>, Keyword)> {
    const VARIANTS: &[(&str, PartnerVariant)] = &[
        ("choose a background", PartnerVariant::ChooseABackground),
        ("doctor's companion", PartnerVariant::DoctorsCompanion),
        ("doctors companion", PartnerVariant::DoctorsCompanion),
        ("friends forever", PartnerVariant::FriendsForever),
        ("partner", PartnerVariant::Generic),
    ];
    let (r, variant) = crate::prim::phrase_alt(VARIANTS)(i).ok()?;
    Some((r, Keyword::Partner { variant }))
}
