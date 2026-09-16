//! Replacement effects. CR 614.
//!
//! A replacement does not resolve — it changes an event as that event happens —
//! so it has its own bucket rather than living among the abilities. The
//! commonest by far is "this land enters tapped", which the engine spells as a
//! `Moved` event whose execute sets the tap state.

use serde::{Deserialize, Serialize};

use crate::ability::AbilityDefinition;
use crate::effect::ZoneName;
use crate::filter::TargetFilter;
use crate::static_ability::Condition;

/// CR 614.1: the event being replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplacementEvent {
    /// An object changing zones, including entering the battlefield.
    Moved,
    DamageDone,
    Discard,
    TurnFaceUp,
}

/// CR 614.1b: whether the replacement is applied automatically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ReplacementMode {
    Mandatory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Replacement {
    pub event: ReplacementEvent,
    pub execute: AbilityDefinition,
    pub mode: ReplacementMode,
    pub valid_card: Option<TargetFilter>,
    pub description: Option<String>,
    pub condition: Option<Condition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_zone: Option<ZoneName>,
}
