//! What a clause acts on.
//!
//! The grammar builds a [`Subject`]: a filter plus the SCOPE the printed text
//! gave it. Scope is carried on the subject rather than baked into a verb,
//! which is what lets one `destroy` production serve both "destroy target
//! creature" and "destroy all creatures". The engine names scope in the effect
//! variant instead (`Destroy` / `DestroyAll`); that translation happens once,
//! at emission, instead of doubling every verb production here.

use phase_oracle_ast::{
    ControllerRef, FilterProp, ManaColor, TargetFilter, TypeFilter, TypedFilter, Zone,
};

use crate::prim::{any_of, fail, phrase, phrase_alt, self_ref, word, In, R};

/// Whether the printed text named one object or a whole class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Single,
    All,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Subject {
    pub filter: TargetFilter,
    pub scope: Scope,
    /// CR 115.1: whether the object was chosen as a target on announcement.
    pub targeted: bool,
}

impl Subject {
    pub fn single(filter: TargetFilter) -> Self {
        Self {
            filter,
            scope: Scope::Single,
            targeted: false,
        }
    }
}

/// A printed core card type, singular or plural.
///
/// Plurals are folded here rather than by a `singular()` helper elsewhere, so
/// the grammar never carries a stringly-typed intermediate.
fn core_type(w: &str) -> Option<TypeFilter> {
    Some(match w {
        "creature" | "creatures" => TypeFilter::Creature,
        "land" | "lands" => TypeFilter::Land,
        "artifact" | "artifacts" => TypeFilter::Artifact,
        "enchantment" | "enchantments" => TypeFilter::Enchantment,
        "instant" | "instants" => TypeFilter::Instant,
        "sorcery" | "sorceries" => TypeFilter::Sorcery,
        "planeswalker" | "planeswalkers" => TypeFilter::Planeswalker,
        "battle" | "battles" => TypeFilter::Battle,
        "permanent" | "permanents" => TypeFilter::Permanent,
        "card" | "cards" => TypeFilter::Card,
        _ => return None,
    })
}

/// The body of a "non-" prefixed word: "nonland" and "non-Human" both yield
/// their tail. CR 205.2a.
fn non_body(w: &str) -> Option<&str> {
    let rest = w.strip_prefix("non")?;
    let rest = rest.strip_prefix('-').unwrap_or(rest);
    (!rest.is_empty()).then_some(rest)
}

/// A "non-" prefixed type word: "noncreature", "nonland".
fn non_type(w: &str) -> Option<TypeFilter> {
    core_type(non_body(w)?).map(|t| TypeFilter::Non(Box::new(t)))
}

fn color(w: &str) -> Option<ManaColor> {
    Some(match w {
        "white" => ManaColor::White,
        "blue" => ManaColor::Blue,
        "black" => ManaColor::Black,
        "red" => ManaColor::Red,
        "green" => ManaColor::Green,
        _ => return None,
    })
}

/// A "non-" prefixed colour word: "nonblack", "non-red".
fn non_color(w: &str) -> Option<ManaColor> {
    color(non_body(w)?)
}

const SUPERTYPES: &[&str] = &["basic", "legendary", "snow", "world"];

/// A printed word in the engine's capitalized spelling.
fn capitalized(w: &str) -> String {
    let mut c = w.chars();
    match c.next() {
        Some(f) => format!("{}{}", f.to_uppercase(), c.as_str()),
        None => String::new(),
    }
}

/// An adjective that restricts without naming a type.
fn state_prop(w: &str) -> Option<FilterProp> {
    Some(match w {
        "tapped" => FilterProp::Tapped,
        "untapped" => FilterProp::Untapped,
        "attacking" => FilterProp::Attacking { defender: None },
        "blocking" => FilterProp::Blocking,
        "unblocked" => FilterProp::Unblocked,
        "token" => FilterProp::Token,
        "nontoken" => FilterProp::NonToken,
        _ => return None,
    })
}

/// `you control` / `an opponent controls` / `target player controls`.
fn controller_clause(i: In<'_>) -> R<'_, Option<ControllerRef>> {
    const TABLE: &[(&str, ControllerRef)] = &[
        ("you control", ControllerRef::You),
        ("an opponent controls", ControllerRef::Opponent),
        ("your opponents control", ControllerRef::Opponent),
        ("target player controls", ControllerRef::TargetPlayer),
        ("target opponent controls", ControllerRef::TargetOpponent),
        ("that player controls", ControllerRef::TargetPlayer),
    ];
    match phrase_alt(TABLE)(i) {
        Ok((r, c)) => Ok((r, Some(c))),
        Err(_) => Ok((i, None)),
    }
}

/// `from your graveyard` / `in your hand`.
///
/// A possessive zone phrase constrains BOTH zone and controller — "your
/// graveyard" is not merely a zone. Both are returned so the caller does not
/// have to re-derive the ownership half.
fn zone_clause(i: In<'_>) -> R<'_, Option<(Zone, Option<ControllerRef>)>> {
    const TABLE: &[(&str, (Zone, Option<ControllerRef>))] = &[
        (
            "from your graveyard",
            (Zone::Graveyard, Some(ControllerRef::You)),
        ),
        (
            "in your graveyard",
            (Zone::Graveyard, Some(ControllerRef::You)),
        ),
        ("from your hand", (Zone::Hand, Some(ControllerRef::You))),
        ("in your hand", (Zone::Hand, Some(ControllerRef::You))),
        (
            "from your library",
            (Zone::Library, Some(ControllerRef::You)),
        ),
        ("in your library", (Zone::Library, Some(ControllerRef::You))),
        ("from a graveyard", (Zone::Graveyard, None)),
        ("in a graveyard", (Zone::Graveyard, None)),
        ("from exile", (Zone::Exile, None)),
        ("in exile", (Zone::Exile, None)),
    ];
    match phrase_alt(TABLE)(i) {
        Ok((r, z)) => Ok((r, Some(z))),
        Err(_) => Ok((i, None)),
    }
}

/// One typed object description, without its determiner.
///
/// `[adjective]* [subtype] <core type> [controller] [zone]`
fn typed_filter(i: In<'_>) -> R<'_, TypedFilter> {
    let mut i = i;
    let mut f = TypedFilter::default();

    // Leading adjectives. One loop over an open vocabulary, not one branch per
    // spelling: each recognizer is a total function from a word to a filter
    // part, so adding an axis never adds a branch here.
    loop {
        let Some(w) = i.first_word() else { break };
        if let Some(c) = color(&w) {
            f.properties.push(FilterProp::HasColor { color: c });
        } else if let Some(c) = non_color(&w) {
            f.properties.push(FilterProp::NotColor { color: c });
        } else if let Some(p) = state_prop(&w) {
            f.properties.push(p);
        } else if SUPERTYPES.contains(&w.as_str()) {
            f.properties.push(FilterProp::HasSupertype {
                value: capitalized(&w),
            });
        } else if non_body(&w).is_some_and(|b| SUPERTYPES.contains(&b)) {
            // "nonlegendary creature" — CR 205.4: a supertype is not a type, so
            // its negation is a property rather than a type-line conjunct.
            let body = non_body(&w).expect("checked");
            f.properties.push(FilterProp::NotSupertype {
                value: capitalized(body),
            });
        } else if let Some(t) = non_type(&w) {
            // "nonland permanent" — a negated type is a conjunct, not an adjective.
            f.type_filters.push(t);
        } else if non_body(&w).is_some()
            && i.take_from_n(1)
                .first_word()
                .is_some_and(|n| core_type(&n).is_some())
        {
            // "non-Human creature" — a negated SUBTYPE, recognized by position:
            // the next word is the core type, so this one names a subtype.
            let body = non_body(&w).expect("checked");
            f.type_filters
                .push(TypeFilter::Non(Box::new(TypeFilter::Subtype(capitalized(
                    body,
                )))));
        } else {
            break;
        }
        i = i.take_from_n(1);
    }

    // A word immediately before a core type word is a subtype: "Goblin creature".
    if let Some(w) = i.first_word() {
        if core_type(&w).is_none() {
            let next = i.take_from_n(1);
            if next.first_word().is_some_and(|n| core_type(&n).is_some()) {
                f.type_filters.push(TypeFilter::Subtype(capitalized(&w)));
                i = next;
            }
        }
    }

    // The core type itself. Required unless a negated type already stood in for
    // it ("target nonland permanent" has both; "nonland" alone does not).
    match i.first_word().and_then(|w| core_type(&w)) {
        Some(t) => {
            // CR 108.1: "card" names the OBJECT, not a type. "target creature
            // card" is a creature; "target Spirit card" is a Spirit. It is only
            // a type constraint of its own when nothing else constrains the
            // type line ("exile target card from a graveyard").
            if !(t == TypeFilter::Card && !f.type_filters.is_empty()) {
                f.type_filters.insert(0, t);
            }
            i = i.take_from_n(1);
        }
        None if !f.type_filters.is_empty() => {}
        None => return fail(i),
    }

    // A trailing "card" after a real type is the same object noun.
    if !f.type_filters.is_empty() {
        if let Some(w) = i.first_word() {
            if matches!(w.as_str(), "card" | "cards") {
                i = i.take_from_n(1);
            }
        }
    }

    let (r, ctrl) = controller_clause(i)?;
    i = r;
    f.controller = ctrl;

    let (r, zone) = zone_clause(i)?;
    i = r;
    if let Some((z, owner)) = zone {
        f.properties.push(FilterProp::InZone { zone: z });
        if let Some(o) = owner {
            f.controller = Some(o);
        }
    }

    Ok((i, f))
}

/// A list of object descriptions joined by "or": "artifact, creature, or land".
///
/// Lowered to `TargetFilter::Or`, one filter per alternative, which is the
/// engine's shape. Composed by iteration rather than by enumerating list
/// lengths, so a four-way list costs nothing extra.
fn typed_filter_list(i: In<'_>) -> R<'_, TargetFilter> {
    // CR 109.5: "another" is printed ONCE before the whole list and excludes the
    // source from every alternative — "sacrifice another creature or artifact"
    // means another of either. Stripping it here rather than inside
    // `typed_filter` is what makes the distribution automatic.
    let (i, another) = match word("another")(i) {
        Ok((r, _)) => (r, true),
        Err(_) => (i, false),
    };

    let (mut rest, first) = typed_filter(i)?;
    let mut parts = vec![TargetFilter::Typed(first)];

    loop {
        // ", " and ", or " and " or " are the three printed separators.
        let after_sep = match crate::prim::opt_kind(phase_oracle_lex::TokenKind::Comma)(rest) {
            Ok((r, _)) => r,
            Err(_) => rest,
        };
        let after_sep = match word("or")(after_sep) {
            Ok((r, _)) => r,
            Err(_) if after_sep != rest => after_sep,
            Err(_) => break,
        };
        match typed_filter(after_sep) {
            Ok((r, f)) => {
                parts.push(TargetFilter::Typed(f));
                rest = r;
            }
            Err(_) => break,
        }
    }

    let mut out = if parts.len() == 1 {
        parts.pop().expect("one element")
    } else {
        TargetFilter::Or { filters: parts }
    };
    if another {
        add_prop(&mut out, FilterProp::Another);
    }
    Ok((rest, out))
}

/// A player reference that is not an object. CR 102.1.
fn player_target(i: In<'_>) -> R<'_, Subject> {
    if let Ok((r, _)) = phrase("target player")(i) {
        return Ok((
            r,
            Subject {
                filter: TargetFilter::Player,
                scope: Scope::Single,
                targeted: true,
            },
        ));
    }
    if let Ok((r, _)) = phrase("target opponent")(i) {
        let f = TargetFilter::Typed(TypedFilter::player(ControllerRef::Opponent));
        return Ok((
            r,
            Subject {
                filter: f,
                scope: Scope::Single,
                targeted: true,
            },
        ));
    }
    // "an opponent" / "a player" — a player named without being targeted.
    if let Ok((r, _)) = phrase("an opponent")(i) {
        let f = TargetFilter::Typed(TypedFilter::player(ControllerRef::Opponent));
        return Ok((r, Subject::single(f)));
    }
    // An unrestricted player carries no controller constraint at all, so the
    // engine spells it `Player` rather than an empty typed filter.
    if let Ok((r, _)) = phrase("a player")(i) {
        return Ok((r, Subject::single(TargetFilter::Player)));
    }

    // "each opponent" / "each other player" / "each player" — a class of
    // players, which is a SCOPE rather than a target (CR 102.1 + CR 101.4).
    const PLAYER_CLASSES: &[(&str, ControllerRef)] = &[
        ("each opponent", ControllerRef::Opponent),
        ("each other player", ControllerRef::Opponent),
        ("each player", ControllerRef::EachPlayer),
    ];
    if let Ok((r, ctrl)) = phrase_alt(PLAYER_CLASSES)(i) {
        let f = TargetFilter::Typed(TypedFilter::player(ctrl));
        return Ok((
            r,
            Subject {
                filter: f,
                scope: Scope::All,
                targeted: false,
            },
        ));
    }

    // CR 603.2: "that player" names the player the trigger's event was about,
    // which is a back-reference and not a new choice.
    if let Ok((r, _)) = phrase("that player")(i) {
        return Ok((r, Subject::single(TargetFilter::TriggeringPlayer)));
    }
    if let Ok((r, _)) = word("you")(i) {
        return Ok((r, Subject::single(TargetFilter::Controller)));
    }
    fail(i)
}

/// The full subject grammar.
pub fn subject(i: In<'_>) -> R<'_, Subject> {
    // CR 115.4: "any target" is creature, player, planeswalker or battle.
    if let Ok((r, _)) = phrase("any target")(i) {
        return Ok((
            r,
            Subject {
                filter: TargetFilter::Any,
                scope: Scope::Single,
                targeted: true,
            },
        ));
    }
    if let Ok((r, s)) = player_target(i) {
        return Ok((r, s));
    }
    if let Ok((r, _)) = self_ref(i) {
        return Ok((r, Subject::single(TargetFilter::SelfRef)));
    }

    // "enchanted creature" / "equipped creature" — the engine spells the host
    // as a typed filter carrying an attachment property, NOT as `AttachedTo`.
    // Reading it as a filter is what lets an Aura's static ability name the
    // same object shape every other filter uses.
    const ATTACHED: &[(&str, FilterProp)] = &[
        ("enchanted creature", FilterProp::EnchantedBy),
        ("enchanted permanent", FilterProp::EnchantedBy),
        ("enchanted artifact", FilterProp::EnchantedBy),
        ("enchanted land", FilterProp::EnchantedBy),
        ("equipped creature", FilterProp::EquippedBy),
    ];
    for (p, prop) in ATTACHED {
        if let Ok((r, _)) = crate::prim::phrase_static(p)(i) {
            let noun = p.rsplit(' ').next().expect("two words");
            let mut t = TypedFilter::of(core_type(noun).expect("known type"));
            t.properties.push(prop.clone());
            return Ok((r, Subject::single(TargetFilter::Typed(t))));
        }
    }

    // "target <object>" — chosen on announcement (CR 601.2c).
    //
    // "another" is printed BEFORE "target" ("return another target creature you
    // control"), so it is stripped here and re-attached to the filter. Leaving
    // it to the noun grammar would make "target" itself look like a subtype,
    // because the word after it is the core type.
    let (i, another) = match word("another")(i) {
        Ok((r, _)) if word("target")(r).is_ok() => (r, true),
        _ => (i, false),
    };
    if let Ok((r, _)) = word("target")(i) {
        let (r, mut f) = typed_filter_list(r)?;
        if another {
            add_prop(&mut f, FilterProp::Another);
        }
        return Ok((
            r,
            Subject {
                filter: f,
                scope: Scope::Single,
                targeted: true,
            },
        ));
    }

    // "each"/"all <object>" — a class, not a chosen target.
    if let Ok((r, _)) = any_of(&["each", "all"])(i) {
        if let Ok((r2, f)) = typed_filter_list(r) {
            return Ok((
                r2,
                Subject {
                    filter: f,
                    scope: Scope::All,
                    targeted: false,
                },
            ));
        }
    }

    // A bare determiner: "a creature you control", "the creature".
    if let Ok((r, _)) = any_of(&["a", "an", "the"])(i) {
        if let Ok((r2, f)) = typed_filter_list(r) {
            return Ok((r2, Subject::single(f)));
        }
    }

    // A bare noun phrase with no determiner at all: "another creature",
    // "creatures you control". Last, so a determiner-led phrase never reaches
    // it, and safe because `typed_filter` still requires a real type word.
    if let Ok((r, f)) = typed_filter_list(i) {
        // A plural bare noun names a class; a singular one names an instance.
        let scope = if is_plural_head(i) {
            Scope::All
        } else {
            Scope::Single
        };
        return Ok((
            r,
            Subject {
                filter: f,
                scope,
                targeted: false,
            },
        ));
    }

    fail(i)
}

/// Attach a property to every branch of a filter.
///
/// A disjunction distributes the property over its alternatives, because
/// "another target artifact or creature" means another of either.
fn add_prop(f: &mut TargetFilter, p: FilterProp) {
    match f {
        // Appended, not prepended: the engine prints `Another` after the
        // adjectives it shares a noun phrase with ("nontoken", "attacking").
        TargetFilter::Typed(t) => t.properties.push(p),
        TargetFilter::Or { filters } => {
            for inner in filters {
                add_prop(inner, p.clone());
            }
        }
        _ => {}
    }
}

/// Whether the noun phrase starting here is printed in the plural.
///
/// Magic uses plurality to mark scope: "creatures you control" is every one of
/// them, "a creature you control" is one. Reading it off the printed noun is
/// what lets a bare noun phrase carry scope without a determiner to say so.
fn is_plural_head(i: In<'_>) -> bool {
    let mut cur = i;
    for _ in 0..6 {
        let Some(w) = cur.first_word() else {
            return false;
        };
        if let Some(t) = core_type(&w) {
            let _ = t;
            return w.ends_with('s') && w != "sorceries" || w == "sorceries";
        }
        cur = cur.take_from_n(1);
    }
    false
}

/// The subject grammar, for positions where a target must be chosen.
pub fn target(i: In<'_>) -> R<'_, Subject> {
    subject(i)
}
