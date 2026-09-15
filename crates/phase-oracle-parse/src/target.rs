//! What a clause acts on.
//!
//! The grammar builds a [`Subject`]: a filter plus the SCOPE the printed text
//! gave it. Scope is carried on the subject rather than baked into a verb,
//! which is what lets one `destroy` production serve both "destroy target
//! creature" and "destroy all creatures". The engine names scope in the effect
//! variant instead (`Destroy` / `DestroyAll`); that translation happens once,
//! at emission, instead of doubling every verb production here.

use phase_oracle_ast::{
    AttachmentKind, Comparator, ControllerRef, FilterProp, ManaColor, PtScope, PtStat, Quantity,
    TargetFilter, TypeFilter, TypedFilter, Zone,
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

/// "enchanted" / "equipped", read against the noun that follows.
///
/// The SINGULAR form names this source's own host ("Enchanted creature gets
/// +1/+2" on an Aura); the PLURAL names any object carrying an attachment of
/// that kind ("Enchanted creatures you control get +2/+2"). Those are different
/// predicates and the engine spells them differently, so the number is read
/// rather than ignored.
fn attachment_prop(w: &str, next: In<'_>) -> Option<FilterProp> {
    let kind = match w {
        "enchanted" => AttachmentKind::Aura,
        "equipped" => AttachmentKind::Equipment,
        _ => return None,
    };
    let plural = next
        .first_word()
        .is_some_and(|n| core_type(&n).is_some() && n.ends_with('s'));
    Some(if plural {
        FilterProp::HasAttachment {
            kind,
            controller: None,
        }
    } else if kind == AttachmentKind::Aura {
        FilterProp::EnchantedBy
    } else {
        FilterProp::EquippedBy
    })
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
        "commander" => FilterProp::IsCommander,
        "multicolored" => FilterProp::ColorCount {
            comparator: Comparator::GE,
            count: 2,
        },
        "face-down" => FilterProp::FaceDown,
        _ => return None,
    })
}

/// `you control` / `an opponent controls` / `target player controls`.
fn controller_clause(i: In<'_>) -> R<'_, Option<ControllerRef>> {
    const TABLE: &[(&str, ControllerRef)] = &[
        // The negation comes FIRST: "you don't control" starts with the same
        // two words as "you control", so matching the shorter phrase first
        // would claim the negation and leave "don't" stranded.
        ("you don't control", ControllerRef::Opponent),
        ("you dont control", ControllerRef::Opponent),
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
        if let Some(prop) = attachment_prop(&w, i.take_from_n(1)) {
            f.properties.push(prop);
        } else if let Some(c) = color(&w) {
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

    // A word immediately before a core type word is a subtype: "Goblin
    // creature". POSITION alone is not enough — "that creature" and "another
    // creature" have the same shape — so the candidate must also be a real
    // subtype (CR 205.3). Before that check, "destroy that creature" parsed as
    // a creature of subtype "That".
    if let Some(w) = i.first_word() {
        if core_type(&w).is_none() {
            let next = i.take_from_n(1);
            let followed_by_type = next.first_word().is_some_and(|n| core_type(&n).is_some());
            if let (true, Some(name)) = (followed_by_type, crate::subtypes::singular_of(&w)) {
                f.type_filters.push(TypeFilter::Subtype(name.to_string()));
                i = next;
            }
        }
    }

    // A bare SUBTYPE with no core type after it: "Destroy all Forests",
    // "target Goblins". CR 205.3 subtypes are printed capitalized and ordinary
    // nouns are not, so capitalization in the source is the signal — which is
    // why this is read from the original text rather than the lowercased word.
    if f.type_filters.is_empty() {
        if let Some(w) = i.first_word() {
            let capitalized_in_source = i
                .first()
                .map(|t| t.text(i.src))
                .is_some_and(|s| s.chars().next().is_some_and(char::is_uppercase));
            let named = (core_type(&w).is_none() && capitalized_in_source)
                .then(|| crate::subtypes::singular_of(&w))
                .flatten();
            if let Some(name) = named {
                // The bare subtype stands alone. The engine does NOT spell out
                // the implied type line here: "all Zombies" is
                // `[Subtype(Zombie)]`, not `[Creature, Subtype(Zombie)]`.
                f.type_filters.push(TypeFilter::Subtype(name.to_string()));
                i = i.take_from_n(1);

                while let Ok((r, prop)) = relative_clause(i) {
                    f.properties.push(prop);
                    i = r;
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
                return Ok((i, f));
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

    // A relative clause is read BEFORE the zone, because that is the printed
    // order: "target creature card WITH FLYING from your graveyard".
    while let Ok((r, prop)) = relative_clause(i) {
        f.properties.push(prop);
        i = r;
    }

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

/// `with <keyword>` / `without <keyword>` / `with mana value N or less` /
/// `with power N or greater`.
///
/// CR 205 + CR 208: a trailing relative clause restricts the noun it follows.
/// This is the production whose ABSENCE the totality rule made visible —
/// "destroy target creature with mana value 3 or less" declined rather than
/// silently widening to every creature, which is the 738-card class the
/// existing parser's post-hoc auditor cannot see.
fn relative_clause(i: In<'_>) -> R<'_, FilterProp> {
    if let Ok((r, _)) = word("with")(i) {
        // "with mana value N or less" — CR 202.3.
        if let Ok((r2, _)) = phrase("mana value")(r) {
            let (r3, (cmp, n)) = comparison(r2)?;
            return Ok((
                r3,
                FilterProp::Cmc {
                    comparator: cmp,
                    value: Quantity::fixed(n),
                },
            ));
        }
        // "with power N or greater" / "with toughness N or less".
        if let Ok((r2, stat)) =
            phrase_alt(&[("power", PtStat::Power), ("toughness", PtStat::Toughness)])(r)
        {
            let (r3, (cmp, n)) = comparison(r2)?;
            return Ok((
                r3,
                FilterProp::PtComparison {
                    stat,
                    scope: PtScope::Current,
                    comparator: cmp,
                    value: Quantity::fixed(n),
                },
            ));
        }
        let (r2, (kw, _printed)) = crate::effect::grantable_keyword(r)?;
        return Ok((r2, FilterProp::WithKeyword { value: kw }));
    }
    if let Ok((r, _)) = word("without")(i) {
        let (r2, (kw, _printed)) = crate::effect::grantable_keyword(r)?;
        return Ok((r2, FilterProp::WithoutKeyword { value: kw }));
    }
    fail(i)
}

/// `N or less` / `N or greater` / `N`.
///
/// Magic prints the bound before the direction, so the number is read first and
/// the comparator second — the reverse of how it reads in code.
fn comparison(i: In<'_>) -> R<'_, (Comparator, i32)> {
    let (r, n) = crate::prim::number(i)?;
    const TABLE: &[(&str, Comparator)] = &[
        ("or less", Comparator::LE),
        ("or greater", Comparator::GE),
        ("or more", Comparator::GE),
    ];
    match phrase_alt(TABLE)(r) {
        Ok((r2, cmp)) => Ok((r2, (cmp, n))),
        Err(_) => Ok((r, (Comparator::EQ, n))),
    }
}

/// A list of object descriptions joined by "or": "artifact, creature, or land".
///
/// Lowered to `TargetFilter::Or`, one filter per alternative, which is the
/// engine's shape. Composed by iteration rather than by enumerating list
/// lengths, so a four-way list costs nothing extra.
pub fn typed_filter_list(i: In<'_>) -> R<'_, TargetFilter> {
    // CR 109.5: "another" is printed ONCE before the whole list and excludes the
    // source from every alternative — "sacrifice another creature or artifact"
    // means another of either. Stripping it here rather than inside
    // `typed_filter` is what makes the distribution automatic.
    // CR 109.5: "another" and "other" are the same exclusion, printed with the
    // singular and plural noun respectively ("another creature" / "other
    // creatures"), so one flag serves both.
    let (i, another) = match any_of(&["another", "other"])(i) {
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

    // A controller or zone printed after the LAST alternative qualifies the
    // whole list: "target instant or sorcery card from your graveyard" means
    // both from the graveyard, not just the sorcery.
    propagate_trailing_qualifier(&mut parts);

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

/// Copy the last alternative's controller and zone onto the earlier ones.
///
/// English attaches a trailing qualifier to the whole coordination, but the
/// grammar necessarily reads it while parsing the final branch. Doing this once
/// here is what keeps every alternative's filter honest without the branch
/// parser needing lookahead.
fn propagate_trailing_qualifier(parts: &mut [TargetFilter]) {
    let Some(TargetFilter::Typed(last)) = parts.last() else {
        return;
    };
    let controller = last.controller;
    // Only a ZONE and a MANA VALUE distribute. Both are properties of the card
    // itself, so they bound the whole coordination: "target instant or sorcery
    // card from your graveyard", "target creature or planeswalker with mana
    // value 3 or less".
    //
    // A keyword or a power comparison does NOT distribute, and trying it made
    // things measurably worse: "artifact or creature with flying" restricts
    // only the creature, because only a creature can have flying. Which
    // qualifiers reach back over a coordination is a semantic question, not a
    // syntactic one.
    let zone: Vec<FilterProp> = last
        .properties
        .iter()
        .filter(|p| matches!(p, FilterProp::InZone { .. } | FilterProp::Cmc { .. }))
        .cloned()
        .collect();
    if controller.is_none() && zone.is_empty() {
        return;
    }
    let count = parts.len();
    for part in parts.iter_mut().take(count.saturating_sub(1)) {
        if let TargetFilter::Typed(t) = part {
            if t.controller.is_none() {
                t.controller = controller;
            }
            for p in &zone {
                if !t.properties.contains(p) {
                    t.properties.push(p.clone());
                }
            }
        }
    }
}

/// An object on the stack: "spell", or "<types> spell".
///
/// CR 111.1: a spell is a zone-dependent object, not a card type, so it has no
/// type-line spelling. The engine names it `StackSpell` and conjoins any type
/// restriction with `And` rather than putting "spell" in `type_filters`.
fn spell_on_the_stack(i: In<'_>) -> R<'_, TargetFilter> {
    if let Ok((r, _)) = any_of(&["spell", "spells"])(i) {
        return Ok((r, TargetFilter::StackSpell));
    }
    // "<type list> spell" — the types restrict WHAT KIND of spell.
    //
    // The conjunction distributes INTO each alternative rather than wrapping
    // the disjunction: "artifact or enchantment spell" is (spell AND artifact)
    // or (spell AND enchantment). Either evaluates the same, but the engine's
    // shape is the distributed one and this is a like-for-like replacement.
    let (r, types) = typed_filter_list(i)?;
    let (r, _) = any_of(&["spell", "spells"])(r)?;
    let with_stack = |f: TargetFilter| TargetFilter::And {
        filters: vec![TargetFilter::StackSpell, f],
    };
    Ok((
        r,
        match types {
            TargetFilter::Or { filters } => TargetFilter::Or {
                filters: filters.into_iter().map(with_stack).collect(),
            },
            other => with_stack(other),
        },
    ))
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
    // A bare pronoun is NOT resolved here. "It" means the chosen target after
    // "Untap target creature", and the SOURCE after "Whenever this creature
    // attacks" — the same word, two referents, decided by whether an earlier
    // clause of the same ability chose a target. That context does not reach
    // this production, so resolving it here would be a guess; the clause
    // declines instead. Reading it is a real piece of work, not an oversight.
    //
    // It is still matched and rejected explicitly, because a sentence-initial
    // "It" is capitalized and would otherwise be read as a bare subtype.
    if crate::prim::any_of(&["it", "they", "them"])(i).is_ok() {
        return fail(i);
    }
    // "that creature" / "that permanent" refer back to the object an earlier
    // clause of the same ability chose (CR 601.2c) — but ONLY in a spell body.
    // Inside a trigger the same words mean the object the EVENT was about, and
    // the engine spells that three different ways (`TriggeringSource`,
    // `EventTarget`, `ParentTarget`) depending on the event. Picking one would
    // be a guess, so a trigger body declines here and the gap stays visible.
    const BACK_REFS: &[(&str, ())] = &[
        ("that creature", ()),
        ("that permanent", ()),
        ("that artifact", ()),
        ("that enchantment", ()),
        ("that land", ()),
        ("that card", ()),
    ];
    if let Ok((r, _)) = phrase_alt(BACK_REFS)(i) {
        if i.ctx.in_trigger {
            return fail(i);
        }
        return Ok((r, Subject::single(TargetFilter::ParentTarget)));
    }
    // CR 201.5: "this card" is a self-reference the engine deliberately does
    // NOT normalize to `~`, because it is context-dependent — but in subject
    // position it is still the source.
    if let Ok((r, _)) = phrase("this card")(i) {
        return Ok((r, Subject::single(TargetFilter::SelfRef)));
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
        if let Ok((r2, f)) = spell_on_the_stack(r) {
            return Ok((
                r2,
                Subject {
                    filter: f,
                    scope: Scope::Single,
                    targeted: true,
                },
            ));
        }
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
        // Position follows the PRINTED order. "another" is printed before the
        // noun, so it lands after the adjectives that share the noun phrase
        // ("nontoken", "attacking") but before a zone clause that trails it
        // ("another target creature card FROM YOUR GRAVEYARD").
        TargetFilter::Typed(t) => {
            let at = t
                .properties
                .iter()
                .position(|q| matches!(q, FilterProp::InZone { .. }))
                .unwrap_or(t.properties.len());
            t.properties.insert(at, p);
        }
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

/// A token's printed body: colours, subtypes and core types, in that order.
///
/// CR 111.1 + CR 205: the token templating prints "1/1 white Soldier creature
/// token", so the colour precedes the subtype and the subtype precedes the core
/// type. The engine's `types` list is core types first and then subtypes, which
/// is the reverse of the printed order for the subtype half — hence one place
/// that reorders, rather than every caller knowing.
/// Tokens whose definition supplies the Artifact type the sentence omits.
const PREDEFINED_ARTIFACT_TOKENS: &[&str] = &[
    "Treasure",
    "Food",
    "Powerstone",
    "Blood",
    "Clue",
    "Lander",
    "Map",
    "Junk",
    "Gold",
    "Shard",
];

pub struct TokenBody {
    pub name: String,
    pub types: Vec<String>,
    pub colors: Vec<ManaColor>,
    /// "create a TAPPED Treasure token" — the same flag a trailing "that are
    /// tapped" clause sets, printed inside the body instead.
    pub tapped: bool,
}

pub fn token_body(i: In<'_>) -> R<'_, TokenBody> {
    let mut i = i;
    let mut tapped = false;
    let mut colors = Vec::new();
    let mut subtypes: Vec<String> = Vec::new();
    let mut core: Vec<String> = Vec::new();

    loop {
        let Some(w) = i.first_word() else { break };
        if matches!(w.as_str(), "token" | "tokens") {
            break;
        }
        if w == "tapped" {
            tapped = true;
        } else if let Some(c) = color(&w) {
            colors.push(c);
        } else if w == "and" && !colors.is_empty() {
            // "3/3 blue and red Elemental" — the conjunction joins colours.
        } else if let Some(t) = core_type(&w) {
            // Only the permanent types appear in a token's type line.
            let name = match t {
                TypeFilter::Creature => "Creature",
                TypeFilter::Artifact => "Artifact",
                TypeFilter::Enchantment => "Enchantment",
                TypeFilter::Land => "Land",
                TypeFilter::Planeswalker => "Planeswalker",
                _ => break,
            };
            core.push(name.to_string());
        } else if w == "colorless" {
            // An explicit absence of colour, which the engine records as an
            // empty list rather than as a marker.
        } else if w.chars().next().is_some_and(|c| c.is_alphabetic()) {
            subtypes.push(capitalized(&w));
        } else {
            break;
        }
        i = i.take_from_n(1);
    }

    if core.is_empty() && subtypes.is_empty() {
        return fail(i);
    }

    // CR 111.9: a predefined token's card type is not printed — "create a
    // Treasure token" names only the subtype, and the Artifact type is implied
    // by the token's definition rather than by the sentence.
    if core.is_empty()
        && subtypes
            .iter()
            .all(|s| PREDEFINED_ARTIFACT_TOKENS.contains(&s.as_str()))
    {
        core.push("Artifact".to_string());
    }

    // The token's name is its last printed subtype ("Phyrexian Wurm" keeps
    // both words, so the whole subtype run is the name).
    let name = if subtypes.is_empty() {
        core.first().cloned().unwrap_or_default()
    } else {
        subtypes.join(" ")
    };

    let mut types = core;
    types.extend(subtypes);
    Ok((
        i,
        TokenBody {
            name,
            types,
            colors,
            tapped,
        },
    ))
}
