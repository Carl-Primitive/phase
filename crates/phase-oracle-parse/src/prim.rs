//! Leaf productions: single tokens and small fixed phrases.

use nom::error::{Error, ErrorKind};
use nom::{Err, IResult};
use phase_oracle_ast::{CounterType, ManaShard, Quantity};
use phase_oracle_lex::{PtPart, Sign, TokenKind};

use crate::stream::Tokens;

pub type In<'a> = Tokens<'a>;
pub type R<'a, T> = IResult<In<'a>, T>;

pub fn fail<'a, T>(i: In<'a>) -> R<'a, T> {
    Err(Err::Error(Error::new(i, ErrorKind::Tag)))
}

/// Consume one token of exactly this kind.
pub fn kind<'a>(k: TokenKind) -> impl Fn(In<'a>) -> R<'a, &'a phase_oracle_lex::Token> {
    move |i: In<'a>| match i.first() {
        Some(t) if t.kind == k => Ok((i.take_from_n(1), t)),
        _ => fail(i),
    }
}

/// Consume one Word token spelled `w`, case-insensitively.
pub fn word<'a>(w: &'static str) -> impl Fn(In<'a>) -> R<'a, ()> {
    move |i: In<'a>| match i.first_word() {
        Some(got) if got == w => Ok((i.take_from_n(1), ())),
        _ => fail(i),
    }
}

/// Consume the first Word token matching any of `ws`, yielding which one.
///
/// This is the shape that keeps an `alt()` over a vocabulary axis to ONE call
/// rather than one arm per spelling.
pub fn any_of<'a>(ws: &'static [&'static str]) -> impl Fn(In<'a>) -> R<'a, &'static str> {
    move |i: In<'a>| {
        let Some(got) = i.first_word() else {
            return fail(i);
        };
        match ws.iter().find(|w| **w == got) {
            Some(w) => Ok((i.take_from_n(1), *w)),
            None => fail(i),
        }
    }
}

/// Consume a fixed phrase of words given at runtime.
pub fn phrase_static<'a>(p: &'a str) -> impl Fn(In<'a>) -> R<'a, ()> + 'a {
    move |mut i: In<'a>| {
        for w in p.split_whitespace() {
            match i.first_word() {
                Some(got) if got == w => i = i.take_from_n(1),
                _ => return fail(i),
            }
        }
        Ok((i, ()))
    }
}

/// Consume a fixed phrase of words, e.g. `phrase("until end of turn")`.
pub fn phrase<'a>(p: &'static str) -> impl Fn(In<'a>) -> R<'a, ()> {
    move |mut i: In<'a>| {
        for w in p.split_whitespace() {
            match i.first_word() {
                Some(got) if got == w => i = i.take_from_n(1),
                _ => return fail(i),
            }
        }
        Ok((i, ()))
    }
}

/// Try each phrase in order, yielding the value paired with the one that matched.
pub fn phrase_alt<'a, T: Clone>(
    table: &'static [(&'static str, T)],
) -> impl Fn(In<'a>) -> R<'a, T> {
    move |i: In<'a>| {
        for (p, v) in table {
            if let Ok((r, _)) = phrase(p)(i) {
                return Ok((r, v.clone()));
            }
        }
        fail(i)
    }
}

/// Consume any Word token, yielding its lowercase spelling.
pub fn any_word(i: In<'_>) -> R<'_, String> {
    match i.first_word() {
        Some(w) => Ok((i.take_from_n(1), w)),
        None => fail(i),
    }
}

/// Consume one token of this kind if present; never fails.
pub fn opt_kind<'a>(k: TokenKind) -> impl Fn(In<'a>) -> R<'a, bool> {
    move |i: In<'a>| match i.first() {
        Some(t) if t.kind == k => Ok((i.take_from_n(1), true)),
        _ => Ok((i, false)),
    }
}

/// `~`, the card's own name after normalization.
pub fn self_ref(i: In<'_>) -> R<'_, ()> {
    kind(TokenKind::SelfRef)(i).map(|(r, _)| (r, ()))
}

/// English number words, plus digits. The corpus tops out at "hundred".
fn word_number(w: &str) -> Option<i32> {
    Some(match w {
        "a" | "an" | "one" => 1,
        "two" => 2,
        "three" => 3,
        "four" => 4,
        "five" => 5,
        "six" => 6,
        "seven" => 7,
        "eight" => 8,
        "nine" => 9,
        "ten" => 10,
        "eleven" => 11,
        "twelve" => 12,
        "thirteen" => 13,
        "fourteen" => 14,
        "fifteen" => 15,
        "sixteen" => 16,
        "seventeen" => 17,
        "eighteen" => 18,
        "nineteen" => 19,
        "twenty" => 20,
        "thirty" => 30,
        "forty" => 40,
        "fifty" => 50,
        "hundred" => 100,
        _ => return None,
    })
}

/// A bare integer, from digits or from an English number word.
pub fn number(i: In<'_>) -> R<'_, i32> {
    match i.first() {
        Some(t) if t.kind == TokenKind::Number => match t.text(i.src).parse() {
            Ok(v) => Ok((i.take_from_n(1), v)),
            Err(_) => fail(i),
        },
        Some(t) if t.kind == TokenKind::Word => match word_number(&t.text(i.src).to_lowercase()) {
            Some(v) => Ok((i.take_from_n(1), v)),
            None => fail(i),
        },
        _ => fail(i),
    }
}

/// A count in any printed form.
///
/// CR 107.3: `X` is a REFERENCE to a value chosen elsewhere, never a constant,
/// so it lowers to `Quantity::Ref` and not to `Fixed`.
pub fn quantity(i: In<'_>) -> R<'_, Quantity> {
    if let Some(w) = i.first_word() {
        if w == "x" {
            return Ok((i.take_from_n(1), Quantity::variable_x()));
        }
    }
    let (r, v) = number(i)?;
    Ok((r, Quantity::fixed(v)))
}

fn pt_value(p: PtPart) -> Option<i32> {
    match p {
        PtPart::Number { sign, value } => {
            let v = value as i32;
            Some(if sign == Sign::Minus { -v } else { v })
        }
        // `X` and `*` in a P/T modification are not constants; the grammar has
        // no production for them yet, so the clause declines rather than
        // silently pumping by zero.
        PtPart::Variable { .. } | PtPart::Star { .. } => None,
    }
}

/// A power/toughness pair token, yielding signed literal values.
pub fn pt_pair(i: In<'_>) -> R<'_, (i32, i32)> {
    match i.first() {
        Some(t) => match t.kind {
            TokenKind::PtPair { power, toughness } => {
                match (pt_value(power), pt_value(toughness)) {
                    (Some(p), Some(tg)) => Ok((i.take_from_n(1), (p, tg))),
                    _ => fail(i),
                }
            }
            _ => fail(i),
        },
        None => fail(i),
    }
}

/// A counter kind: `+1/+1`, `-1/-1`, or a named counter word.
pub fn counter_type(i: In<'_>) -> R<'_, CounterType> {
    if let Ok((rest, (p, t))) = pt_pair(i) {
        let k = match (p, t) {
            (1, 1) => CounterType::Plus1Plus1,
            (-1, -1) => CounterType::Minus1Minus1,
            // An asymmetric counter is a real engine shape
            // (`CounterType::PowerToughness`) this grammar does not yet emit.
            _ => return fail(i),
        };
        return Ok((rest, k));
    }
    let (rest, w) = any_word(i)?;
    Ok((rest, CounterType::Named(w)))
}

/// The body of a braced symbol, without its braces: `{T}` yields `T`.
pub fn symbol_body(i: In<'_>) -> R<'_, &str> {
    match i.first() {
        Some(t) if t.kind == TokenKind::Symbol => {
            let body = t.text(i.src).trim_start_matches('{').trim_end_matches('}');
            Ok((i.take_from_n(1), body))
        }
        _ => fail(i),
    }
}

/// One mana symbol, as a shard or as a generic amount.
///
/// Generic mana is a COUNT, not a shard, which is why this yields a two-armed
/// result rather than an `Option<ManaShard>`: `{3}` contributes 3 to `generic`
/// while `{R}` pushes one shard.
pub enum ManaSym {
    Shard(ManaShard),
    Generic(u32),
}

pub fn mana_symbol(i: In<'_>) -> R<'_, ManaSym> {
    let (r, body) = symbol_body(i)?;
    if let Ok(n) = body.parse::<u32>() {
        return Ok((r, ManaSym::Generic(n)));
    }
    match ManaShard::from_symbol(body) {
        Some(s) => Ok((r, ManaSym::Shard(s))),
        None => fail(i),
    }
}
