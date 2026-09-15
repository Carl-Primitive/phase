//! Token vocabulary for Oracle text.
//!
//! The vocabulary is derived from a census of all 35,564 cards carrying Oracle
//! text in `data/card-data.json`, not from intuition. Counts cited on each
//! variant are corpus occurrences at the time of writing.

use std::ops::Range;

/// A half-open byte range into the source text.
///
/// Byte offsets, not char offsets: they index `&str` directly, and every
/// boundary the lexer emits lands on a UTF-8 char boundary by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        debug_assert!(start <= end, "span start must not exceed end");
        Self { start, end }
    }

    pub fn len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Slice the source this span was produced from.
    pub fn of<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start..self.end]
    }
}

impl From<Span> for Range<usize> {
    fn from(s: Span) -> Self {
        s.start..s.end
    }
}

/// Sign carried by a power/toughness part or a loyalty cost.
///
/// Magic prints loyalty minus as U+2212 MINUS SIGN (643 occurrences), not
/// U+002D HYPHEN-MINUS. Both are accepted and normalize to `Minus`, so no
/// downstream consumer has to know which codepoint was printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sign {
    Plus,
    Minus,
    /// No printed sign, as in the bare `2/2` of a token's printed body.
    None,
}

/// One part of a power/toughness pair: a literal, or `X`/`*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtPart {
    Number {
        sign: Sign,
        value: u32,
    },
    Variable {
        sign: Sign,
    },
    /// `*`, the characteristic-defining star.
    Star {
        sign: Sign,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// An alphabetic run. Internal apostrophes and hyphens are kept inside the
    /// token ("opponent's", "can't", "Ancient-Tomb"), because splitting them
    /// would force every grammar rule to reassemble them.
    Word,
    /// A run of ASCII digits with no sign and no slash.
    Number,
    /// A braced symbol, including the braces: `{T}`, `{2}`, `{W/U}`, `{TK}`.
    /// 62 distinct symbols occur; the payload is left unparsed here so the
    /// mana grammar owns interpretation.
    Symbol,
    /// A bracketed loyalty cost: `[+1]`, `[−3]`, `[0]`, `[−X]`. 1,108 brackets.
    Loyalty {
        cost: PtPart,
    },
    /// A power/toughness pair: `+2/+1`, `-1/-1`, `2/2`, `+X/+X`, `*/*`.
    /// 10,348 signed and 4,165 bare occurrences.
    PtPair {
        power: PtPart,
        toughness: PtPart,
    },
    /// A parenthesised reminder-text span, including its delimiters.
    /// Nesting-aware: 839 reminder spans contain a quoted ability, and 5 nest
    /// parentheses more than one deep.
    Reminder {
        terminated: bool,
    },
    /// A double-quoted span, including its delimiters. Usually a granted or
    /// printed ability belonging to another object.
    Quoted {
        terminated: bool,
    },
    /// `~`, the card's own name after self-reference normalization.
    ///
    /// Never printed on a card: the corpus contains no tilde. It exists because
    /// normalization runs BEFORE the lexer, so "Shivan Dragon" and "this
    /// creature" both arrive here as one token the grammar matches once,
    /// instead of a name-shaped phrase every production would have to
    /// re-recognize. It is also exactly the spelling the engine prints in an
    /// ability's `description`, so descriptions need no second substitution.
    SelfRef,
    /// `•`, the modal option marker. 2,061 occurrences.
    Bullet,
    /// `—` U+2014 EM DASH. Separates ability-word and chapter heads. 4,682.
    EmDash,
    Period,
    Comma,
    Semicolon,
    Colon,
    /// A solidus that is not part of a symbol or a P/T pair.
    Slash,
    /// `+` standing alone. Structural in two places the grammar resolves by
    /// position: a line-leading `+` opens a Spree option (21 cards), and a
    /// trailing `+` closes a threshold row head such as `12+ |`.
    Plus,
    /// `-` U+002D standing alone, most often the low/high separator of a
    /// die-roll range (`1-6 |`). Its em-dash spelling is [`TokenKind::EmDash`].
    Hyphen,
    /// `|`, the die-roll and threshold result-table row separator. 160 rows
    /// across the corpus, in the shapes `1-6 |`, `1—9 |` and `12+ |`.
    Pipe,
    /// A line break. Structurally significant: it separates printed abilities.
    Newline,
    /// Any other printed character. Kept as a token rather than dropped so
    /// coverage stays total. The corpus tail includes `☐ → ♦ ∞ √ ꞉ | & ! ? _ # %`.
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, start: usize, end: usize) -> Self {
        Self {
            kind,
            span: Span::new(start, end),
        }
    }

    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        self.span.of(source)
    }
}
