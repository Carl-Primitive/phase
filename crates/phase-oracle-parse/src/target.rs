//! Productions for what a clause acts on.

use phase_card_schema::{CardZone, Controller, ObjectFilter, Target};

use crate::prim::{fail, phrase, word, In, R};

/// Printed card types this slice recognizes. Subtypes are accepted positionally
/// (the word immediately before a type word), so new sets do not need a list edit.
const TYPE_WORDS: &[&str] = &[
    "creature", "creatures", "permanent", "permanents", "artifact", "artifacts",
    "enchantment", "enchantments", "land", "lands", "planeswalker", "planeswalkers",
    "spell", "spells", "card", "cards", "token", "tokens", "battle", "battles",
    "instant", "instants", "sorcery", "sorceries",
];

const COLOR_WORDS: &[&str] =
    &["white", "blue", "black", "red", "green", "colorless", "multicolored"];

/// Adjectives that restrict without naming a type.
const STATE_WORDS: &[&str] = &[
    "tapped", "untapped", "attacking", "blocking", "blocked", "unblocked",
    "nonland", "nontoken", "nonlegendary", "legendary", "basic",
];

fn singular(w: &str) -> &str {
    match w {
        "creatures" => "creature",
        "permanents" => "permanent",
        "artifacts" => "artifact",
        "enchantments" => "enchantment",
        "lands" => "land",
        "planeswalkers" => "planeswalker",
        "spells" => "spell",
        "cards" => "card",
        "tokens" => "token",
        "battles" => "battle",
        "instants" => "instant",
        "sorceries" => "sorcery",
        other => other,
    }
}

fn article(i: In<'_>) -> R<'_, ()> {
    for a in ["a", "an", "the"] {
        if let Some(w) = i.first_word() {
            if w == a {
                return Ok((i.take_from_n(1), ()));
            }
        }
    }
    Ok((i, ()))
}

fn controller_clause(i: In<'_>) -> R<'_, Option<Controller>> {
    if let Ok((r, _)) = phrase("you control")(i) {
        return Ok((r, Some(Controller::You)));
    }
    if let Ok((r, _)) = phrase("an opponent controls")(i) {
        return Ok((r, Some(Controller::Opponent)));
    }
    if let Ok((r, _)) = phrase("your opponents control")(i) {
        return Ok((r, Some(Controller::Opponent)));
    }
    Ok((i, None))
}

fn zone_clause(i: In<'_>) -> R<'_, Option<CardZone>> {
    for (p, z) in [
        ("in your graveyard", CardZone::Graveyard),
        ("from your graveyard", CardZone::Graveyard),
        ("in your hand", CardZone::Hand),
        ("from your hand", CardZone::Hand),
        ("in exile", CardZone::Exile),
    ] {
        if let Ok((r, _)) = phrase(p)(i) {
            return Ok((r, Some(z)));
        }
    }
    Ok((i, None))
}

/// `[article] [another] [colors] [states] [subtype] <type> [controller] [zone]`
pub fn object_filter(i: In<'_>) -> R<'_, ObjectFilter> {
    let (mut i, _) = article(i)?;
    let mut f = ObjectFilter::default();

    if let Ok((r, _)) = word("another")(i) {
        f.excludes_source = true;
        i = r;
    }

    loop {
        let Some(w) = i.first_word() else { break };
        if COLOR_WORDS.contains(&w.as_str()) {
            f.colors.push(w);
            i = i.take_from_n(1);
            continue;
        }
        if STATE_WORDS.contains(&w.as_str()) {
            f.subtypes.push(w);
            i = i.take_from_n(1);
            continue;
        }
        break;
    }

    // A word immediately before a type word is a subtype ("Goblin creature").
    if let Some(w) = i.first_word() {
        if !TYPE_WORDS.contains(&w.as_str()) {
            let next = i.take_from_n(1);
            if next.first_word().is_some_and(|n| TYPE_WORDS.contains(&n.as_str())) {
                f.subtypes.push(w);
                i = next;
            }
        }
    }

    let Some(w) = i.first_word() else { return fail(i) };
    if !TYPE_WORDS.contains(&w.as_str()) {
        return fail(i);
    }
    f.types.push(singular(&w).to_string());
    i = i.take_from_n(1);

    let (i, ctrl) = controller_clause(i)?;
    f.controller = ctrl;
    let (i, zone) = zone_clause(i)?;
    f.zone = zone;
    Ok((i, f))
}

/// The full target grammar.
pub fn target(i: In<'_>) -> R<'_, Target> {
    if let Ok((r, _)) = phrase("any target")(i) {
        return Ok((r, Target::AnyTarget));
    }
    if let Ok((r, _)) = phrase("each opponent")(i) {
        return Ok((r, Target::EachOpponent));
    }
    if let Ok((r, _)) = phrase("target opponent")(i) {
        return Ok((r, Target::Player { controller: Controller::Opponent, chosen: true }));
    }
    if let Ok((r, _)) = phrase("target player")(i) {
        return Ok((r, Target::Player { controller: Controller::Any, chosen: true }));
    }
    if let Ok((r, _)) = phrase("each player")(i) {
        return Ok((r, Target::Player { controller: Controller::Any, chosen: false }));
    }
    if let Ok((r, _)) = word("target")(i) {
        let (r, f) = object_filter(r)?;
        return Ok((r, Target::Chosen { filter: f }));
    }
    for w in ["each", "all"] {
        if let Ok((r, _)) = word(w)(i) {
            if let Ok((r2, f)) = object_filter(r) {
                return Ok((r2, Target::Each { filter: f }));
            }
        }
    }
    for p in ["enchanted creature", "enchanted permanent", "equipped creature"] {
        if let Ok((r, _)) = phrase(p)(i) {
            return Ok((r, Target::Attached));
        }
    }
    // The card's own name, normalized to a single CARDNAME token upstream.
    if let Ok((r, _)) = word("cardname")(i) {
        return Ok((r, Target::This));
    }
    for p in ["this creature", "this permanent", "this artifact", "this enchantment", "this land"] {
        if let Ok((r, _)) = phrase(p)(i) {
            return Ok((r, Target::This));
        }
    }
    if let Ok((r, _)) = word("you")(i) {
        return Ok((r, Target::You));
    }
    fail(i)
}
