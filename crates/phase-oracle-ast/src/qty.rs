//! Counts, and references to counts that are not knowable until resolution.
//!
//! Mirrors `phase_engine::types::ability::{QuantityExpr, QuantityRef}`.
//!
//! The two-level split is deliberate and belongs to the engine, not to this
//! mirror: `Fixed` is a constant, `Ref` wraps a REFERENCE to a dynamic game
//! value. Flattening them into one enum was tried during the spike and was
//! worse — it forces every consumer to handle a constant and a game-state
//! lookup through the same arm.

use serde::{Deserialize, Serialize};

use crate::filter::{TargetFilter, Zone};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Quantity {
    Ref { qty: QuantityRef },
    Fixed { value: i32 },
    Offset { inner: Box<Quantity>, offset: i32 },
    Multiply { factor: i32, inner: Box<Quantity> },
}

impl Quantity {
    pub fn fixed(v: i32) -> Self {
        Quantity::Fixed { value: v }
    }

    /// The announced value of X. CR 107.3.
    pub fn variable_x() -> Self {
        Quantity::Ref {
            qty: QuantityRef::Variable {
                name: "X".to_string(),
            },
        }
    }
}

/// Where a dynamic count is read from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum QuantityRef {
    Variable {
        name: String,
    },
    /// "that many" — the amount established by the event that caused this ability.
    EventContextAmount,
    ObjectCount {
        filter: TargetFilter,
    },
    HandSize {
        player: TargetFilter,
    },
    LifeTotal {
        player: TargetFilter,
    },
    Power {
        scope: StatScope,
    },
    Toughness {
        scope: StatScope,
    },
    ZoneCardCount {
        zone: Zone,
        card_types: Vec<String>,
        scope: String,
    },
    CostXPaid,
}

/// Which object a power/toughness reference reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StatScope {
    Source,
    Target,
    EventSource,
}
