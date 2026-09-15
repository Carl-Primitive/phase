//! Grammar over the Oracle token stream, producing [`phase_card_schema`] values.

#[cfg(feature = "corpus")]
pub mod bridge;
pub mod clause;
pub mod prim;
pub mod stream;
pub mod target;

use phase_card_schema::{Clause, SourceSpan};
use phase_oracle_lex::{lex, Token, TokenKind};

use crate::stream::Tokens;

/// Replace every printing of the card's own name with a single `CARDNAME`
/// word, so self-reference is one token rather than a name-shaped phrase the
/// grammar would otherwise have to re-recognize at every call site.
///
/// Both the full name and the pre-comma short name are replaced, because Magic
/// prints "Jace, the Mind Sculptor" once and then "Jace" thereafter.
pub fn normalize_self_reference(name: &str, oracle: &str) -> String {
    let mut out = oracle.replace(name, "CARDNAME");
    if let Some(short) = name.split(',').next() {
        if short != name && short.len() >= 3 {
            out = out.replace(short, "CARDNAME");
        }
    }
    out
}

/// Split a token slice into clauses at sentence punctuation.
fn sentences<'a>(toks: &'a [Token]) -> Vec<&'a [Token]> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for (idx, t) in toks.iter().enumerate() {
        if matches!(t.kind, TokenKind::Period | TokenKind::Newline) {
            if idx > start {
                out.push(&toks[start..=idx]);
            }
            start = idx + 1;
        }
    }
    if start < toks.len() {
        out.push(&toks[start..]);
    }
    out
}

/// Parse one card's Oracle text into schema clauses.
///
/// Reminder text is dropped before parsing: it restates rules rather than
/// creating them, and it is the one span the grammar is allowed to discard
/// because the lexer proved it was a complete parenthesised unit.
pub fn parse_card(name: &str, oracle: &str) -> Vec<Clause> {
    let src = normalize_self_reference(name, oracle);
    let all = lex(&src);
    let kept: Vec<Token> = all
        .into_iter()
        .filter(|t| !matches!(t.kind, TokenKind::Reminder { .. }))
        .collect();

    sentences(&kept)
        .into_iter()
        .filter(|s| s.iter().any(|t| t.kind == TokenKind::Word))
        .map(|s| {
            let stream = Tokens::new(s, &src);
            let (start, end) = stream.span().unwrap_or((0, 0));
            let text = src[start..end].trim().to_string();
            let (effect, duration) = clause::parse_clause(stream, &text);
            Clause { effect, duration, source: SourceSpan::new(start, end) }
        })
        .collect()
}
