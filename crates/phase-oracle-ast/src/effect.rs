//! What an ability does, mirroring `phase_engine::types::ability::Effect`.
//!
//! Only the effect families the grammar can currently produce are mirrored.
//! There is deliberately no catch-all variant: a clause the grammar cannot
//! express DECLINES with a span, rather than lowering into an untyped blob that
//! a consumer would have to guess about.

use serde::{Deserialize, Serialize};

use crate::filter::TargetFilter;
use crate::qty::Quantity;

/// CR 122.1: a counter kind. Serializes as a flat string so it can be a JSON
/// map key in the engine's `HashMap<CounterType, u32>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CounterType {
    Plus1Plus1,
    Minus1Minus1,
    Loyalty,
    Named(String),
}

impl CounterType {
    pub fn key(&self) -> &str {
        match self {
            CounterType::Plus1Plus1 => "P1P1",
            CounterType::Minus1Minus1 => "M1M1",
            CounterType::Loyalty => "loyalty",
            CounterType::Named(n) => n,
        }
    }
}

impl Serialize for CounterType {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.key())
    }
}

impl<'de> Deserialize<'de> for CounterType {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "P1P1" => CounterType::Plus1Plus1,
            "M1M1" => CounterType::Minus1Minus1,
            "loyalty" => CounterType::Loyalty,
            _ => CounterType::Named(s),
        })
    }
}

/// CR 601.2c vs CR 608.2d: WHEN an object choice is made.
///
/// A target is chosen as the spell or ability is put on the stack; an untargeted
/// instruction ("return a nonland permanent you control") chooses while it
/// resolves. The engine prints this only for the resolution case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChoiceTiming {
    AtResolution,
}

/// CR 701.20a / CR 701.21a: tap or untap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TapState {
    Tap,
    Untap,
}

/// How many objects a tap/untap instruction reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TapScope {
    Single,
    All,
}

/// CR 400.1: a zone an object moves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZoneName {
    Battlefield,
    Graveyard,
    Hand,
    Library,
    Exile,
    Stack,
    Command,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Effect {
    /// CR 701.7a. `cant_regenerate` carries the "can't be regenerated" rider.
    Destroy {
        target: TargetFilter,
        cant_regenerate: bool,
    },
    /// The mass-scope sibling of `Destroy`. The engine names scope in the
    /// variant rather than carrying it on the target; that is a wart this
    /// mirror reproduces rather than fixes, because changing shape and changing
    /// parser at once makes a regression impossible to attribute.
    DestroyAll {
        target: TargetFilter,
        cant_regenerate: bool,
    },
    /// CR 120: damage. `amount` may be dynamic.
    DealDamage {
        amount: Quantity,
        target: TargetFilter,
    },
    DamageAll {
        amount: Quantity,
        target: TargetFilter,
    },
    /// Damage dealt to a class of PLAYERS. Distinct from `DamageAll`, which
    /// ranges over objects: CR 102.1 makes a player not an object, so the two
    /// cannot share a target slot.
    DamageEachPlayer {
        amount: Quantity,
        player_filter: crate::ability::PlayerScope,
    },
    /// CR 121.
    Draw {
        count: Quantity,
        target: TargetFilter,
    },
    /// CR 119.3. `player` is OMITTED when the subject is the ability's
    /// controller — absence encodes "you". That is the engine's convention and
    /// the one real correctness hazard in the format; it is reproduced here
    /// exactly and flagged for a later format proposal, not fixed in passing.
    GainLife {
        amount: Quantity,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        player: Option<TargetFilter>,
    },
    /// CR 119.4. Note the field is `target`, not `player`: the two life effects
    /// disagree in the engine and the mirror follows the engine.
    LoseLife {
        amount: Quantity,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<TargetFilter>,
    },
    /// CR 613.4b: a power/toughness modification from a resolved effect.
    Pump {
        power: Quantity,
        toughness: Quantity,
        target: TargetFilter,
    },
    PumpAll {
        power: Quantity,
        toughness: Quantity,
        target: TargetFilter,
    },
    /// CR 122.
    PutCounter {
        counter_type: CounterType,
        count: Quantity,
        target: TargetFilter,
    },
    PutCounterAll {
        counter_type: CounterType,
        count: Quantity,
        target: TargetFilter,
    },
    /// CR 701.20 / CR 701.21.
    SetTapState {
        target: TargetFilter,
        scope: TapScope,
        state: TapState,
    },
    /// CR 701.16a.
    Sacrifice {
        target: TargetFilter,
        count: Quantity,
    },
    /// CR 701.8a.
    Discard {
        count: Quantity,
        target: TargetFilter,
    },
    /// CR 701.13a.
    Mill {
        count: Quantity,
        target: TargetFilter,
        destination: ZoneName,
    },
    /// CR 701.18a.
    Scry {
        count: Quantity,
        target: TargetFilter,
    },
    /// CR 701.19a.
    Surveil {
        count: Quantity,
        target: TargetFilter,
    },
    /// CR 701.22a.
    Shuffle { target: TargetFilter },
    /// "Return to its owner's hand". `destination` is `None` for the plain
    /// hand-bounce; the engine reserves the field for the library variants.
    Bounce {
        target: TargetFilter,
        destination: Option<ZoneName>,
        /// CR 608.2d: present only when the object is chosen during
        /// resolution rather than targeted on announcement.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selection: Option<ChoiceTiming>,
    },
    /// The mass form carries NO destination field. That asymmetry with
    /// `Bounce` is the engine's printed shape, verified against the corpus.
    BounceAll { target: TargetFilter },
    /// CR 701.5a. Exile is spelled as a zone change, not as its own variant.
    ChangeZone {
        origin: Option<ZoneName>,
        destination: ZoneName,
        target: TargetFilter,
        owner_library: bool,
        enter_transformed: bool,
        enter_tapped: bool,
        enters_attacking: bool,
    },
    /// The mass form carries ONLY the four fields the engine prints for it.
    /// The battlefield-entry riders that `ChangeZone` has are absent, verified
    /// against every `ChangeZoneAll` in the corpus.
    ChangeZoneAll {
        origin: Option<ZoneName>,
        destination: ZoneName,
        target: TargetFilter,
    },
    /// CR 701.5a: counter a spell or ability.
    Counter { target: TargetFilter },
    /// A spell or ability that creates a continuous effect. CR 611.
    ///
    /// This is how the engine spells "target creature gains flying until end
    /// of turn": not as a keyword-granting effect, but as a `StaticAbility`
    /// carried in a wrapper whose `target` holds the chosen object and whose
    /// `affected` points back at it through `ParentTarget`.
    GenericEffect {
        static_abilities: Vec<crate::static_ability::StaticAbility>,
        duration: Option<crate::ability::Duration>,
        target: Option<TargetFilter>,
    },
    /// CR 701.15a.
    Regenerate { target: TargetFilter },
    /// CR 701.3a: attach this permanent to another. The Equip and Fortify
    /// keyword abilities lower to this.
    Attach { target: TargetFilter },
    /// CR 701.19a: search a library for cards matching a filter.
    ///
    /// `target_player` is absent for the common "search YOUR library" form; the
    /// engine only prints it when someone else's library is searched.
    SearchLibrary {
        filter: TargetFilter,
        count: Quantity,
        reveal: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_player: Option<TargetFilter>,
    },
    /// CR 701.20a.
    GainControl { target: TargetFilter },
    /// CR 122.2.
    RemoveCounter {
        counter_type: CounterType,
        count: Quantity,
        target: TargetFilter,
    },
    /// CR 701.28a.
    Transform {
        target: TargetFilter,
        scope: TapScope,
    },
    /// CR 605: a mana ability's production.
    Mana { produced: ManaProduced },
    /// CR 111: token creation.
    Token {
        name: String,
        power: Quantity,
        toughness: Quantity,
        /// Core types first, then subtypes, in the engine's printed order.
        types: Vec<String>,
        colors: Vec<crate::filter::ManaColor>,
        keywords: Vec<String>,
        tapped: bool,
        count: Quantity,
        owner: TargetFilter,
        enters_attacking: bool,
    },
}

/// What a mana ability adds. CR 106.1.
///
/// `Fixed` lists the exact symbols; `Colorless` is a COUNT because `{C}{C}` is
/// two of one thing rather than two different things; `AnyOneColor` defers the
/// choice to the player and carries the colours they may pick from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ManaProduced {
    Fixed {
        colors: Vec<crate::filter::ManaColor>,
    },
    Colorless {
        count: Quantity,
    },
    AnyOneColor {
        count: Quantity,
        color_options: Vec<crate::filter::ManaColor>,
    },
}
