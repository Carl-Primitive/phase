//! `AbilityDefinition`, mirroring the engine's hand-written `Serialize`.
//!
//! Field ORDER here is not cosmetic: it reproduces `AbilityDefinitionRepr` in
//! `phase_engine::types::ability`, so the printed JSON is byte-identical rather
//! than merely semantically equal.
//!
//! Eleven fields are always printed even at their defaults (`cost`,
//! `sub_ability`, `duration`, `description`, `target_prompt`, `condition`,
//! `optional_targeting`, `optional`, `forward_result`, plus `kind` and
//! `effect`). Everything else is skipped at its default. A census over all
//! 61,631 abilities in `card-data.json` confirms exactly that split.

use serde::{Deserialize, Serialize};

use crate::cost::AbilityCost;
use crate::effect::Effect;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AbilityKind {
    #[default]
    Spell,
    Activated,
    Database,
}

/// CR 608.2c: how a continuation links to the ability it hangs off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SubAbilityLink {
    /// Part of the parent's action ("…, then shuffle"). The engine's default,
    /// and therefore omitted from JSON.
    #[default]
    ContinuationStep,
    /// An independent following instruction, printed as its own sentence.
    SequentialSibling,
}

impl SubAbilityLink {
    fn is_continuation(&self) -> bool {
        matches!(self, SubAbilityLink::ContinuationStep)
    }
}

/// CR 602.5d and friends: when an activated ability may be activated.
/// CR 608.2: a predicate checked as this ability resolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AbilityCondition {
    /// CR 608.2d: "If you do, ..." — the preceding OPTIONAL effect was
    /// actually performed. A signal about what happened during this same
    /// resolution, not a fact about the game state.
    EffectOutcome { signal: EffectSignal },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectSignal {
    OptionalEffectPerformed,
}

/// CR 601.2c + CR 115.6: how many targets a slot allows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiTargetSpec {
    pub min: usize,
    pub max: crate::qty::Quantity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AbilityTag {
    Equip,
    Fortify,
    Reconfigure,
    /// CR 702.142b and friends: keyword-ish labels printed like ability words
    /// but naming a class of ability that other cards refer to.
    Boast,
    Exhaust,
    PowerUp,
    Cycling,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ActivationRestriction {
    AsSorcery,
    OnlyOnceEachTurn,
    OnlyOnce,
    DuringYourTurn,
    DuringYourUpkeep,
}

/// How long a continuous effect lasts. CR 611.2.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Duration {
    UntilEndOfTurn,
    UntilEndOfCombat,
    Permanent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbilityDefinition {
    pub kind: AbilityKind,
    pub effect: Box<Effect>,
    pub cost: Option<AbilityCost>,
    pub sub_ability: Option<Box<AbilityDefinition>>,
    pub duration: Option<Duration>,
    /// The printed text of the whole ability line, with the card's own name
    /// rendered as `~`. Only the OUTERMOST definition of a line carries it;
    /// every `sub_ability` below it has `None`.
    pub description: Option<String>,
    pub target_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub activation_restrictions: Vec<ActivationRestriction>,
    /// CR 702.6b: which keyword this ability came from, for effects that refer
    /// to abilities by keyword class.
    /// CR 602.1: the zone this ability may be activated from. Absent means the
    /// battlefield, which is why Cycling has to say `Hand` explicitly.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activation_zone: Option<crate::filter::Zone>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ability_tag: Option<AbilityTag>,
    /// Always printed, even when null.
    pub condition: Option<AbilityCondition>,
    pub optional_targeting: bool,
    /// CR 608.2d: "You may …".
    pub optional: bool,
    /// CR 700.2: modal metadata, when this ability pauses for a mode choice.
    /// The modes themselves live in `mode_abilities`, so the two are always set
    /// together.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modal: Option<crate::ModalChoice>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mode_abilities: Vec<AbilityDefinition>,
    pub forward_result: bool,
    /// CR 101.4: when set, the effect is performed once per matching player,
    /// each becoming the acting player in APNAP order. "Each opponent mills a
    /// card" is a controller-shaped `Mill` iterated over opponents, NOT a
    /// mill whose target is the opponents.
    /// CR 115.6: "up to one target creature" — the slot may legally take none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multi_target: Option<MultiTargetSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub player_scope: Option<PlayerScope>,
    #[serde(skip_serializing_if = "SubAbilityLink::is_continuation")]
    pub sub_link: SubAbilityLink,
    /// CR 605.1a: computed, not parsed. The engine appends it when the ability
    /// produces mana and needs no target, because that is what decides whether
    /// it can be activated without using the stack.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub is_mana_ability: bool,
}

/// Which players an iterated effect runs for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PlayerScope {
    Opponent,
    All,
    TriggeringPlayer,
}

impl AbilityDefinition {
    /// A bare definition carrying `effect` and nothing else. Every field the
    /// engine always prints is set to the value the engine prints at default,
    /// so a caller only overrides what the Oracle text actually said.
    pub fn new(kind: AbilityKind, effect: Effect) -> Self {
        Self {
            kind,
            effect: Box::new(effect),
            cost: None,
            sub_ability: None,
            duration: None,
            description: None,
            target_prompt: None,
            activation_restrictions: Vec::new(),
            activation_zone: None,
            ability_tag: None,
            condition: None,
            optional_targeting: false,
            optional: false,
            modal: None,
            mode_abilities: Vec::new(),
            forward_result: false,
            multi_target: None,
            player_scope: None,
            sub_link: SubAbilityLink::ContinuationStep,
            is_mana_ability: false,
        }
    }

    /// Recompute the mana-ability rider from the effect chain.
    ///
    /// CR 605.1a: an activated or spell ability that could add mana and has no
    /// target is a mana ability. Derived rather than parsed, so it can never
    /// disagree with the effect it describes.
    pub fn refresh_mana_ability(&mut self) {
        self.is_mana_ability = matches!(*self.effect, Effect::Mana { .. });
    }

    pub fn spell(effect: Effect) -> Self {
        Self::new(AbilityKind::Spell, effect)
    }

    /// Hang `next` off the end of this definition's `sub_ability` chain.
    ///
    /// Chaining at the TAIL rather than the head is what makes printed order
    /// and resolution order agree (CR 608.2c, "written order").
    pub fn chain(&mut self, next: AbilityDefinition) {
        let mut cur = self;
        while cur.sub_ability.is_some() {
            cur = cur.sub_ability.as_mut().expect("checked Some");
        }
        cur.sub_ability = Some(Box::new(next));
    }
}
