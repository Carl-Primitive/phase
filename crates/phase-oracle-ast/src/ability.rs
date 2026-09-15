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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivationRestriction {
    AsSorcery,
    OncePerTurn,
    OnlyDuringYourTurn,
    OnlyDuringCombat,
    OnlyDuringUpkeep,
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
    /// Always printed, even when null.
    pub condition: Option<serde_json::Value>,
    pub optional_targeting: bool,
    /// CR 608.2d: "You may …".
    pub optional: bool,
    pub forward_result: bool,
    #[serde(skip_serializing_if = "SubAbilityLink::is_continuation")]
    pub sub_link: SubAbilityLink,
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
            condition: None,
            optional_targeting: false,
            optional: false,
            forward_result: false,
            sub_link: SubAbilityLink::ContinuationStep,
        }
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
