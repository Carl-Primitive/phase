//! Self-reference normalization: every printing of the card's own name becomes
//! a single `~`.
//!
//! Run BEFORE the lexer, so the grammar matches ONE token instead of a
//! name-shaped phrase that every production would have to re-recognize. `~` is
//! also the spelling the engine prints in an ability's `description`, so a
//! description is just a slice of the normalized text and needs no second pass.
//!
//! Mirrors `phase_engine::parser::oracle_util::normalize_card_name_refs` for
//! the strategies that carry corpus mass. The engine's further fallbacks
//! (Alchemy `A-` prefixes, quoted-grant masking, "X of Y" prefixes, the
//! progressive first-words fallback) are not reproduced; a card that needs one
//! shows up as a description mismatch in the corpus diff, which is exactly
//! where an unreproduced strategy should surface.

/// CR 201.5: phrases that refer to the ability's own source regardless of the
/// card's name. Copied verbatim from the engine's `SELF_REF_TYPE_PHRASES`.
///
/// "this spell" and "this card" are deliberately ABSENT: the engine keeps them
/// in a separate parse-only list because they are context-dependent and not
/// safe to normalize.
const SELF_REF_TYPE_PHRASES: &[&str] = &[
    "this creature",
    "this permanent",
    "this artifact",
    "this land",
    "this enchantment",
    "this attraction",
    "this equipment",
    "this aura",
    "this vehicle",
    "this planeswalker",
    "this emblem",
    "this battle",
    "this token",
    "this spacecraft",
    "this saga",
    "this class",
    "this case",
    "this room",
];

/// Replace whole-word occurrences of `needle`, comparing case-insensitively.
///
/// "Whole word" is bounded by ASCII alphanumerics on both sides, so
/// "Ancestor's" does not match inside "Ancestor's Prophet" as a prefix of a
/// longer word, while "Bolt." still matches before the period.
fn replace_all_words(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() || needle.len() > haystack.len() {
        return haystack.to_string();
    }
    let lower_haystack = haystack.to_lowercase();
    let lower_needle = needle.to_lowercase();
    // A lowercase fold can change byte length (e.g. `İ`), which would make
    // positions found in the folded string invalid in the original.
    if lower_haystack.len() != haystack.len() || lower_needle.len() != needle.len() {
        return haystack.to_string();
    }

    let mut result = String::with_capacity(haystack.len());
    let mut last_end = 0usize;
    for (pos, _) in lower_haystack.match_indices(&lower_needle) {
        let end = pos + needle.len();
        if pos < last_end {
            continue;
        }
        let at_start = pos == 0 || !haystack.as_bytes()[pos - 1].is_ascii_alphanumeric();
        let at_end = end == haystack.len() || !haystack.as_bytes()[end].is_ascii_alphanumeric();
        if at_start && at_end {
            result.push_str(&haystack[last_end..pos]);
            result.push_str(replacement);
            last_end = end;
        }
    }
    if last_end == 0 {
        return haystack.to_string();
    }
    result.push_str(&haystack[last_end..]);
    result
}

/// The pre-comma short name Magic uses after a legendary card's first mention:
/// "Jace, the Mind Sculptor" is printed "Jace" thereafter.
fn comma_short_name(card_name: &str) -> Option<&str> {
    let short = card_name.split_once(", ")?.0;
    (short.len() >= 2).then_some(short)
}

/// Rewrite `oracle` so every self-reference is the single token `~`.
pub fn normalize(card_name: &str, oracle: &str) -> String {
    // Alchemy rebalanced cards print the unprefixed name in their text.
    let name = card_name.strip_prefix("A-").unwrap_or(card_name);

    let mut out = replace_all_words(oracle, name, "~");
    if let Some(short) = comma_short_name(name) {
        out = replace_all_words(&out, short, "~");
    }
    for phrase in SELF_REF_TYPE_PHRASES {
        out = replace_all_words(&out, phrase, "~");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_the_full_printed_name() {
        assert_eq!(
            normalize("Shock", "Shock deals 2 damage to any target."),
            "~ deals 2 damage to any target."
        );
    }

    #[test]
    fn replaces_the_pre_comma_short_name_too() {
        assert_eq!(
            normalize(
                "Jace, the Mind Sculptor",
                "Jace, the Mind Sculptor is great. Jace draws."
            ),
            "~ is great. ~ draws."
        );
    }

    #[test]
    fn replaces_generic_self_reference_phrases() {
        assert_eq!(
            normalize(
                "Prodigal Sorcerer",
                "{T}: This creature deals 1 damage to any target."
            ),
            "{T}: ~ deals 1 damage to any target."
        );
    }

    #[test]
    fn leaves_this_spell_and_this_card_alone() {
        // The engine keeps these off the normalization list on purpose: they are
        // context-dependent and `~` would erase the distinction.
        let t = "Counter this spell unless you pay {2}. Exile this card.";
        assert_eq!(normalize("Whatever", t), t);
    }

    #[test]
    fn respects_word_boundaries() {
        // "Bolt" must not match inside "Bolted".
        assert_eq!(
            normalize("Bolt", "Bolted creatures and Bolt."),
            "Bolted creatures and ~."
        );
    }

    #[test]
    fn a_name_that_never_appears_changes_nothing() {
        let t = "Draw a card.";
        assert_eq!(normalize("Divination", t), t);
    }
}
