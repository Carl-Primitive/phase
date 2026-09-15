//! Object and player predicates, mirroring `phase_engine::types::ability`.
//!
//! Every type here reproduces the engine's serde shape exactly: same tag, same
//! field names, same field order, same `skip_serializing_if`. That contract is
//! verified not by inspection but by JSON comparison against `card-data.json`.
//!
//! The crate deliberately does NOT depend on `phase-engine`: the edit-test loop
//! over the grammar must stay under a second, and linking the engine costs two
//! and a half minutes.

use serde::{Deserialize, Serialize};

use crate::qty::Quantity;

/// CR 205: a type-line constraint.
///
/// Externally tagged, so a unit variant prints as a bare string (`"Creature"`)
/// while a newtype variant prints as an object (`{"Subtype":"Elf"}`). That
/// asymmetry is the engine's printed shape, not an artifact of this mirror.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeFilter {
    Creature,
    Land,
    Artifact,
    Enchantment,
    Instant,
    Sorcery,
    Planeswalker,
    Battle,
    Kindred,
    Permanent,
    Card,
    Any,
    /// CR 205.2a: "noncreature" / "non-Human".
    Non(Box<TypeFilter>),
    /// CR 205.3: a printed subtype.
    Subtype(String),
    /// CR 608.2b: "creature or enchantment".
    AnyOf(Vec<TypeFilter>),
}

/// Who controls the object, relative to the ability's controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControllerRef {
    You,
    Opponent,
    TargetPlayer,
    TargetOpponent,
    EachPlayer,
    SourceController,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Comparator {
    GT,
    LT,
    GE,
    LE,
    EQ,
    NE,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttachmentKind {
    Aura,
    Equipment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManaColor {
    White,
    Blue,
    Black,
    Red,
    Green,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Zone {
    Battlefield,
    Graveyard,
    Hand,
    Library,
    Exile,
    Stack,
    Command,
}

/// CR 205 / CR 700: a non-type restriction on an object.
///
/// Only the properties the grammar can currently produce are mirrored. A
/// property the grammar cannot express is NOT modelled as an escape hatch: the
/// clause declines instead, so a gap stays attributable to a production rather
/// than disappearing into an untyped blob.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum FilterProp {
    Token,
    NonToken,
    Attacking {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        defender: Option<ControllerRef>,
    },
    Blocking,
    Unblocked,
    Tapped,
    Untapped,
    WithKeyword {
        value: String,
    },
    WithoutKeyword {
        value: String,
    },
    Cmc {
        comparator: Comparator,
        value: Quantity,
    },
    InZone {
        zone: Zone,
    },
    Owned {
        controller: ControllerRef,
    },
    EnchantedBy,
    EquippedBy,
    /// CR 303.4 / CR 301.5: the object has SOME attachment of this kind, as
    /// opposed to being the specific host of this source. "Enchanted creatures
    /// you control" (plural) is this; "enchanted creature" (the Aura's own
    /// host) is `EnchantedBy`.
    HasAttachment {
        kind: AttachmentKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        controller: Option<ControllerRef>,
    },
    /// "another" — excludes the ability's own source. CR 109.5.
    Another,
    HasColor {
        color: ManaColor,
    },
    NotColor {
        color: ManaColor,
    },
    HasSupertype {
        value: String,
    },
    NotSupertype {
        value: String,
    },
    Named {
        name: String,
    },
}

/// The `Typed` payload. serde flattens it into the tagged object, because the
/// engine declares the variant as a newtype over this struct.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypedFilter {
    #[serde(default)]
    pub type_filters: Vec<TypeFilter>,
    #[serde(default)]
    pub controller: Option<ControllerRef>,
    #[serde(default)]
    pub properties: Vec<FilterProp>,
}

impl TypedFilter {
    pub fn of(t: TypeFilter) -> Self {
        Self {
            type_filters: vec![t],
            ..Self::default()
        }
    }

    /// A player-shaped `Typed` filter.
    ///
    /// CR 109.1 + CR 102.1: a player is not an object, so an EMPTY
    /// `type_filters` is the only spelling that admits the player axis. This
    /// constructor is the single place that encodes that.
    pub fn player(controller: ControllerRef) -> Self {
        Self {
            type_filters: Vec::new(),
            controller: Some(controller),
            properties: Vec::new(),
        }
    }
}

/// What a clause points at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TargetFilter {
    None,
    Any,
    Player,
    Controller,
    Opponent,
    SelfRef,
    AttachedTo,
    ParentTarget,
    TriggeringSource,
    /// CR 603.2: the player named by the event that caused this trigger —
    /// "that player" after "whenever ~ deals damage to an opponent".
    TriggeringPlayer,
    Another,
    Single,
    /// CR 111.1 + CR 601: an object on the stack that is a spell. "Target
    /// spell" names this, NOT a card type — a spell is a zone-dependent object
    /// and has no type-line spelling.
    StackSpell,
    StackAbility,
    Typed(TypedFilter),
    Not {
        filter: Box<TargetFilter>,
    },
    Or {
        filters: Vec<TargetFilter>,
    },
    And {
        filters: Vec<TargetFilter>,
    },
}

impl TargetFilter {
    pub fn typed(f: TypedFilter) -> Self {
        TargetFilter::Typed(f)
    }

    pub fn of_type(t: TypeFilter) -> Self {
        TargetFilter::Typed(TypedFilter::of(t))
    }
}
