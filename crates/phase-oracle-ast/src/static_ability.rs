//! Continuous effects. CR 611.
//!
//! A granted keyword is NOT an effect in the engine's format — it is a
//! continuous modification carried by a `StaticAbility`, wrapped in a
//! `GenericEffect` when a spell or activated ability grants it for a duration.
//! That indirection is the engine's, and reproducing it is what makes "target
//! creature gains flying until end of turn" match byte-for-byte.

use serde::{Deserialize, Serialize};

use crate::filter::TargetFilter;

/// One continuous change to an object's characteristics. CR 613.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Modification {
    /// CR 613.4b: layer 7c power modification.
    AddPower {
        value: i32,
    },
    AddToughness {
        value: i32,
    },
    /// CR 613.1f: layer 6 ability addition.
    AddKeyword {
        keyword: String,
    },
    RemoveKeyword {
        keyword: String,
    },
    /// CR 702.73a: the object is every creature type.
    AddAllCreatureTypes,
    /// CR 613.1c: layer 5 colour setting. An EMPTY list is colourless, which
    /// is the whole content of Devoid (CR 702.114a) — absence of colour is a
    /// value here, not a missing field.
    SetColor {
        colors: Vec<crate::filter::ManaColor>,
    },
}

/// CR 509.1b: a blocking restriction naming WHICH creatures cannot block.
///
/// Externally tagged, because the engine prints the data-carrying mode as an
/// object while every mode without data stays a bare string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StaticMode {
    Continuous,
    CantBlock,
    CantAttack,
    CantUntap,
    /// CR 509.1b: the object cannot be chosen as a blocker's target.
    CantBeBlocked,
    /// CR 508.1d: the object must be declared as an attacker if able.
    MustAttack,
    /// CR 509.1b with a restriction on the blocker.
    CantBeBlockedBy {
        filter: crate::filter::TargetFilter,
    },
}

/// CR 613.1: a game-state predicate that gates a continuous effect.
///
/// Typed rather than an untyped blob, for the same reason nothing else here is:
/// a condition the grammar cannot express must DECLINE, not lower into
/// something a consumer has to guess about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Condition {
    /// CR 400.1: at least one object matching `filter` exists.
    IsPresent {
        filter: TargetFilter,
    },
    Not {
        condition: Box<Condition>,
    },
    And {
        conditions: Vec<Condition>,
    },
    Or {
        conditions: Vec<Condition>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaticAbility {
    pub mode: StaticMode,
    pub affected: TargetFilter,
    pub modifications: Vec<Modification>,
    pub condition: Option<Condition>,
    pub affected_zone: Option<String>,
    pub effect_zone: Option<String>,
    pub active_zones: Vec<String>,
    pub characteristic_defining: bool,
    /// The predicate as printed, in infinitive form: "gain flying",
    /// "get +1/+1 and gain trample".
    pub description: Option<String>,
}

impl StaticAbility {
    pub fn continuous(affected: TargetFilter, modifications: Vec<Modification>) -> Self {
        Self {
            mode: StaticMode::Continuous,
            affected,
            modifications,
            condition: None,
            affected_zone: None,
            effect_zone: None,
            active_zones: Vec::new(),
            characteristic_defining: false,
            description: None,
        }
    }
}
