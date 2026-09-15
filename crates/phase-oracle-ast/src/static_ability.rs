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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StaticMode {
    Continuous,
    CantBlock,
    CantAttack,
    CantUntap,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaticAbility {
    pub mode: StaticMode,
    pub affected: TargetFilter,
    pub modifications: Vec<Modification>,
    pub condition: Option<serde_json::Value>,
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
