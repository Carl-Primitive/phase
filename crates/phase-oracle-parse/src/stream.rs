//! A nom [`Input`] over a token slice.
//!
//! This is what lets the grammar be written in nom while matching on tokens
//! rather than on raw text. Every combinator below this point sees
//! [`Token`]s, so word boundaries, mana symbols, reminder spans and quoting
//! are settled once in the lexer and never re-derived in a production.

use nom::{Compare, CompareResult, Input, Needed};
use phase_oracle_lex::{Token, TokenKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tokens<'a> {
    pub toks: &'a [Token],
    /// The text the tokens index into, so a production can read a word's
    /// spelling without the lexer having to allocate one string per token.
    pub src: &'a str,
}

impl<'a> Tokens<'a> {
    pub fn new(toks: &'a [Token], src: &'a str) -> Self {
        Self { toks, src }
    }

    pub fn first(&self) -> Option<&'a Token> {
        self.toks.first()
    }

    pub fn is_empty(&self) -> bool {
        self.toks.is_empty()
    }

    /// The lowercase spelling of the first token, for word matching.
    pub fn first_word(&self) -> Option<String> {
        let t = self.first()?;
        (t.kind == TokenKind::Word).then(|| t.text(self.src).to_lowercase())
    }

    /// Drop the first `n` tokens.
    pub fn take_from_n(&self, n: usize) -> Self {
        Tokens {
            toks: &self.toks[n.min(self.toks.len())..],
            src: self.src,
        }
    }

    /// Byte span covered by the remaining tokens.
    pub fn span(&self) -> Option<(usize, usize)> {
        Some((self.toks.first()?.span.start, self.toks.last()?.span.end))
    }
}

impl<'a> Input for Tokens<'a> {
    type Item = &'a Token;
    type Iter = std::slice::Iter<'a, Token>;
    type IterIndices = std::iter::Enumerate<std::slice::Iter<'a, Token>>;

    fn input_len(&self) -> usize {
        self.toks.len()
    }

    fn take(&self, index: usize) -> Self {
        Tokens {
            toks: &self.toks[..index],
            src: self.src,
        }
    }

    fn take_from(&self, index: usize) -> Self {
        Tokens {
            toks: &self.toks[index..],
            src: self.src,
        }
    }

    fn take_split(&self, index: usize) -> (Self, Self) {
        let (a, b) = self.toks.split_at(index);
        (
            Tokens {
                toks: b,
                src: self.src,
            },
            Tokens {
                toks: a,
                src: self.src,
            },
        )
    }

    fn position<P>(&self, predicate: P) -> Option<usize>
    where
        P: Fn(Self::Item) -> bool,
    {
        self.toks.iter().position(predicate)
    }

    fn iter_elements(&self) -> Self::Iter {
        self.toks.iter()
    }

    fn iter_indices(&self) -> Self::IterIndices {
        self.toks.iter().enumerate()
    }

    fn slice_index(&self, count: usize) -> Result<usize, Needed> {
        if self.toks.len() >= count {
            Ok(count)
        } else {
            Err(Needed::new(count - self.toks.len()))
        }
    }
}

/// Compare a token stream against a literal phrase, word by word, case-insensitively.
///
/// This is what makes `tag("destroy target creature")` mean "three word tokens
/// spelled thus" rather than "this substring appears somewhere". A phrase can
/// never match across a word boundary, which is the whole class of bug the
/// lexer exists to remove.
impl<'a> Compare<&str> for Tokens<'a> {
    fn compare(&self, t: &str) -> CompareResult {
        let mut toks = self.toks.iter();
        for want in t.split_whitespace() {
            match toks.next() {
                None => return CompareResult::Incomplete,
                Some(tok) => {
                    if !tok.text(self.src).eq_ignore_ascii_case(want) {
                        return CompareResult::Error;
                    }
                }
            }
        }
        CompareResult::Ok
    }

    fn compare_no_case(&self, t: &str) -> CompareResult {
        self.compare(t)
    }
}
