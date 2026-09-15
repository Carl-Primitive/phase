//! Oracle text lexer: text to a token stream carrying byte spans.
//!
//! The parser above this layer matches on tokens, never on raw substrings.
//! That is the design difference from the existing parser, and it is what lets
//! word boundaries, mana symbols, reminder text and granted-ability quoting be
//! settled once here instead of re-derived at every call site.

pub mod lexer;
pub mod token;

pub use lexer::{lex, verify_coverage, CoverageError};
pub use token::{PtPart, Sign, Span, Token, TokenKind};
