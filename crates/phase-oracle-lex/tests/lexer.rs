//! Lexer behaviour, pinned at the token level rather than per card.
//!
//! Every case here is a shape the corpus census turned up, cited with its
//! occurrence count, so the suite tests the vocabulary and not one printing.

use phase_oracle_lex::{lex, verify_coverage, PtPart, Sign, TokenKind};

fn kinds(src: &str) -> Vec<TokenKind> {
    lex(src).into_iter().map(|t| t.kind).collect()
}

/// Coverage is the invariant the grammar above the lexer depends on.
fn assert_total(src: &str) {
    let tokens = lex(src);
    assert_eq!(
        verify_coverage(src, &tokens),
        Ok(()),
        "lexer left bytes unclaimed in {src:?}"
    );
}

#[test]
fn words_keep_internal_apostrophes_and_hyphens() {
    let src = "opponent's can't six-sided opponents'";
    assert_eq!(kinds(src), vec![TokenKind::Word; 4]);
    let texts: Vec<_> = lex(src).iter().map(|t| t.text(src).to_string()).collect();
    assert_eq!(texts, ["opponent's", "can't", "six-sided", "opponents'"]);
    assert_total(src);
}

/// A trailing hyphen does not bind, because nothing follows it to bind to.
#[test]
fn trailing_hyphen_is_its_own_token() {
    let src = "Sliver- ";
    assert_eq!(kinds(src), vec![TokenKind::Word, TokenKind::Hyphen]);
    assert_total(src);
}

#[test]
fn mana_symbols_lex_whole_including_hybrids() {
    let src = "{T}: Add {W/U} or {2/B}.";
    let toks = lex(src);
    let syms: Vec<_> = toks
        .iter()
        .filter(|t| t.kind == TokenKind::Symbol)
        .map(|t| t.text(src).to_string())
        .collect();
    assert_eq!(syms, ["{T}", "{W/U}", "{2/B}"]);
    assert_total(src);
}

/// CR prints loyalty minus as U+2212, not U+002D. Both must normalize.
#[test]
fn loyalty_accepts_both_minus_spellings() {
    for src in ["[\u{2212}3]", "[-3]"] {
        assert_eq!(
            kinds(src),
            vec![TokenKind::Loyalty {
                cost: PtPart::Number { sign: Sign::Minus, value: 3 }
            }],
            "failed for {src:?}"
        );
        assert_total(src);
    }
    assert_eq!(
        kinds("[+1]"),
        vec![TokenKind::Loyalty {
            cost: PtPart::Number { sign: Sign::Plus, value: 1 }
        }]
    );
    assert_eq!(
        kinds("[0]"),
        vec![TokenKind::Loyalty {
            cost: PtPart::Number { sign: Sign::None, value: 0 }
        }]
    );
    assert_eq!(
        kinds("[\u{2212}X]"),
        vec![TokenKind::Loyalty { cost: PtPart::Variable { sign: Sign::Minus } }]
    );
}

/// Bracket spans that are not loyalty (playtest placeholders, 33 in corpus)
/// must still lex totally rather than being swallowed or panicking.
#[test]
fn non_loyalty_brackets_stay_total() {
    let src = "[CHOICE A] and [with mana value 2 or less]";
    assert_total(src);
    assert!(
        !lex(src).iter().any(|t| matches!(t.kind, TokenKind::Loyalty { .. })),
        "placeholder brackets must not be read as loyalty costs"
    );
}

#[test]
fn pt_pairs_cover_signed_bare_variable_and_star() {
    let cases = [
        ("+2/+1", PtPart::Number { sign: Sign::Plus, value: 2 }, PtPart::Number { sign: Sign::Plus, value: 1 }),
        ("-1/-1", PtPart::Number { sign: Sign::Minus, value: 1 }, PtPart::Number { sign: Sign::Minus, value: 1 }),
        ("2/2", PtPart::Number { sign: Sign::None, value: 2 }, PtPart::Number { sign: Sign::None, value: 2 }),
        ("+X/+X", PtPart::Variable { sign: Sign::Plus }, PtPart::Variable { sign: Sign::Plus }),
        ("*/*", PtPart::Star { sign: Sign::None }, PtPart::Star { sign: Sign::None }),
    ];
    for (src, power, toughness) in cases {
        assert_eq!(kinds(src), vec![TokenKind::PtPair { power, toughness }], "failed for {src:?}");
        assert_total(src);
    }
}

/// A pair must not absorb a following alphanumeric.
#[test]
fn pt_pair_declines_when_followed_by_alphanumeric() {
    let src = "2/2x";
    assert!(
        !lex(src).iter().any(|t| matches!(t.kind, TokenKind::PtPair { .. })),
        "`2/2x` is not a power/toughness pair"
    );
    assert_total(src);
}

/// 839 reminder spans contain a quoted ability. The `)` inside the quote must
/// not close the reminder early.
#[test]
fn reminder_span_survives_a_quoted_ability_inside_it() {
    let src = r#"(It's an artifact with "{T}: Add {C}. (not really)".)"#;
    let toks = lex(src);
    assert_eq!(toks.len(), 1, "the whole reminder is one token: {toks:?}");
    assert_eq!(toks[0].kind, TokenKind::Reminder { terminated: true });
    assert_eq!(toks[0].text(src), src);
    assert_total(src);
}

#[test]
fn nested_parentheses_close_at_the_outermost() {
    let src = "(outer (inner) still outer)";
    let toks = lex(src);
    assert_eq!(toks.len(), 1);
    assert_eq!(toks[0].text(src), src);
    assert_total(src);
}

/// Two corpus cards (Kylem All-Star, Lander Rizzi) carry Oracle text that is
/// truncated mid-reminder, so the span never closes. The lexer must stay total
/// and must not swallow whatever follows.
#[test]
fn truncated_reminder_span_is_unterminated_but_total() {
    let src = "create a token. (It's an aura with \"enchanted creature gets +1/+1 for each";
    let toks = lex(src);
    assert!(
        toks.iter().any(|t| t.kind == TokenKind::Reminder { terminated: false }),
        "expected an unterminated reminder span: {toks:?}"
    );
    assert_total(src);
}

/// KNOWN LIMITATION, pinned deliberately rather than left to be discovered.
///
/// `"` is its own opening and closing delimiter, so nested quoting cannot be
/// resolved by the lexer. Mijo, the Bull prints an ability inside an ability
/// (`named Rock with "Equipped creature has "{1}, {T}: ..."`), and the lexer
/// pairs quotes in printed order, producing an inner span where the card means
/// an outer one. One card in 35,564 does this, so the grammar owns the repair
/// if it ever needs to; what the lexer guarantees here is only that coverage
/// stays total and nothing panics.
#[test]
fn nested_quotes_pair_in_printed_order_and_stay_total() {
    let src = "named Rock with \"Equipped creature has \"{1}, {T}: deals 2 damage.\" and equip {1}.\"";
    assert_total(src);
    let first = lex(src)
        .into_iter()
        .find(|t| matches!(t.kind, TokenKind::Quoted { .. }))
        .expect("a quoted span");
    assert_eq!(
        first.text(src),
        "\"Equipped creature has \"",
        "documents printed-order pairing, not semantic nesting"
    );
}

#[test]
fn unterminated_brace_does_not_swallow_the_remainder() {
    let src = "{T: Add {R}.";
    assert_total(src);
    assert!(lex(src).len() > 1, "a stray brace must not consume everything");
}

/// Spree prints its options as a line-leading `+` with a cost (21 cards).
#[test]
fn spree_option_head_lexes_as_plus_symbol_emdash() {
    let src = "+ {1} \u{2014} Deals 2 damage.";
    let k = kinds(src);
    assert_eq!(k[0], TokenKind::Plus);
    assert_eq!(k[1], TokenKind::Symbol);
    assert_eq!(k[2], TokenKind::EmDash);
    assert_total(src);
}

/// Die-roll result rows: `1-6 |`, `1—9 |`, `12+ |` (160 rows).
#[test]
fn roll_table_rows_lex_as_composable_atoms() {
    assert_eq!(
        kinds("1-6 | Add {R}."),
        vec![
            TokenKind::Number, TokenKind::Hyphen, TokenKind::Number, TokenKind::Pipe,
            TokenKind::Word, TokenKind::Symbol, TokenKind::Period,
        ]
    );
    assert_eq!(kinds("1\u{2014}9 |")[1], TokenKind::EmDash);
    assert_eq!(
        kinds("12+ |"),
        vec![TokenKind::Number, TokenKind::Plus, TokenKind::Pipe]
    );
    assert_total("12+ | Flying, trample");
}

#[test]
fn newlines_are_emitted_and_other_whitespace_is_not() {
    assert_eq!(
        kinds("a\nb   c"),
        vec![TokenKind::Word, TokenKind::Newline, TokenKind::Word, TokenKind::Word]
    );
    assert_total("a\nb   c");
}

#[test]
fn empty_and_whitespace_only_inputs_are_total() {
    for src in ["", "   ", "\n\n", " \t "] {
        assert_total(src);
    }
}

/// Multi-byte input must never split a char boundary.
#[test]
fn multibyte_text_is_total_and_boundary_safe() {
    for src in ["\u{2014}\u{2022}\u{2212}\u{221E}", "Éomer, Marshal of Rohan", "\u{2610} \u{2192} \u{2666}"] {
        assert_total(src);
        for t in lex(src) {
            assert!(src.is_char_boundary(t.span.start) && src.is_char_boundary(t.span.end));
        }
    }
}

/// The coverage checker must actually be able to fail, or the suite proves nothing.
#[test]
fn coverage_checker_rejects_a_hand_built_gap() {
    let src = "destroy target creature";
    let mut toks = lex(src);
    toks.remove(1);
    assert!(
        matches!(
            verify_coverage(src, &toks),
            Err(phase_oracle_lex::CoverageError::UnclaimedText { .. })
        ),
        "dropping a token must be detected as unclaimed text"
    );
}
