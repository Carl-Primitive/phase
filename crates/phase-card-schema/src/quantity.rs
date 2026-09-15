//! How many, and of what.

use serde::{Deserialize, Serialize};

/// A count that may not be knowable until resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Quantity {
    /// A printed literal: "draw two cards".
    Fixed { value: u32 },
    /// The announced value of X.
    Variable,
    /// "that many", bound to an amount established earlier in the ability.
    ThatMany,
    /// "any number of".
    AnyNumber,
    /// "all"/"each", a count over everything matching.
    All,
}

/// The kinds of counter this slice of the schema recognizes.
///
/// `PlusOnePlusOne` and `MinusOneMinusOne` are named rather than encoded as a
/// power/toughness pair because they are a single printed counter kind, not an
/// arithmetic value, and every consumer treats them as an atom.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CounterKind {
    PlusOnePlusOne,
    MinusOneMinusOne,
    /// Any other printed counter word: charge, loyalty, stun, oil, …
    Named { name: String },
}
