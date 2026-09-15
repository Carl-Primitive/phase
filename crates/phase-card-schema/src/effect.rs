//! What a clause does.
//!
//! Vocabulary chosen from a corpus census of sentence-initial verbs, so each
//! variant covers a printed family rather than a single card. Counts in the
//! comments are candidate sentences in the 35,564-card corpus.

use serde::{Deserialize, Serialize};

use crate::filter::Target;
use crate::quantity::{CounterKind, Quantity};

/// How long a continuous effect lasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Duration {
    EndOfTurn,
    EndOfCombat,
    YourNextTurn,
    /// No printed end: the effect lasts as long as its source does.
    Indefinite,
}

/// A power/toughness modification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PtChange {
    pub power: i32,
    pub toughness: i32,
    /// True when the printed value was `X` or `*` rather than a literal.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub variable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Effect {
    /// "destroy target creature" (744)
    Destroy { target: Target },
    /// "exile target creature" (551)
    Exile { target: Target },
    /// "<source> deals N damage to <target>"
    DealDamage { amount: Quantity, target: Target },
    /// "draw a card" / "draw two cards" (611)
    Draw { who: Target, amount: Quantity },
    /// "you gain 3 life"
    GainLife { who: Target, amount: Quantity },
    /// "each opponent loses 2 life"
    LoseLife { who: Target, amount: Quantity },
    /// "target creature gets +2/+2"
    ModifyPt { target: Target, change: PtChange },
    /// "put a +1/+1 counter on target creature" (610)
    PutCounter { target: Target, counter: CounterKind, amount: Quantity },
    /// "tap target creature" / "untap target permanent" (191)
    SetTapped { target: Target, tapped: bool },
    /// "counter target spell" (321)
    CounterSpell { target: Target },
    /// "sacrifice a creature"
    Sacrifice { who: Target, amount: Quantity },
    /// "discard a card"
    Discard { who: Target, amount: Quantity },
    /// "each player mills three cards"
    Mill { who: Target, amount: Quantity },
    /// "return target creature card to your hand" (542)
    ReturnToHand { target: Target },
    /// "create a 2/2 white Knight creature token" (509)
    CreateToken { amount: Quantity, description: String },
    /// "target creature gains flying"
    GainKeyword { target: Target, keyword: String },
    /// Two or more effects printed as one sentence, joined by "and" or "then".
    /// "then" is ordered; "and" is not, but both lower to the same shape here
    /// because the schema records what was printed, not how a runtime schedules it.
    Sequence { effects: Vec<Effect>, ordered: bool },
    /// A clause the grammar declined. Carries the exact printed text and the
    /// reason, so a gap is always attributable to a span rather than to a card.
    ///
    /// This is the schema's honest-failure representation. A consumer must
    /// treat it as "this clause is not represented", never as a no-op.
    Unparsed { text: String, reason: DeclineReason },
}

/// Why the grammar declined a clause.
///
/// Structural rather than textual: each variant names the production that
/// refused, so a census of declines is a work list over the grammar and not a
/// list of card names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclineReason {
    /// No production matched the clause's head.
    UnknownVerb,
    /// The head matched but its object did not parse.
    UnparsedTarget,
    /// The head matched but a count did not parse.
    UnparsedQuantity,
    /// Everything matched, but tokens were left over. This is the decline that
    /// the totality rule produces, and the one a post-hoc text auditor exists
    /// to catch in a parser that cannot state it directly.
    TrailingTokens,
}
