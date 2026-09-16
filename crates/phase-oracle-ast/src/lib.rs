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
pub mod replacement;
pub mod static_ability;
pub mod trigger;

pub use ability::{
    AbilityCondition, AbilityDefinition, AbilityKind, AbilityTag, ActivationRestriction, Duration,
    EffectSignal, MultiTargetSpec, PlayerScope, SubAbilityLink,
};
pub use cost::{
    AbilityCost, CounterMatch, CounterSelection, ManaCost, ManaShard, SacrificeCost,
    TapRequirement, TapRequirementKind,
};

pub use effect::{ChoiceTiming, CounterType, Effect, ManaProduced, TapScope, TapState, ZoneName};
pub use filter::{
    AttachmentKind, Comparator, ControllerRef, FilterProp, ManaColor, PtScope, PtStat,
    TargetFilter, TypeFilter, TypedFilter, Zone,
};
pub use qty::{Quantity, QuantityRef, StatScope};
pub use replacement::{Replacement, ReplacementEvent, ReplacementMode};

pub use static_ability::{Condition, Modification, StaticAbility, StaticMode};
pub use trigger::{DamageKindFilter, PhaseName, TriggerDefinition, TriggerMode};

use serde::{Deserialize, Serialize};

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
    /// CR 702.16: "protection from black". The quality protected from is the
    /// keyword's argument, so it is keyed the same way a cost is.
    Protection {
        #[serde(rename = "Protection")]
        quality: ProtectionQuality,
    },
    /// CR 702.124: the partner family. A DECK-CONSTRUCTION rule with no
    /// gameplay behaviour, which is why it can be hoisted as a bare entry while
    /// keywords like Storm and Exalted cannot.
    Partner {
        #[serde(rename = "Partner")]
        variant: PartnerVariant,
    },
    /// CR 702.122a: "Crew 2". The argument is a POWER threshold rather than a
    /// cost, so it carries its own shape.
    Crew {
        #[serde(rename = "Crew")]
        crew: CrewCost,
    },
    /// A keyword printed with a cost: "Flashback {1}{B}", "Morph {2}{U}".
    ///
    /// A one-entry map, because the engine keys the payload by the keyword's
    /// own name rather than tagging it. The payload SHAPE differs by keyword
    /// and is not something the printed text reveals — see [`KeywordCost`].
    Costed(std::collections::BTreeMap<String, KeywordCost>),
}

/// Which partner rule a card uses. CR 702.124.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "type")]
pub enum PartnerVariant {
    Generic,
    ChooseABackground,
    DoctorsCompanion,
    FriendsForever,
}

/// CR 702.122a: the total power that must be tapped to crew.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CrewCost {
    pub power: u32,
    /// Printed by the engine even when absent, so it is not skipped.
    pub once_per_turn: Option<bool>,
}

/// What a protection keyword protects from. CR 702.16e.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ProtectionQuality {
    Color(filter::ManaColor),
    Multicolored,
}

/// The payload a costed keyword carries.
///
/// Two families, and which one a keyword belongs to is a fact about the ENGINE
/// rather than about the card: Morph and Foretell carry a bare `ManaCost`,
/// while Flashback and Evoke wrap the same thing in a tagged envelope that can
/// also hold a non-mana cost. The printed text is identical either way, so the
/// family is looked up per keyword rather than inferred.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum KeywordCost {
    Bare(cost::ManaCost),
    Wrapped(WrappedKeywordCost),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum WrappedKeywordCost {
    Mana(cost::ManaCost),
    PayLife(u32),
}

/// CR 700.2: metadata for a modal spell or ability.
///
/// The modes themselves are ordinary abilities, one per bullet, living in the
/// card's `abilities` array. This struct only records how many of them the
/// controller picks — which is why `mode_count` and the array length must
/// agree, and why each mode's own `description` is null while their printed
/// text is repeated here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModalChoice {
    pub min_choices: usize,
    pub max_choices: usize,
    pub mode_count: usize,
    pub mode_descriptions: Vec<String>,
    pub allow_repeat_modes: bool,
    /// CR 700.2a: the controller, for every modal this grammar produces.
    pub chooser: TargetFilter,
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub replacements: Vec<Replacement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modal: Option<ModalChoice>,
}

impl CardOutput {
    pub fn is_empty(&self) -> bool {
        self.keywords.is_empty()
            && self.abilities.is_empty()
            && self.triggers.is_empty()
            && self.static_abilities.is_empty()
            && self.replacements.is_empty()
            && self.modal.is_none()
    }
}
