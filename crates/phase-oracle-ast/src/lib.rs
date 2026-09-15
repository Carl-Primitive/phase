//! Engine-shaped card definition AST.
//!
//! These types serialize byte-identically to the card definitions in
//! `data/card-data.json`, WITHOUT depending on `phase-engine`. That pairing is
//! the point of the crate:
//!
//! * matching the engine's shape exactly makes the new parser a like-for-like
//!   replacement, so a divergence is always a parser bug and never a format
//!   disagreement;
//! * not linking the engine keeps the edit-test loop under a second, where
//!   `cargo check -p phase-engine --all-targets` costs two minutes forty.
//!
//! Only the slice of the engine's vocabulary the grammar can currently produce
//! is mirrored, and there is deliberately no catch-all variant anywhere: text
//! the grammar cannot express DECLINES with a span rather than lowering into an
//! untyped blob. Coverage is therefore always measurable.

pub mod ability;
pub mod cost;
pub mod effect;
pub mod filter;
pub mod qty;
pub mod static_ability;
pub mod trigger;

pub use ability::{
    AbilityDefinition, AbilityKind, AbilityTag, ActivationRestriction, Duration, PlayerScope,
    SubAbilityLink,
};
pub use cost::{
    AbilityCost, CounterMatch, CounterSelection, ManaCost, ManaShard, SacrificeCost,
    TapRequirement, TapRequirementKind,
};
pub use effect::{ChoiceTiming, CounterType, Effect, ManaProduced, TapScope, TapState, ZoneName};
pub use filter::{
    AttachmentKind, Comparator, ControllerRef, FilterProp, ManaColor, TargetFilter, TypeFilter,
    TypedFilter, Zone,
};
pub use qty::{Quantity, QuantityRef, StatScope};
pub use static_ability::{Modification, StaticAbility, StaticMode};
pub use trigger::{DamageKindFilter, PhaseName, TriggerDefinition, TriggerMode};

use serde::Serialize;

/// Everything one card's Oracle text lowers to.
///
/// The buckets are the engine's, not a convenience of this crate: the engine
/// hoists bare keyword lines into `keywords`, puts activated and spell
/// abilities in `abilities`, and gives triggers their own array. Each is
/// omitted from the card record entirely when empty.
/// A keyword as the engine prints it in a card's `keywords` array.
///
/// Most print as a bare string, but a PARAMETERIZED keyword prints as an object
/// carrying its argument. Landwalk is the shape this grammar produces today:
/// "swampwalk" is not a keyword named Swampwalk, it is Landwalk of Swamp, and
/// flattening it to a string would lose the land type every consumer needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum Keyword {
    Simple(String),
    Landwalk {
        #[serde(rename = "Landwalk")]
        land_type: String,
    },
    /// CR 702.5: "Enchant creature" states what an Aura may legally be attached
    /// to, so the keyword carries a filter rather than standing alone.
    Enchant {
        #[serde(rename = "Enchant")]
        filter: TargetFilter,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CardOutput {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<Keyword>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub abilities: Vec<AbilityDefinition>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub triggers: Vec<TriggerDefinition>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub static_abilities: Vec<static_ability::StaticAbility>,
}

impl CardOutput {
    pub fn is_empty(&self) -> bool {
        self.keywords.is_empty()
            && self.abilities.is_empty()
            && self.triggers.is_empty()
            && self.static_abilities.is_empty()
    }
}
