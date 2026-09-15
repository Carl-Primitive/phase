//! Activation and additional costs, mirroring `phase_engine::types::ability::AbilityCost`.

use serde::{Deserialize, Serialize};

use crate::filter::TargetFilter;
use crate::qty::Quantity;

/// One component of a printed mana cost. CR 107.4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManaShard {
    White,
    Blue,
    Black,
    Red,
    Green,
    Colorless,
    Snow,
    X,
    TwoOrMoreColorSource,
    WhiteBlue,
    WhiteBlack,
    BlueBlack,
    BlueRed,
    BlackRed,
    BlackGreen,
    RedWhite,
    RedGreen,
    GreenWhite,
    GreenBlue,
    TwoWhite,
    TwoBlue,
    TwoBlack,
    TwoRed,
    TwoGreen,
    PhyrexianWhite,
    PhyrexianBlue,
    PhyrexianBlack,
    PhyrexianRed,
    PhyrexianGreen,
    ColorlessWhite,
    ColorlessBlue,
    ColorlessBlack,
    ColorlessRed,
    ColorlessGreen,
}

impl ManaShard {
    /// The printed symbol body, without braces. This is the single authority
    /// mapping a lexed `{...}` symbol onto a shard, so the grammar never
    /// re-derives it.
    pub fn from_symbol(body: &str) -> Option<Self> {
        let up = body.to_ascii_uppercase();
        Some(match up.as_str() {
            "W" => Self::White,
            "U" => Self::Blue,
            "B" => Self::Black,
            "R" => Self::Red,
            "G" => Self::Green,
            "C" => Self::Colorless,
            "S" => Self::Snow,
            "X" => Self::X,
            "Z" => Self::TwoOrMoreColorSource,
            "W/U" | "U/W" => Self::WhiteBlue,
            "W/B" | "B/W" => Self::WhiteBlack,
            "U/B" | "B/U" => Self::BlueBlack,
            "U/R" | "R/U" => Self::BlueRed,
            "B/R" | "R/B" => Self::BlackRed,
            "B/G" | "G/B" => Self::BlackGreen,
            "R/W" | "W/R" => Self::RedWhite,
            "R/G" | "G/R" => Self::RedGreen,
            "G/W" | "W/G" => Self::GreenWhite,
            "G/U" | "U/G" => Self::GreenBlue,
            "2/W" => Self::TwoWhite,
            "2/U" => Self::TwoBlue,
            "2/B" => Self::TwoBlack,
            "2/R" => Self::TwoRed,
            "2/G" => Self::TwoGreen,
            "W/P" => Self::PhyrexianWhite,
            "U/P" => Self::PhyrexianBlue,
            "B/P" => Self::PhyrexianBlack,
            "R/P" => Self::PhyrexianRed,
            "G/P" => Self::PhyrexianGreen,
            "C/W" => Self::ColorlessWhite,
            "C/U" => Self::ColorlessBlue,
            "C/B" => Self::ColorlessBlack,
            "C/R" => Self::ColorlessRed,
            "C/G" => Self::ColorlessGreen,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ManaCost {
    NoCost,
    Cost {
        shards: Vec<ManaShard>,
        generic: u32,
    },
}

/// CR 118.3: what a player pays to activate an ability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AbilityCost {
    Mana {
        cost: ManaCost,
    },
    /// CR 118.3 + CR 701.20a: the `{T}` symbol.
    Tap,
    Untap,
    /// CR 118.4 + CR 606.3: a planeswalker loyalty cost.
    Loyalty {
        amount: i32,
    },
    /// CR 119.4.
    PayLife {
        amount: Quantity,
    },
    Sacrifice(SacrificeCost),
    /// `filter`, `random` and `self_scope` are always printed by the engine,
    /// including at their defaults, so the mirror prints them too.
    Discard {
        count: Quantity,
        filter: Option<TargetFilter>,
        #[serde(rename = "random")]
        selection_random: bool,
        #[serde(rename = "self_ref")]
        self_scope: bool,
    },
    /// All listed sub-costs must be paid. This is the AND-composition; the
    /// engine's `OneOf` is the OR-composition and is a separate variant.
    Composite {
        costs: Vec<AbilityCost>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SacrificeCost {
    pub target: TargetFilter,
    pub count: u32,
}
