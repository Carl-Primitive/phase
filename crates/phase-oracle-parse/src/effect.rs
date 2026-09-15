//! Effect productions: one printed instruction to one engine `Effect`.
//!
//! Every production takes a [`Subject`] and reads its scope, so a single arm
//! covers both the single-object and the mass form of an instruction. The
//! engine names scope in the variant (`Destroy` / `DestroyAll`), and
//! [`scoped`] is the ONE place that translation happens.

use phase_oracle_ast::{
    ChoiceTiming, Duration, Effect, Modification, Quantity, StaticAbility, TapScope, TapState,
    TargetFilter, ZoneName,
};

use crate::prim::{any_of, fail, phrase, phrase_alt, quantity, word, In, R};
use crate::target::{subject, Scope, Subject};

/// Facts a clause establishes that its EFFECT shape cannot carry.
///
/// `targeted_player` is the motivating case: "target opponent" and "each
/// opponent" lower to the same `TargetFilter`, but only the first fills a
/// trigger's `valid_target` slot. The distinction is real (CR 115.1 — a target
/// is chosen on announcement) and is lost at the AST layer, so the grammar
/// reports it alongside rather than guessing later from the printed text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClauseFacts {
    pub targeted_player: bool,
    pub targeted_object: bool,
}

impl ClauseFacts {
    fn of(s: &Subject) -> Self {
        let player_shaped = match &s.filter {
            TargetFilter::Player => true,
            TargetFilter::Typed(t) => t.type_filters.is_empty(),
            _ => false,
        };
        Self {
            targeted_player: s.targeted && player_shaped,
            targeted_object: s.targeted && !player_shaped,
        }
    }

    pub fn merge(self, other: Self) -> Self {
        Self {
            targeted_player: self.targeted_player || other.targeted_player,
            targeted_object: self.targeted_object || other.targeted_object,
        }
    }
}

/// Pick between an effect's single-object and mass form.
///
/// This is the whole of the "scope named in the variant" translation. Keeping
/// it in one function is what lets every production above be written once.
fn scoped(scope: Scope, single: Effect, all: Effect) -> Effect {
    match scope {
        Scope::Single => single,
        Scope::All => all,
    }
}

fn cards_noun(i: In<'_>) -> R<'_, ()> {
    any_of(&["card", "cards"])(i).map(|(r, _)| (r, ()))
}

fn counters_noun(i: In<'_>) -> R<'_, ()> {
    any_of(&["counter", "counters"])(i).map(|(r, _)| (r, ()))
}

/// CR 119.3: `GainLife`/`LoseLife` OMIT the player field when the subject is
/// the controller, so absence encodes "you". Reproduced in one place rather
/// than at each call site, and flagged for a later format proposal.
fn implicit_controller(f: &TargetFilter) -> Option<TargetFilter> {
    match f {
        TargetFilter::Controller => None,
        other => Some(other.clone()),
    }
}

/// Quantity-then-noun, where a bare noun means one: "draw a card" / "draw cards".
fn count_of(i: In<'_>, noun: fn(In<'_>) -> R<'_, ()>) -> R<'_, Quantity> {
    if let Ok((r, q)) = quantity(i) {
        let (r, _) = noun(r)?;
        return Ok((r, q));
    }
    let (r, _) = noun(i)?;
    Ok((r, Quantity::fixed(1)))
}

fn zone_change(t: TargetFilter, origin: Option<ZoneName>, dest: ZoneName, all: bool) -> Effect {
    if all {
        Effect::ChangeZoneAll {
            origin,
            destination: dest,
            target: t,
            owner_library: false,
            enter_transformed: false,
            enter_tapped: false,
            enters_attacking: false,
        }
    } else {
        Effect::ChangeZone {
            origin,
            destination: dest,
            target: t,
            owner_library: false,
            enter_transformed: false,
            enter_tapped: false,
            enters_attacking: false,
        }
    }
}

/// Verb-initial clauses: the imperative voice, with the actor elided.
///
/// CR 608.2: an instruction with no printed subject is performed by the
/// ability's controller.
pub fn imperative(i: In<'_>) -> R<'_, (Effect, ClauseFacts)> {
    if let Ok((r, _)) = word("destroy")(i) {
        let (r, s) = subject(r)?;
        let f = ClauseFacts::of(&s);
        return Ok((
            r,
            (
                scoped(
                    s.scope,
                    Effect::Destroy {
                        target: s.filter.clone(),
                        cant_regenerate: false,
                    },
                    Effect::DestroyAll {
                        target: s.filter,
                        cant_regenerate: false,
                    },
                ),
                f,
            ),
        ));
    }

    // CR 701.5a: exile is a zone change, not its own effect family.
    if let Ok((r, _)) = word("exile")(i) {
        let (r, s) = subject(r)?;
        let f = ClauseFacts::of(&s);
        let origin = subject_zone(&s.filter);
        return Ok((
            r,
            (
                scoped(
                    s.scope,
                    zone_change(s.filter.clone(), origin, ZoneName::Exile, false),
                    zone_change(s.filter, origin, ZoneName::Exile, true),
                ),
                f,
            ),
        ));
    }

    if let Ok((r, _)) = word("counter")(i) {
        let (r, s) = subject(r)?;
        let f = ClauseFacts::of(&s);
        return Ok((r, (Effect::Counter { target: s.filter }, f)));
    }

    if let Ok((r, _)) = word("regenerate")(i) {
        let (r, s) = subject(r)?;
        let f = ClauseFacts::of(&s);
        return Ok((r, (Effect::Regenerate { target: s.filter }, f)));
    }

    if let Ok((r, state)) = phrase_alt(&[("tap", TapState::Tap), ("untap", TapState::Untap)])(i) {
        let (r, s) = subject(r)?;
        let f = ClauseFacts::of(&s);
        let scope = match s.scope {
            Scope::Single => TapScope::Single,
            Scope::All => TapScope::All,
        };
        return Ok((
            r,
            (
                Effect::SetTapState {
                    target: s.filter,
                    scope,
                    state,
                },
                f,
            ),
        ));
    }

    if let Ok((r, _)) = word("draw")(i) {
        let (r, q) = count_of(r, cards_noun)?;
        return Ok((
            r,
            (
                Effect::Draw {
                    count: q,
                    target: TargetFilter::Controller,
                },
                ClauseFacts::default(),
            ),
        ));
    }

    if let Ok((r, _)) = word("discard")(i) {
        let (r, q) = count_of(r, cards_noun)?;
        return Ok((
            r,
            (
                Effect::Discard {
                    count: q,
                    target: TargetFilter::Controller,
                },
                ClauseFacts::default(),
            ),
        ));
    }

    if let Ok((r, _)) = word("sacrifice")(i) {
        let (r, s) = subject(r)?;
        let f = ClauseFacts::of(&s);
        return Ok((
            r,
            (
                Effect::Sacrifice {
                    target: s.filter,
                    count: Quantity::fixed(1),
                },
                f,
            ),
        ));
    }

    if let Ok((r, which)) = any_of(&["scry", "surveil", "mill"])(i) {
        let (r, q) = quantity(r).unwrap_or((r, Quantity::fixed(1)));
        // "mill three cards" prints the noun; scry and surveil do not.
        let r = cards_noun(r).map(|(rr, _)| rr).unwrap_or(r);
        let t = TargetFilter::Controller;
        let e = match which {
            "scry" => Effect::Scry {
                count: q,
                target: t,
            },
            "surveil" => Effect::Surveil {
                count: q,
                target: t,
            },
            _ => Effect::Mill {
                count: q,
                target: t,
                destination: ZoneName::Graveyard,
            },
        };
        return Ok((r, (e, ClauseFacts::default())));
    }

    if let Ok((r, _)) = word("shuffle")(i) {
        // "shuffle" alone and "shuffle your library" are the same instruction.
        let r = phrase("your library")(r).map(|(rr, _)| rr).unwrap_or(r);
        return Ok((
            r,
            (
                Effect::Shuffle {
                    target: TargetFilter::Controller,
                },
                ClauseFacts::default(),
            ),
        ));
    }

    // "put a +1/+1 counter on target creature"
    if let Ok((r, _)) = word("put")(i) {
        let (r, q) = quantity(r).unwrap_or((r, Quantity::fixed(1)));
        let (r, c) = crate::prim::counter_type(r)?;
        let (r, _) = counters_noun(r)?;
        let (r, _) = word("on")(r)?;
        let (r, s) = subject(r)?;
        let f = ClauseFacts::of(&s);
        return Ok((
            r,
            (
                scoped(
                    s.scope,
                    Effect::PutCounter {
                        counter_type: c.clone(),
                        count: q.clone(),
                        target: s.filter.clone(),
                    },
                    Effect::PutCounterAll {
                        counter_type: c,
                        count: q,
                        target: s.filter,
                    },
                ),
                f,
            ),
        ));
    }

    if let Ok((r, _)) = word("return")(i) {
        return return_clause(i, r);
    }

    fail(i)
}

/// `return <subject> to <zone>`.
///
/// A hand-bounce from the battlefield is `Bounce`; every other origin is a
/// `ChangeZone`. The origin recovered from the subject's own zone property is
/// what decides it, so "return target creature card from your graveyard to
/// your hand" is not mistaken for a battlefield bounce.
fn return_clause<'a>(orig: In<'a>, r: In<'a>) -> R<'a, (Effect, ClauseFacts)> {
    let (r, s) = subject(r)?;
    let facts = ClauseFacts::of(&s);
    const DESTS: &[(&str, ZoneName)] = &[
        ("to its owner's hand", ZoneName::Hand),
        ("to its owners hand", ZoneName::Hand),
        ("to their owner's hand", ZoneName::Hand),
        ("to their owners hand", ZoneName::Hand),
        ("to their owners' hands", ZoneName::Hand),
        ("to your hand", ZoneName::Hand),
        ("to the battlefield", ZoneName::Battlefield),
        ("to its owner's library", ZoneName::Library),
    ];
    let Ok((r, dest)) = phrase_alt(DESTS)(r) else {
        return fail(orig);
    };

    let origin = subject_zone(&s.filter);
    if dest == ZoneName::Hand && origin.is_none() {
        return Ok((
            r,
            (
                scoped(
                    s.scope,
                    Effect::Bounce {
                        target: s.filter.clone(),
                        destination: None,
                        // CR 601.2c vs CR 608.2d: an object the text did NOT
                        // make a target is chosen while the ability resolves,
                        // not as it goes on the stack. Only a filter that
                        // actually ranges over objects needs the choice;
                        // `~` and a back-reference name one already.
                        selection: choice_timing(&s),
                    },
                    Effect::BounceAll { target: s.filter },
                ),
                facts,
            ),
        ));
    }
    Ok((
        r,
        (
            scoped(
                s.scope,
                zone_change(s.filter.clone(), origin, dest, false),
                zone_change(s.filter, origin, dest, true),
            ),
            facts,
        ),
    ))
}

/// When an object choice is made, for a subject that was not targeted.
fn choice_timing(s: &Subject) -> Option<ChoiceTiming> {
    (!s.targeted && matches!(s.filter, TargetFilter::Typed(_)))
        .then_some(ChoiceTiming::AtResolution)
}

/// Recover the origin zone a filter already names, so a `return` production
/// does not have to parse "from your graveyard" twice.
fn subject_zone(f: &TargetFilter) -> Option<ZoneName> {
    let TargetFilter::Typed(t) = f else {
        return None;
    };
    t.properties.iter().find_map(|p| match p {
        phase_oracle_ast::FilterProp::InZone { zone } => Some(match zone {
            phase_oracle_ast::Zone::Graveyard => ZoneName::Graveyard,
            phase_oracle_ast::Zone::Hand => ZoneName::Hand,
            phase_oracle_ast::Zone::Library => ZoneName::Library,
            phase_oracle_ast::Zone::Exile => ZoneName::Exile,
            phase_oracle_ast::Zone::Battlefield => ZoneName::Battlefield,
            phase_oracle_ast::Zone::Stack => ZoneName::Stack,
            phase_oracle_ast::Zone::Command => ZoneName::Command,
        }),
        _ => None,
    })
}

/// One predicate attached to an already-parsed subject.
///
/// A CONTINUOUS predicate ("gets +1/+1", "gains flying") is kept apart from an
/// instantaneous one ("draws a card"), because the engine lowers a run of
/// continuous predicates into a single `StaticAbility` and everything else into
/// its own effect.
enum Predicate {
    /// Layer-changing modifications.
    ///
    /// A LIST, not one modification: "gets +1/+1" is two independent layer-7c
    /// changes (CR 613.4b), and collapsing them into one variant would force
    /// every consumer to know that power implies toughness.
    Continuous {
        modifications: Vec<Modification>,
    },
    Instant(Effect, ClauseFacts),
}

fn predicate<'a>(s: &Subject, i: In<'a>) -> R<'a, Predicate> {
    if let Ok((r, _)) = any_of(&["gets", "get"])(i) {
        let (r, (p, t)) = crate::prim::pt_pair(r)?;
        return Ok((
            r,
            Predicate::Continuous {
                // CR 613.4b: power and toughness are independent layer-7c changes.
                modifications: vec![
                    Modification::AddPower { value: p },
                    Modification::AddToughness { value: t },
                ],
            },
        ));
    }

    // "gains 3 life" before "gains flying": a count followed by the noun `life`
    // is unambiguous, and a keyword grant never takes a count.
    if let Ok((r, _)) = any_of(&["gains", "gain", "has", "have"])(i) {
        if let Ok((r2, q)) = quantity(r) {
            if let Ok((r3, _)) = word("life")(r2) {
                return Ok((
                    r3,
                    Predicate::Instant(
                        Effect::GainLife {
                            amount: q,
                            player: implicit_controller(&s.filter),
                        },
                        ClauseFacts::of(s),
                    ),
                ));
            }
        }
        if let Ok((r2, (pascal, _printed))) = keyword_word(r) {
            return Ok((
                r2,
                Predicate::Continuous {
                    modifications: vec![Modification::AddKeyword { keyword: pascal }],
                },
            ));
        }
    }

    if let Ok((r, _)) = any_of(&["loses", "lose"])(i) {
        let (r, q) = quantity(r)?;
        let (r, _) = word("life")(r)?;
        return Ok((
            r,
            Predicate::Instant(
                Effect::LoseLife {
                    amount: q,
                    target: implicit_controller(&s.filter),
                },
                ClauseFacts::of(s),
            ),
        ));
    }

    if let Ok((r, _)) = any_of(&["draws", "draw"])(i) {
        let (r, q) = count_of(r, cards_noun)?;
        return Ok((
            r,
            Predicate::Instant(
                Effect::Draw {
                    count: q,
                    target: s.filter.clone(),
                },
                ClauseFacts::of(s),
            ),
        ));
    }

    if let Ok((r, _)) = any_of(&["discards", "discard"])(i) {
        let (r, q) = count_of(r, cards_noun)?;
        return Ok((
            r,
            Predicate::Instant(
                Effect::Discard {
                    count: q,
                    target: s.filter.clone(),
                },
                ClauseFacts::of(s),
            ),
        ));
    }

    if let Ok((r, _)) = any_of(&["mills", "mill"])(i) {
        let (r, q) = count_of(r, cards_noun)?;
        return Ok((
            r,
            Predicate::Instant(
                Effect::Mill {
                    count: q,
                    target: s.filter.clone(),
                    destination: ZoneName::Graveyard,
                },
                ClauseFacts::of(s),
            ),
        ));
    }

    if let Ok((r, _)) = any_of(&["sacrifices", "sacrifice"])(i) {
        let (r, obj) = subject(r)?;
        let facts = ClauseFacts::of(s).merge(ClauseFacts::of(&obj));
        return Ok((
            r,
            Predicate::Instant(
                Effect::Sacrifice {
                    target: obj.filter,
                    count: Quantity::fixed(1),
                },
                facts,
            ),
        ));
    }

    // "<source> deals N damage to <victim>"
    if let Ok((r, _)) = any_of(&["deals", "deal"])(i) {
        let (r, q) = quantity(r)?;
        let (r, _) = word("damage")(r)?;
        let (r, _) = word("to")(r)?;
        let (r, victim) = subject(r)?;
        let facts = ClauseFacts::of(s).merge(ClauseFacts::of(&victim));
        return Ok((
            r,
            Predicate::Instant(
                scoped(
                    victim.scope,
                    Effect::DealDamage {
                        amount: q.clone(),
                        target: victim.filter.clone(),
                    },
                    Effect::DamageAll {
                        amount: q,
                        target: victim.filter,
                    },
                ),
                facts,
            ),
        ));
    }

    fail(i)
}

/// The printed keyword vocabulary.
///
/// A closed list on purpose: an unrecognized word after "gains" is far more
/// likely to be an unparsed phrase than a keyword, and guessing would make the
/// grammar claim clauses it does not understand.
const KEYWORDS: &[&str] = &[
    "flying",
    "trample",
    "vigilance",
    "haste",
    "reach",
    "menace",
    "defender",
    "deathtouch",
    "lifelink",
    "hexproof",
    "shroud",
    "indestructible",
    "flash",
    "intimidate",
    "fear",
    "banding",
    "infect",
    "wither",
    "changeling",
    "persist",
    "undying",
    "exalted",
    "prowess",
    "skulk",
    "horsemanship",
    "shadow",
    "devoid",
    "ingest",
    "myriad",
    "melee",
    "mentor",
    "afterlife",
    "convoke",
    "delve",
    "cascade",
    "storm",
    "islandwalk",
    "swampwalk",
    "forestwalk",
    "mountainwalk",
    "plainswalk",
    "flanking",
    "provoke",
    "soulbond",
    "unleash",
    "evolve",
    "extort",
    "battalion",
    "toxic",
    "decayed",
    "daybound",
    "nightbound",
    "exploit",
    "dethrone",
    "improvise",
];

/// One keyword, yielding the engine's PascalCase name and the printed spelling.
///
/// Both are needed: the modification carries "FirstStrike" while the static
/// ability's description carries "first strike".
pub fn keyword_word(i: In<'_>) -> R<'_, (String, String)> {
    const TWO_WORD: &[(&str, &str)] = &[
        ("first strike", "FirstStrike"),
        ("double strike", "DoubleStrike"),
    ];
    for (p, name) in TWO_WORD {
        if let Ok((r, _)) = crate::prim::phrase_static(p)(i) {
            return Ok((r, ((*name).to_string(), (*p).to_string())));
        }
    }
    let Some(w) = i.first_word() else {
        return fail(i);
    };
    if !KEYWORDS.contains(&w.as_str()) {
        return fail(i);
    }
    let mut c = w.chars();
    let cap = c
        .next()
        .map(|x| x.to_uppercase().to_string())
        .unwrap_or_default();
    Ok((i.take_from_n(1), (format!("{cap}{}", c.as_str()), w)))
}

/// Subject-initial clauses: `<subject> <predicate> [and <predicate>]*`.
///
/// A conjunction reuses the leading subject, so "Target creature gets +1/+1 and
/// gains flying" is one clause rather than a decline — and, because both
/// predicates are continuous, it lowers to ONE static ability the way the
/// engine prints it.
pub fn subject_clause(i: In<'_>) -> R<'_, (Vec<Effect>, ClauseFacts, Option<Duration>)> {
    let (r, s) = subject(i)?;
    let (mut rest, first) = predicate(&s, r)?;
    let mut preds = vec![first];

    // CR 601.2c: a target is chosen ONCE. A second predicate about the same
    // printed subject refers back to that choice, which the engine spells
    // `ParentTarget` rather than repeating the filter — otherwise "target
    // player draws three cards and loses 3 life" would choose twice.
    let back_ref = Subject {
        filter: TargetFilter::ParentTarget,
        scope: s.scope,
        targeted: false,
    };
    let later = if s.targeted { &back_ref } else { &s };

    while let Ok((r2, _)) = word("and")(rest) {
        match predicate(later, r2) {
            Ok((r3, next)) => {
                preds.push(next);
                rest = r3;
            }
            Err(_) => break,
        }
    }

    // The predicate region, as printed. The engine renders a static ability's
    // description verbatim from the card rather than normalizing verb forms
    // ("get +1/+1 and gains flying"), so it is read from the source here
    // instead of being reassembled from the parsed predicates.
    let consumed = r.toks.len() - rest.toks.len();
    let printed = infinitive(&crate::line::render(&r.toks[..consumed], r.src));

    // A duration attaches to the whole conjunction, not to one predicate, so it
    // is read here, handed to the continuous lowering below, AND returned: the
    // engine prints it in two places at once — inside the `GenericEffect` and
    // on the enclosing `AbilityDefinition`.
    let (rest, dur) = match crate::line::duration(rest) {
        Some((r, d)) => (r, Some(d)),
        None => (rest, None),
    };

    let (effects, facts) = lower_predicates(&s, preds, dur.clone(), &printed);
    Ok((rest, (effects, facts, dur)))
}

/// Turn a run of predicates into engine effects.
///
/// The split is structural. A run containing ANY keyword grant becomes one
/// `GenericEffect` carrying a `StaticAbility`; a run of pure power/toughness
/// changes becomes `Pump`/`PumpAll`; anything else keeps its own effect. That
/// is the engine's own division and it is why "gains flying" and "gets +1/+1"
/// do not lower the same way despite reading alike.
fn lower_predicates(
    s: &Subject,
    preds: Vec<Predicate>,
    duration: Option<Duration>,
    printed: &str,
) -> (Vec<Effect>, ClauseFacts) {
    let mut mods: Vec<Modification> = Vec::new();
    let mut instants: Vec<Effect> = Vec::new();
    let mut facts = ClauseFacts::default();
    let mut has_keyword = false;

    for p in preds {
        match p {
            Predicate::Continuous { modifications } => {
                has_keyword |= modifications
                    .iter()
                    .any(|m| matches!(m, Modification::AddKeyword { .. }));
                mods.extend(modifications);
            }
            Predicate::Instant(e, f) => {
                instants.push(e);
                facts = facts.merge(f);
            }
        }
    }

    if !mods.is_empty() {
        facts = facts.merge(ClauseFacts::of(s));
        let (target, affected) = if s.targeted {
            (Some(s.filter.clone()), TargetFilter::ParentTarget)
        } else {
            (None, s.filter.clone())
        };

        let effect = if has_keyword {
            let mut sa = StaticAbility::continuous(affected, mods);
            sa.description = Some(printed.to_string());
            Effect::GenericEffect {
                static_abilities: vec![sa],
                duration,
                target,
            }
        } else {
            // A pure power/toughness change has its own effect family.
            let power = mods
                .iter()
                .find_map(|m| match m {
                    Modification::AddPower { value } => Some(*value),
                    _ => None,
                })
                .unwrap_or(0);
            let toughness = mods
                .iter()
                .find_map(|m| match m {
                    Modification::AddToughness { value } => Some(*value),
                    _ => None,
                })
                .unwrap_or(0);
            scoped(
                s.scope,
                Effect::Pump {
                    power: Quantity::fixed(power),
                    toughness: Quantity::fixed(toughness),
                    target: s.filter.clone(),
                },
                Effect::PumpAll {
                    power: Quantity::fixed(power),
                    toughness: Quantity::fixed(toughness),
                    target: s.filter.clone(),
                },
            )
        };
        let mut out = vec![effect];
        out.append(&mut instants);
        return (out, facts);
    }

    (instants, facts)
}

/// Put a predicate's LEADING verb into the infinitive, as the engine prints it
/// in a static ability's description.
///
/// Only the leading verb: the engine renders "gets +1/+1 and gains flying" as
/// "get +1/+1 and gains flying", leaving later verbs exactly as printed. That
/// asymmetry looks like an oversight, and is reproduced rather than corrected
/// because this parser is a like-for-like replacement and a shape change must
/// not be smuggled in alongside one.
fn infinitive(printed: &str) -> String {
    const VERBS: &[(&str, &str)] = &[
        ("gains ", "gain "),
        ("gets ", "get "),
        ("has ", "have "),
        ("deals ", "deal "),
        ("loses ", "lose "),
        ("draws ", "draw "),
        ("discards ", "discard "),
        ("mills ", "mill "),
        ("sacrifices ", "sacrifice "),
    ];
    for (from, to) in VERBS {
        if let Some(rest) = printed.strip_prefix(*from) {
            return format!("{to}{rest}");
        }
    }
    printed.to_string()
}
