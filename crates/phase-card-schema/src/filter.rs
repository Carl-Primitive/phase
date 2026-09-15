//! What a clause acts on.

use serde::{Deserialize, Serialize};

/// A printed card type or subtype, held as the printed word.
///
/// Deliberately a string rather than a closed enum: Magic adds types and
/// subtypes every set, and a closed enum would make the schema's major version
/// move for a reason that is not a shape change.
pub type TypeName = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Controller {
    /// "you control"
    You,
    /// "an opponent controls"
    Opponent,
    /// Unstated: any controller.
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardZone {
    Battlefield,
    Graveyard,
    Hand,
    Library,
    Exile,
    Stack,
}

/// A predicate over objects.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ObjectFilter {
    /// Printed types the object must have, e.g. `["creature"]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub types: Vec<TypeName>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subtypes: Vec<TypeName>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub colors: Vec<String>,
    pub controller: Option<Controller>,
    pub zone: Option<CardZone>,
    /// "another", excluding the ability's own source.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub excludes_source: bool,
}

/// What a clause points at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    /// "any target": a creature, player, planeswalker or battle.
    AnyTarget,
    /// "target <filter>", chosen on announcement.
    Chosen { filter: ObjectFilter },
    /// "each <filter>" / "all <filter>", not chosen.
    Each { filter: ObjectFilter },
    /// The ability's own source.
    This,
    /// The permanent this Aura or Equipment is attached to.
    Attached,
    /// "you"
    You,
    /// "target player" / "target opponent"
    Player { controller: Controller, chosen: bool },
    /// "each opponent"
    EachOpponent,
}
