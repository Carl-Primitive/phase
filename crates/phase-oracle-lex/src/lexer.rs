//! The scanner: Oracle text to a token stream.
//!
//! Two properties this scanner guarantees, and that the whole parser design
//! rests on:
//!
//! 1. **Totality.** Every byte of the input lies inside exactly one token or
//!    inside an inter-token gap that is whitespace only. [`verify_coverage`]
//!    checks this and is run over the entire corpus in tests.
//! 2. **Panic-freedom.** Malformed input never panics. The corpus contains
//!    three cards with an odd number of double quotes, one of which nests an
//!    unescaped quote inside another, so "well formed" is not an assumption
//!    the lexer is allowed to make.

use crate::token::{PtPart, Sign, Token, TokenKind};

/// Tokenize Oracle text.
///
/// Whitespace other than `\n` is consumed and not emitted; `\n` becomes
/// [`TokenKind::Newline`] because it separates printed abilities.
pub fn lex(src: &str) -> Vec<Token> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;

    while i < src.len() {
        let c = match src[i..].chars().next() {
            Some(c) => c,
            None => break,
        };
        let cw = c.len_utf8();

        // Whitespace: consumed, never emitted, except newline.
        if c == '\n' {
            out.push(Token::new(TokenKind::Newline, i, i + cw));
            i += cw;
            continue;
        }
        if c.is_whitespace() {
            i += cw;
            continue;
        }

        // Braced symbol: `{T}`, `{W/U}`, `{TK}`.
        if c == '{' {
            match memchr(bytes, b'}', i + 1) {
                Some(end) => {
                    out.push(Token::new(TokenKind::Symbol, i, end + 1));
                    i = end + 1;
                }
                // Unterminated brace: emit one Other char and keep going, so a
                // malformed card still yields total coverage.
                None => {
                    out.push(Token::new(TokenKind::Other, i, i + cw));
                    i += cw;
                }
            }
            continue;
        }

        // Parenthesised reminder text. Depth-counted, and quote-aware so a
        // `)` printed inside a granted ability does not close the span early.
        if c == '(' {
            let (end, terminated) = scan_reminder(src, i);
            out.push(Token::new(TokenKind::Reminder { terminated }, i, end));
            i = end;
            continue;
        }

        // Double-quoted span. Not self-nesting: the closing delimiter is the
        // next `"`. Unterminated spans run to end of line, then end of input.
        if c == '"' {
            let (end, terminated) = scan_quoted(src, i);
            out.push(Token::new(TokenKind::Quoted { terminated }, i, end));
            i = end;
            continue;
        }

        // Bracketed loyalty cost: `[+1]`, `[−3]`, `[0]`, `[−X]`.
        if c == '[' {
            if let Some((cost, end)) = scan_loyalty(src, i) {
                out.push(Token::new(TokenKind::Loyalty { cost }, i, end));
                i = end;
                continue;
            }
            out.push(Token::new(TokenKind::Other, i, i + cw));
            i += cw;
            continue;
        }

        // Power/toughness pair, signed or bare. Tried before Number and Word
        // so `+2/+1`, `2/2` and `*/*` stay single tokens.
        if let Some((power, toughness, end)) = scan_pt_pair(src, i) {
            out.push(Token::new(TokenKind::PtPair { power, toughness }, i, end));
            i = end;
            continue;
        }

        if c.is_ascii_digit() {
            let end = scan_while(src, i, |ch| ch.is_ascii_digit());
            out.push(Token::new(TokenKind::Number, i, end));
            i = end;
            continue;
        }

        if c.is_alphabetic() {
            let end = scan_word(src, i);
            out.push(Token::new(TokenKind::Word, i, end));
            i = end;
            continue;
        }

        let kind = match c {
            '.' => TokenKind::Period,
            ',' => TokenKind::Comma,
            ';' => TokenKind::Semicolon,
            ':' => TokenKind::Colon,
            // U+A789 MODIFIER LETTER COLON appears twice in the corpus where an
            // ordinary colon is meant. Folded here so the cost/effect grammar
            // above never has to know two spellings exist.
            '\u{A789}' => TokenKind::Colon,
            '+' => TokenKind::Plus,
            '-' => TokenKind::Hyphen,
            '|' => TokenKind::Pipe,
            '/' => TokenKind::Slash,
            '~' => TokenKind::SelfRef,
            '•' => TokenKind::Bullet,
            '—' => TokenKind::EmDash,
            _ => TokenKind::Other,
        };
        out.push(Token::new(kind, i, i + cw));
        i += cw;
    }

    out
}

/// Byte search from `from`, returning an absolute index.
fn memchr(bytes: &[u8], needle: u8, from: usize) -> Option<usize> {
    bytes
        .get(from..)?
        .iter()
        .position(|&b| b == needle)
        .map(|p| p + from)
}

/// Scan a parenthesised span starting at `open`. Returns the end offset
/// (exclusive) and whether a matching `)` was actually found.
fn scan_reminder(src: &str, open: usize) -> (usize, bool) {
    let mut depth = 0usize;
    let mut in_quote = false;
    for (off, ch) in src[open..].char_indices() {
        let abs = open + off;
        match ch {
            '"' => in_quote = !in_quote,
            '(' if !in_quote => depth += 1,
            ')' if !in_quote => {
                depth -= 1;
                if depth == 0 {
                    return (abs + ch.len_utf8(), true);
                }
            }
            _ => {}
        }
    }
    (src.len(), false)
}

/// Scan a double-quoted span starting at `open`.
fn scan_quoted(src: &str, open: usize) -> (usize, bool) {
    let rest = &src[open + 1..];
    for (off, ch) in rest.char_indices() {
        if ch == '"' {
            return (open + 1 + off + ch.len_utf8(), true);
        }
        // An unterminated quote is bounded at the line break rather than
        // swallowing the rest of the card. Three corpus cards need this.
        if ch == '\n' {
            return (open + 1 + off, false);
        }
    }
    (src.len(), false)
}

/// Read an optional sign at `i`, returning the sign and the offset after it.
fn scan_sign(src: &str, i: usize) -> (Sign, usize) {
    match src[i..].chars().next() {
        Some('+') => (Sign::Plus, i + 1),
        // U+002D HYPHEN-MINUS and U+2212 MINUS SIGN both mean minus.
        Some('-') => (Sign::Minus, i + 1),
        Some('−') => (Sign::Minus, i + '−'.len_utf8()),
        _ => (Sign::None, i),
    }
}

/// Read one P/T part (`2`, `X`, `*`) with its optional sign.
fn scan_pt_part(src: &str, i: usize) -> Option<(PtPart, usize)> {
    let (sign, after_sign) = scan_sign(src, i);
    let mut chars = src[after_sign..].chars();
    match chars.next()? {
        c if c.is_ascii_digit() => {
            let end = scan_while(src, after_sign, |ch| ch.is_ascii_digit());
            let value = src[after_sign..end].parse().ok()?;
            Some((PtPart::Number { sign, value }, end))
        }
        'X' | 'x' => Some((PtPart::Variable { sign }, after_sign + 1)),
        '*' => Some((PtPart::Star { sign }, after_sign + 1)),
        _ => None,
    }
}

/// Read a `<part>/<part>` pair. Returns `None` when the shape does not hold,
/// leaving the caller to lex the text some other way.
fn scan_pt_pair(src: &str, i: usize) -> Option<(PtPart, PtPart, usize)> {
    let (power, after_power) = scan_pt_part(src, i)?;
    if src[after_power..].chars().next()? != '/' {
        return None;
    }
    let (toughness, end) = scan_pt_part(src, after_power + 1)?;
    // Reject a trailing alphanumeric so `2/2x` is not read as a pair.
    if src[end..]
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric())
    {
        return None;
    }
    Some((power, toughness, end))
}

/// Read a bracketed loyalty cost.
fn scan_loyalty(src: &str, open: usize) -> Option<(PtPart, usize)> {
    let (cost, after) = scan_pt_part(src, open + 1)?;
    if src[after..].chars().next()? != ']' {
        return None;
    }
    Some((cost, after + 1))
}

/// Read a word: alphabetic, with internal apostrophes and internal hyphens.
///
/// A trailing apostrophe is kept (`opponents'`); a trailing hyphen is not,
/// since a hyphen only binds when another letter follows it.
fn scan_word(src: &str, start: usize) -> usize {
    let mut end = start;
    let mut chars = src[start..].char_indices().peekable();
    while let Some((off, ch)) = chars.next() {
        let abs = start + off;
        if ch.is_alphabetic() {
            end = abs + ch.len_utf8();
            continue;
        }
        if ch == '\'' {
            end = abs + ch.len_utf8();
            continue;
        }
        if ch == '-' {
            match chars.peek() {
                Some((_, next)) if next.is_alphabetic() => {
                    end = abs + ch.len_utf8();
                    continue;
                }
                _ => break,
            }
        }
        break;
    }
    end
}

fn scan_while(src: &str, start: usize, pred: impl Fn(char) -> bool) -> usize {
    let mut end = start;
    for (off, ch) in src[start..].char_indices() {
        if !pred(ch) {
            return start + off;
        }
        end = start + off + ch.len_utf8();
    }
    end
}

/// A byte range the lexer failed to account for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverageError {
    /// Two tokens overlap, or they are not in ascending order.
    Overlap {
        previous_end: usize,
        next_start: usize,
    },
    /// A gap between tokens held something other than whitespace.
    UnclaimedText {
        start: usize,
        end: usize,
        text: String,
    },
}

/// Assert that `tokens` account for every byte of `src`.
///
/// This is the invariant that makes the grammar above it checkable: if the
/// lexer claims every byte, a clause that fails to consume every token it was
/// handed is detectable structurally, with no post-hoc text auditor.
pub fn verify_coverage(src: &str, tokens: &[Token]) -> Result<(), CoverageError> {
    let mut cursor = 0usize;
    for tok in tokens {
        if tok.span.start < cursor {
            return Err(CoverageError::Overlap {
                previous_end: cursor,
                next_start: tok.span.start,
            });
        }
        let gap = &src[cursor..tok.span.start];
        if !gap.chars().all(char::is_whitespace) {
            return Err(CoverageError::UnclaimedText {
                start: cursor,
                end: tok.span.start,
                text: gap.to_string(),
            });
        }
        cursor = tok.span.end;
    }
    let tail = &src[cursor..];
    if !tail.chars().all(char::is_whitespace) {
        return Err(CoverageError::UnclaimedText {
            start: cursor,
            end: src.len(),
            text: tail.to_string(),
        });
    }
    Ok(())
}
