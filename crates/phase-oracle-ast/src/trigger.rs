//! `TriggerDefinition`, mirroring the engine's serde shape.
//!
//! Sixteen fields are always printed, even at their defaults. A census over all
//! 19,392 triggers in `card-data.json` confirms that set exactly, and this
//! struct prints them in the same order.

use serde::{Deserialize, Serialize};

use crate::ability::AbilityDefinition;
use crate::effect::ZoneName;
use crate::filter::TargetFilter;

/// CR 603.2: the event a trigger watches for.
///
/// Serializes as a bare string for unit variants; the engine's parameterized
/// modes (for example `{"Planeswalked":{"role":"To"}}`) are externally tagged
/// and are not yet produced by this grammar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerMode {
    /// CR 603.6: enters-the-battlefield and dies triggers alike. Which one is
    /// determined by `origin`/`destination`, not by a separate mode.
    ChangesZone,
    /// CR 603.2: "At the beginning of <step>".
    Phase,
    Attacks,
    Blocks,
    YouAttack,
    BecomesBlocked,
    SpellCast,
    DamageDone,
    DamageReceived,
    CounterAdded,
    CounterRemoved,
    LeavesBattlefield,
    Taps,
    Drawn,
    Discarded,
    Sacrificed,
    BecomesTarget,
    LifeGained,
}

/// CR 500.1: which step or phase a `Phase` trigger fires in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhaseName {
    Untap,
    Upkeep,
    Draw,
    PreCombatMain,
    BeginCombat,
    DeclareAttackers,
    DeclareBlockers,
    CombatDamage,
    EndCombat,
    PostCombatMain,
    End,
    Cleanup,
}

/// CR 120.3: combat damage versus any damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DamageKindFilter {
    #[default]
    Any,
    Combat,
    NonCombat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggerDefinition {
    pub mode: TriggerMode,
    pub execute: AbilityDefinition,
    pub valid_card: Option<TargetFilter>,
    pub origin: Option<ZoneName>,
    pub destination: Option<ZoneName>,
    pub trigger_zones: Vec<ZoneName>,
    pub phase: Option<PhaseName>,
    /// CR 603.2c: "you may" on the trigger itself.
    pub optional: bool,
    pub damage_kind: DamageKindFilter,
    pub secondary: bool,
    pub valid_target: Option<TargetFilter>,
    pub valid_source: Option<TargetFilter>,
    /// The printed text of the whole trigger line, with `~` for the card name.
    pub description: Option<String>,
    pub constraint: Option<serde_json::Value>,
    /// CR 603.4: the intervening-if clause.
    pub condition: Option<serde_json::Value>,
    pub batched: bool,
}

impl TriggerDefinition {
    /// A trigger with every always-printed field at the value the engine
    /// prints by default, so a caller sets only what the Oracle text said.
    pub fn new(mode: TriggerMode, execute: AbilityDefinition) -> Self {
        Self {
            mode,
            execute,
            valid_card: None,
            origin: None,
            destination: None,
            // CR 603.6a: an ability triggers from the battlefield unless the
            // printed text puts its source somewhere else.
            trigger_zones: vec![ZoneName::Battlefield],
            phase: None,
            optional: false,
            damage_kind: DamageKindFilter::Any,
            secondary: false,
            valid_target: None,
            valid_source: None,
            description: None,
            constraint: None,
            condition: None,
            batched: false,
        }
    }
}
