//! Leaf productions: single tokens and small fixed phrases.

use nom::error::{Error, ErrorKind};
use nom::{Err, IResult};
use phase_card_schema::{CounterKind, Quantity};
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

/// Consume any Word token, yielding its lowercase spelling.
pub fn any_word(i: In<'_>) -> R<'_, String> {
    match i.first_word() {
        Some(w) => Ok((i.take_from_n(1), w)),
        None => fail(i),
    }
}

/// English number words, plus digits. The corpus tops out at "hundred".
fn word_number(w: &str) -> Option<u32> {
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
        "twenty" => 20,
        "thirty" => 30,
        "fifty" => 50,
        "hundred" => 100,
        _ => return None,
    })
}

/// A count in any printed form.
pub fn quantity(i: In<'_>) -> R<'_, Quantity> {
    if let Ok((rest, _)) = phrase("that many")(i) {
        return Ok((rest, Quantity::ThatMany));
    }
    if let Ok((rest, _)) = phrase("any number of")(i) {
        return Ok((rest, Quantity::AnyNumber));
    }
    match i.first() {
        Some(t) if t.kind == TokenKind::Number => {
            let v = t.text(i.src).parse().map_err(|_| Err::Error(Error::new(i, ErrorKind::Digit)))?;
            Ok((i.take_from_n(1), Quantity::Fixed { value: v }))
        }
        Some(t) if t.kind == TokenKind::Word => {
            let w = t.text(i.src).to_lowercase();
            if w == "x" {
                return Ok((i.take_from_n(1), Quantity::Variable));
            }
            if w == "all" || w == "each" {
                return Ok((i.take_from_n(1), Quantity::All));
            }
            match word_number(&w) {
                Some(v) => Ok((i.take_from_n(1), Quantity::Fixed { value: v })),
                None => fail(i),
            }
        }
        _ => fail(i),
    }
}

fn pt_value(p: PtPart) -> (i32, bool) {
    match p {
        PtPart::Number { sign, value } => {
            let v = value as i32;
            (if sign == Sign::Minus { -v } else { v }, false)
        }
        PtPart::Variable { .. } | PtPart::Star { .. } => (0, true),
    }
}

/// A power/toughness pair token, yielding signed values.
pub fn pt_pair(i: In<'_>) -> R<'_, (i32, i32, bool)> {
    match i.first() {
        Some(t) => match t.kind {
            TokenKind::PtPair { power, toughness } => {
                let (p, pv) = pt_value(power);
                let (tg, tv) = pt_value(toughness);
                Ok((i.take_from_n(1), (p, tg, pv || tv)))
            }
            _ => fail(i),
        },
        None => fail(i),
    }
}

/// A counter kind: `+1/+1`, `-1/-1`, or a named counter word.
pub fn counter_kind(i: In<'_>) -> R<'_, CounterKind> {
    if let Ok((rest, (p, t, _))) = pt_pair(i) {
        let k = match (p, t) {
            (1, 1) => CounterKind::PlusOnePlusOne,
            (-1, -1) => CounterKind::MinusOneMinusOne,
            _ => return fail(i),
        };
        return Ok((rest, k));
    }
    let (rest, w) = any_word(i)?;
    Ok((rest, CounterKind::Named { name: w }))
}
