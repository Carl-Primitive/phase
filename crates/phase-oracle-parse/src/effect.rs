//! Effect productions: one printed instruction to one engine `Effect`.
//!
//! Every production takes a [`Subject`] and reads its scope, so a single arm
//! covers both the single-object and the mass form of an instruction. The
//! engine names scope in the variant (`Destroy` / `DestroyAll`), and
//! [`scoped`] is the ONE place that translation happens.

use phase_oracle_ast::{
    ChoiceTiming, ControllerRef, Duration, Effect, ManaColor, ManaProduced, ManaShard,
    Modification, PlayerScope, Quantity, StaticAbility, TapScope, TapState, TargetFilter, ZoneName,
};

use crate::prim::{any_of, fail, phrase, phrase_alt, quantity, word, In, ManaSym, R};
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
    /// CR 101.4: the clause's subject was a CLASS of players, so the effect is
    /// iterated once per player rather than aimed at them. "Each opponent mills
    /// a card" is a controller-shaped mill run for each opponent.
    pub player_scope: Option<PlayerScope>,
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
            player_scope: None,
        }
    }

    /// Facts for a clause whose SUBJECT is a class of players.
    fn iterated(s: &Subject) -> Self {
        Self {
            player_scope: player_scope_of(s),
            ..Self::of(s)
        }
    }

    pub fn merge(self, other: Self) -> Self {
        Self {
            targeted_player: self.targeted_player || other.targeted_player,
            targeted_object: self.targeted_object || other.targeted_object,
            player_scope: self.player_scope.or(other.player_scope),
        }
    }
}

/// The player class a mass subject names, if it names one.
///
/// CR 102.1: a player is not an object, so a player-shaped filter with `All`
/// scope is an ITERATION, not a mass target.
fn player_scope_of(s: &Subject) -> Option<PlayerScope> {
    if s.scope != Scope::All || s.targeted {
        return None;
    }
    let TargetFilter::Typed(t) = &s.filter else {
        return None;
    };
    if !t.type_filters.is_empty() {
        return None;
    }
    match t.controller? {
        ControllerRef::Opponent => Some(PlayerScope::Opponent),
        ControllerRef::EachPlayer => Some(PlayerScope::All),
        _ => None,
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

/// The subject an ITERATED clause writes its effect against.
///
/// An iterated effect is performed by each player in turn, so its own player
/// slot names the acting player — which the engine spells as the controller,
/// with `player_scope` saying who that is on each pass.
fn acting_subject(s: &Subject) -> Subject {
    if player_scope_of(s).is_some() {
        Subject::single(TargetFilter::Controller)
    } else {
        s.clone()
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

/// Build the single or mass zone-change effect.
///
/// The mass form carries FEWER fields than the single one: the engine prints no
/// battlefield-entry riders on `ChangeZoneAll`. That asymmetry is verified
/// against the corpus, not assumed.
fn zone_change(t: TargetFilter, origin: Option<ZoneName>, dest: ZoneName, all: bool) -> Effect {
    if all {
        Effect::ChangeZoneAll {
            origin,
            destination: dest,
            target: t,
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

    if let Ok((r, produced)) = add_mana(i) {
        return Ok((r, (Effect::Mana { produced }, ClauseFacts::default())));
    }

    if let Ok((r, e)) = create_token(i) {
        return Ok((r, (e, ClauseFacts::default())));
    }

    fail(i)
}

/// `add <mana>` — CR 605.1a.
///
/// Three printed shapes, and the engine distinguishes them because they are
/// genuinely different: a list of symbols is fixed, `{C}` repeated is a COUNT
/// of one thing, and "one mana of any color" is a choice made on resolution.
fn add_mana(i: In<'_>) -> R<'_, ManaProduced> {
    let (r, _) = word("add")(i)?;

    // "add one mana of any color" / "add two mana of any one color"
    if let Ok((r2, count)) = quantity(r) {
        if let Ok((r3, _)) = word("mana")(r2) {
            const OF_ANY: &[(&str, ())] = &[
                ("of any color", ()),
                ("of any one color", ()),
                ("in any combination of colors", ()),
            ];
            if let Ok((r4, _)) = phrase_alt(OF_ANY)(r3) {
                return Ok((
                    r4,
                    ManaProduced::AnyOneColor {
                        count,
                        color_options: vec![
                            ManaColor::White,
                            ManaColor::Blue,
                            ManaColor::Black,
                            ManaColor::Red,
                            ManaColor::Green,
                        ],
                    },
                ));
            }
        }
    }

    // A run of mana symbols. Colourless is counted rather than listed, because
    // `{C}{C}` is two of one thing.
    let (mut rest, first) = crate::prim::mana_symbol(r)?;
    let mut colors = Vec::new();
    let mut colorless = 0i32;
    let mut push = |sym: ManaSym| -> bool {
        match sym {
            ManaSym::Shard(ManaShard::White) => colors.push(ManaColor::White),
            ManaSym::Shard(ManaShard::Blue) => colors.push(ManaColor::Blue),
            ManaSym::Shard(ManaShard::Black) => colors.push(ManaColor::Black),
            ManaSym::Shard(ManaShard::Red) => colors.push(ManaColor::Red),
            ManaSym::Shard(ManaShard::Green) => colors.push(ManaColor::Green),
            ManaSym::Shard(ManaShard::Colorless) => colorless += 1,
            // Hybrid, Phyrexian and `{X}` production are real shapes this
            // grammar has no production for; declining keeps the gap visible.
            _ => return false,
        }
        true
    };
    if !push(first) {
        return fail(i);
    }
    while let Ok((r2, sym)) = crate::prim::mana_symbol(rest) {
        if !push(sym) {
            return fail(i);
        }
        rest = r2;
    }

    match (colors.is_empty(), colorless) {
        (true, 0) => fail(i),
        (true, n) => Ok((
            rest,
            ManaProduced::Colorless {
                count: Quantity::fixed(n),
            },
        )),
        (false, 0) => Ok((rest, ManaProduced::Fixed { colors })),
        // A mixed run is a shape the engine spells differently; decline rather
        // than guess which half wins.
        (false, _) => fail(i),
    }
}

/// `create <n> [<p/t>] [<colors>] [<subtypes>] <types> token[s] [with <keywords>]`
///
/// CR 111.1. The printed order of a token's description is fixed by the
/// templating, which is what lets one production read every one of them.
fn create_token(i: In<'_>) -> R<'_, Effect> {
    let (r, _) = word("create")(i)?;
    let (r, count) = quantity(r).unwrap_or((r, Quantity::fixed(1)));

    // Power/toughness, present only for creature tokens.
    let (r, pt) = match crate::prim::pt_pair(r) {
        Ok((r2, pair)) => (r2, Some(pair)),
        Err(_) => (r, None),
    };

    let (r, t) = crate::target::token_body(r)?;
    let (r, _) = any_of(&["token", "tokens"])(r)?;

    // "with flying" / "with flying and vigilance"
    let (r, keywords) = token_keywords(r);

    // "that are tapped and attacking" / "that's tapped", the trailing spelling
    // of the same two flags the body can carry inline.
    let (r, trailing_tapped, attacking) = token_entry_state(r);
    let tapped = t.tapped || trailing_tapped;

    let (power, toughness) = pt.unwrap_or((0, 0));
    Ok((
        r,
        Effect::Token {
            name: t.name,
            power: Quantity::fixed(power),
            toughness: Quantity::fixed(toughness),
            types: t.types,
            colors: t.colors,
            keywords,
            tapped,
            count,
            owner: TargetFilter::Controller,
            enters_attacking: attacking,
        },
    ))
}

/// The keyword list a token is printed with.
fn token_keywords(i: In<'_>) -> (In<'_>, Vec<String>) {
    let Ok((mut rest, _)) = word("with")(i) else {
        return (i, Vec::new());
    };
    let mut out = Vec::new();
    loop {
        match keyword_word(rest) {
            Ok((r, (pascal, _))) => {
                out.push(pascal);
                rest = r;
            }
            Err(_) => break,
        }
        let after_sep = match rest.first() {
            Some(t) if t.kind == phase_oracle_lex::TokenKind::Comma => rest.take_from_n(1),
            _ => rest,
        };
        match word("and")(after_sep) {
            Ok((r, _)) => rest = r,
            Err(_) if after_sep != rest => rest = after_sep,
            Err(_) => break,
        }
    }
    if out.is_empty() {
        return (i, Vec::new());
    }
    (rest, out)
}

/// "that are tapped and attacking" / "that's tapped".
fn token_entry_state(i: In<'_>) -> (In<'_>, bool, bool) {
    const HEADS: &[(&str, ())] = &[("that are", ()), ("that's", ()), ("thats", ())];
    let Ok((mut rest, _)) = phrase_alt(HEADS)(i) else {
        return (i, false, false);
    };
    let (mut tapped, mut attacking) = (false, false);
    loop {
        if let Ok((r, _)) = word("tapped")(rest) {
            tapped = true;
            rest = r;
        } else if let Ok((r, _)) = word("attacking")(rest) {
            attacking = true;
            rest = r;
        } else {
            break;
        }
        match word("and")(rest) {
            Ok((r, _)) => rest = r,
            Err(_) => break,
        }
    }
    if !tapped && !attacking {
        return (i, false, false);
    }
    (rest, tapped, attacking)
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
    // The engine's `selection` field is only partly predictable: it correlates
    // with a `You` controller 93 to 38, so no rule reproduces it exactly. This
    // reading — an object the text did not TARGET is chosen during resolution
    // (CR 608.2d rather than CR 601.2c) — is the best-scoring one measured, and
    // is kept because dropping the field entirely scored WORSE. It is a known
    // approximation, not a settled rule.
    (!s.targeted && matches!(s.filter, TargetFilter::Typed(_)))
        .then_some(ChoiceTiming::AtResolution)
}

/// Recover the origin zone a filter already names, so a `return` production
/// does not have to parse "from your graveyard" twice.
fn subject_zone(f: &TargetFilter) -> Option<ZoneName> {
    // A disjunction carries the same zone on every branch once the trailing
    // qualifier has been distributed, so any branch answers for all:
    // "target instant or sorcery card from your graveyard".
    let t = match f {
        TargetFilter::Typed(t) => t,
        TargetFilter::Or { filters } | TargetFilter::And { filters } => {
            return filters.iter().find_map(subject_zone)
        }
        _ => return None,
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
        // "gains flying and first strike" — ONE grant of several keywords, not
        // several grants. The verb is printed once, so the list is read here
        // rather than by the outer conjunction, which would look for a second
        // "gains" that is not there.
        if let Ok((r2, keywords)) = keyword_list(r) {
            return Ok((
                r2,
                Predicate::Continuous {
                    modifications: keywords
                        .into_iter()
                        .map(|keyword| Modification::AddKeyword { keyword })
                        .collect(),
                },
            ));
        }
    }

    if let Ok((r, _)) = any_of(&["loses", "lose"])(i) {
        let (r, q) = quantity(r)?;
        let (r, _) = word("life")(r)?;
        // Unlike `GainLife`, the engine PRINTS the subject here even when it is
        // the controller. The two life effects genuinely disagree about this;
        // the mirror follows each of them rather than tidying either.
        return Ok((
            r,
            Predicate::Instant(
                Effect::LoseLife {
                    amount: q,
                    target: Some(s.filter.clone()),
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
        // CR 102.1: a class of PLAYERS is not a mass object target, so it takes
        // its own effect family rather than the object-shaped `DamageAll`.
        if let Some(scope) = player_scope_of(&victim) {
            return Ok((
                r,
                Predicate::Instant(
                    Effect::DamageEachPlayer {
                        amount: q,
                        player_filter: scope,
                    },
                    facts,
                ),
            ));
        }
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
/// Keywords the engine hoists into a card's `keywords` array as a BARE STRING.
///
/// Derived from the corpus, not from intuition: for each candidate, every card
/// whose entire Oracle text is that one keyword was checked to confirm the
/// engine emits nothing else for it. Keywords that also generate a trigger or a
/// static ability (evolve, exalted, unleash, extort, flanking, persist,
/// undying, changeling, mentor, myriad, provoke, dethrone) are deliberately
/// ABSENT: claiming them here would hoist the keyword and silently drop the
/// behaviour it stands for.
///
/// Landwalk is absent for a different reason — it is parameterized, and
/// [`landwalk`] handles it.
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
    "horsemanship",
    "shadow",
    "devoid",
    "prowess",
    "skulk",
    "convoke",
    "delve",
    "cascade",
    "improvise",
    "daybound",
    "nightbound",
];

/// CR 702.14: landwalk, which is ONE keyword parameterized by a land type
/// rather than five keywords that happen to rhyme.
pub fn landwalk(w: &str) -> Option<&'static str> {
    Some(match w {
        "plainswalk" => "Plains",
        "islandwalk" => "Island",
        "swampwalk" => "Swamp",
        "mountainwalk" => "Mountain",
        "forestwalk" => "Forest",
        _ => return None,
    })
}

/// One or more keywords joined by "and" or commas, after a single grant verb.
fn keyword_list(i: In<'_>) -> R<'_, Vec<String>> {
    let (mut rest, (first, _)) = keyword_word(i)?;
    let mut out = vec![first];
    loop {
        let after_comma = match rest.first() {
            Some(t) if t.kind == phase_oracle_lex::TokenKind::Comma => rest.take_from_n(1),
            _ => rest,
        };
        let after_and = match word("and")(after_comma) {
            Ok((r, _)) => r,
            Err(_) if after_comma != rest => after_comma,
            Err(_) => break,
        };
        match keyword_word(after_and) {
            Ok((r, (kw, _))) => {
                out.push(kw);
                rest = r;
            }
            Err(_) => break,
        }
    }
    Ok((rest, out))
}

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
pub fn subject_clause(i: In<'_>) -> R<'_, ClauseParse> {
    let (r, printed_subject) = subject(i)?;
    // CR 101.4: "each opponent mills a card" is a CONTROLLER-shaped mill run
    // once per opponent, not a mill aimed at opponents. The acting subject is
    // what the effect is written against; `player_scope` says who acts.
    let s = acting_subject(&printed_subject);
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

    let (mut effects, standalone, facts) =
        lower_predicates(&s, preds, dur.clone(), &printed, &printed_subject);
    let facts = facts.merge(ClauseFacts::iterated(&printed_subject));

    // CR 101.4: when `player_scope` names who acts, the effect's own player
    // slot is redundant and the engine leaves it out.
    if facts.player_scope.is_some() {
        for e in &mut effects {
            if let Effect::LoseLife { target, .. } = e {
                *target = None;
            }
        }
    }
    Ok((
        rest,
        ClauseParse {
            effects,
            standalone,
            facts,
            duration: dur,
        },
    ))
}

/// Turn a run of predicates into engine effects.
///
/// The split is structural. A run containing ANY keyword grant becomes one
/// `GenericEffect` carrying a `StaticAbility`; a run of pure power/toughness
/// changes becomes `Pump`/`PumpAll`; anything else keeps its own effect. That
/// is the engine's own division and it is why "gains flying" and "gets +1/+1"
/// do not lower the same way despite reading alike.
/// What one subject-initial clause lowered to.
pub struct ClauseParse {
    pub effects: Vec<Effect>,
    /// Set when the clause is a CONTINUOUS effect with no printed end — an
    /// ability of the permanent itself rather than something a spell does.
    /// CR 611.2: such an effect lasts as long as its source, which is exactly
    /// what a static ability is, so the engine files it under
    /// `static_abilities` instead of wrapping it in a resolving effect.
    pub standalone: Option<StaticAbility>,
    pub facts: ClauseFacts,
    pub duration: Option<Duration>,
}

fn lower_predicates(
    s: &Subject,
    preds: Vec<Predicate>,
    duration: Option<Duration>,
    printed: &str,
    printed_subject: &Subject,
) -> (Vec<Effect>, Option<StaticAbility>, ClauseFacts) {
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

        // A continuous change with no printed end, on an object the text did
        // not target, is the permanent's own static ability.
        let standalone = if duration.is_none() && !printed_subject.targeted {
            let mut sa =
                StaticAbility::continuous(spell_out_type_line(affected.clone()), mods.clone());
            sa.description = None;
            Some(sa)
        } else {
            None
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
            // CR 613.4b: a filter-based subject the text did NOT target is the
            // mass form even when the noun is singular — "enchanted creature
            // gets +0/+1" pumps whatever this Aura is on, through the filter.
            // Targeting, not plurality, is what separates the two here: a
            // measured split of 536 untargeted `PumpAll` against 898 targeted
            // `Pump` in the corpus.
            let pt_scope = if !s.targeted && matches!(s.filter, TargetFilter::Typed(_)) {
                Scope::All
            } else {
                Scope::Single
            };
            scoped(
                pt_scope,
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
        // A standalone static only stands alone when the clause said nothing
        // else: "creatures you control get +1/+1" is one, "target player draws
        // a card and creatures get +1/+1" is not.
        let standalone = standalone.filter(|_| instants.is_empty());
        let mut out = vec![effect];
        out.append(&mut instants);
        return (out, standalone, facts);
    }

    (instants, None, facts)
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

/// Spell out the type line a bare creature subtype implies.
///
/// POSITIONAL, and measured: a static ability's `affected` slot carries
/// `[Creature, Subtype(Elf)]` 620 times against 106 without, while an effect's
/// `target` slot carries the bare `[Subtype(Spirit)]` 636 times against 108
/// with. Same printed words, two slots, two shapes — so the expansion happens
/// where the static is BUILT rather than where the filter is parsed.
fn spell_out_type_line(f: TargetFilter) -> TargetFilter {
    let TargetFilter::Typed(mut t) = f else {
        return f;
    };
    let bare_creature_subtype = t.type_filters.len() == 1
        && matches!(&t.type_filters[0],
            phase_oracle_ast::TypeFilter::Subtype(s) if crate::subtypes::is_creature_type(s));
    if bare_creature_subtype {
        t.type_filters
            .insert(0, phase_oracle_ast::TypeFilter::Creature);
    }
    TargetFilter::Typed(t)
}
