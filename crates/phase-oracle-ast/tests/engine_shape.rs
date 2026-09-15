//! The mirror is faithful, pinned against real `card-data.json` output.
//!
//! Each case is a verbatim copy of what `phase-engine` prints for that card.
//! If the engine's shape drifts, these fail here rather than silently costing
//! exact-match points in the corpus diff, where a shape bug and a grammar bug
//! look identical.

use phase_oracle_ast::*;
use serde_json::json;

fn creature() -> TargetFilter {
    TargetFilter::of_type(TypeFilter::Creature)
}

#[test]
fn doom_blade_destroy_spell() {
    let mut t = TypedFilter::of(TypeFilter::Creature);
    t.properties.push(FilterProp::NotColor {
        color: ManaColor::Black,
    });
    let mut a = AbilityDefinition::spell(Effect::Destroy {
        target: TargetFilter::Typed(t),
        cant_regenerate: false,
    });
    a.description = Some("Destroy target nonblack creature.".into());

    assert_eq!(
        serde_json::to_value(&a).unwrap(),
        json!({
            "kind": "Spell",
            "effect": {
                "type": "Destroy",
                "target": {
                    "type": "Typed",
                    "type_filters": ["Creature"],
                    "controller": null,
                    "properties": [{"type": "NotColor", "color": "Black"}]
                },
                "cant_regenerate": false
            },
            "cost": null,
            "sub_ability": null,
            "duration": null,
            "description": "Destroy target nonblack creature.",
            "target_prompt": null,
            "condition": null,
            "optional_targeting": false,
            "optional": false,
            "forward_result": false
        })
    );
}

#[test]
fn prodigal_sorcerer_activated_tap_ability() {
    let mut a = AbilityDefinition::new(
        AbilityKind::Activated,
        Effect::DealDamage {
            amount: Quantity::fixed(1),
            target: TargetFilter::Any,
        },
    );
    a.cost = Some(AbilityCost::Tap);
    a.description = Some("{T}: ~ deals 1 damage to any target.".into());

    assert_eq!(
        serde_json::to_value(&a).unwrap(),
        json!({
            "kind": "Activated",
            "effect": {
                "type": "DealDamage",
                "amount": {"type": "Fixed", "value": 1},
                "target": {"type": "Any"}
            },
            "cost": {"type": "Tap"},
            "sub_ability": null,
            "duration": null,
            "description": "{T}: ~ deals 1 damage to any target.",
            "target_prompt": null,
            "condition": null,
            "optional_targeting": false,
            "optional": false,
            "forward_result": false
        })
    );
}

#[test]
fn icy_manipulator_composite_cost() {
    let cost = AbilityCost::Composite {
        costs: vec![
            AbilityCost::Mana {
                cost: ManaCost::Cost {
                    shards: vec![],
                    generic: 1,
                },
            },
            AbilityCost::Tap,
        ],
    };
    assert_eq!(
        serde_json::to_value(&cost).unwrap(),
        json!({
            "type": "Composite",
            "costs": [
                {"type": "Mana", "cost": {"type": "Cost", "shards": [], "generic": 1}},
                {"type": "Tap"}
            ]
        })
    );
}

#[test]
fn shivan_dragon_pump_with_duration() {
    let mut a = AbilityDefinition::new(
        AbilityKind::Activated,
        Effect::Pump {
            power: Quantity::fixed(1),
            toughness: Quantity::fixed(0),
            target: TargetFilter::SelfRef,
        },
    );
    a.cost = Some(AbilityCost::Mana {
        cost: ManaCost::Cost {
            shards: vec![ManaShard::Red],
            generic: 0,
        },
    });
    a.duration = Some(Duration::UntilEndOfTurn);
    a.description = Some("{R}: ~ gets +1/+0 until end of turn.".into());

    let v = serde_json::to_value(&a).unwrap();
    assert_eq!(
        v["cost"],
        json!({"type": "Mana", "cost": {"type": "Cost", "shards": ["Red"], "generic": 0}})
    );
    assert_eq!(v["duration"], json!("UntilEndOfTurn"));
    assert_eq!(v["effect"]["target"], json!({"type": "SelfRef"}));
}

#[test]
fn elvish_visionary_etb_trigger() {
    let exec = AbilityDefinition::spell(Effect::Draw {
        count: Quantity::fixed(1),
        target: TargetFilter::Controller,
    });
    let mut t = TriggerDefinition::new(TriggerMode::ChangesZone, exec);
    t.valid_card = Some(TargetFilter::SelfRef);
    t.destination = Some(ZoneName::Battlefield);
    t.description = Some("When ~ enters, draw a card.".into());

    assert_eq!(
        serde_json::to_value(&t).unwrap(),
        json!({
            "mode": "ChangesZone",
            "execute": {
                "kind": "Spell",
                "effect": {
                    "type": "Draw",
                    "count": {"type": "Fixed", "value": 1},
                    "target": {"type": "Controller"}
                },
                "cost": null,
                "sub_ability": null,
                "duration": null,
                "description": null,
                "target_prompt": null,
                "condition": null,
                "optional_targeting": false,
                "optional": false,
                "forward_result": false
            },
            "valid_card": {"type": "SelfRef"},
            "origin": null,
            "destination": "Battlefield",
            "trigger_zones": ["Battlefield"],
            "phase": null,
            "optional": false,
            "damage_kind": "Any",
            "secondary": false,
            "valid_target": null,
            "valid_source": null,
            "description": "When ~ enters, draw a card.",
            "constraint": null,
            "condition": null,
            "batched": false
        })
    );
}

#[test]
fn ball_lightning_end_step_phase_trigger() {
    let exec = AbilityDefinition::spell(Effect::Sacrifice {
        target: TargetFilter::SelfRef,
        count: Quantity::fixed(1),
    });
    let mut t = TriggerDefinition::new(TriggerMode::Phase, exec);
    t.phase = Some(PhaseName::End);
    t.description = Some("At the beginning of the end step, sacrifice ~.".into());

    let v = serde_json::to_value(&t).unwrap();
    assert_eq!(v["mode"], json!("Phase"));
    assert_eq!(v["phase"], json!("End"));
    assert_eq!(v["valid_card"], json!(null));
    assert_eq!(v["trigger_zones"], json!(["Battlefield"]));
}

#[test]
fn sub_ability_chains_at_the_tail_in_written_order() {
    // CR 608.2c: printed order is resolution order, so a chain appends.
    let mut a = AbilityDefinition::spell(Effect::Shuffle {
        target: TargetFilter::Controller,
    });
    a.chain(AbilityDefinition::spell(Effect::Draw {
        count: Quantity::fixed(1),
        target: TargetFilter::Controller,
    }));
    a.chain(AbilityDefinition::spell(Effect::Scry {
        count: Quantity::fixed(2),
        target: TargetFilter::Controller,
    }));

    let v = serde_json::to_value(&a).unwrap();
    assert_eq!(v["effect"]["type"], "Shuffle");
    assert_eq!(v["sub_ability"]["effect"]["type"], "Draw");
    assert_eq!(v["sub_ability"]["sub_ability"]["effect"]["type"], "Scry");
}

#[test]
fn sequential_sibling_link_is_printed_but_continuation_is_not() {
    // The engine omits the default link and prints the sentence-boundary one.
    let mut a = AbilityDefinition::spell(Effect::Shuffle {
        target: TargetFilter::Controller,
    });
    assert!(serde_json::to_value(&a).unwrap().get("sub_link").is_none());
    a.sub_link = SubAbilityLink::SequentialSibling;
    assert_eq!(
        serde_json::to_value(&a).unwrap()["sub_link"],
        "SequentialSibling"
    );
}

#[test]
fn a_player_shaped_typed_filter_has_no_type_constraint() {
    // CR 109.1 + CR 102.1: a player is not an object, so the empty
    // `type_filters` list is the ONLY spelling that reaches the player axis.
    assert_eq!(
        serde_json::to_value(TargetFilter::Typed(TypedFilter::player(
            ControllerRef::Opponent
        )))
        .unwrap(),
        json!({
            "type": "Typed",
            "type_filters": [],
            "controller": "Opponent",
            "properties": []
        })
    );
    assert!(!matches!(creature(), TargetFilter::None));
}

#[test]
fn gain_life_omits_the_player_field_when_the_subject_is_the_controller() {
    // Absence encodes "you". Reproduced deliberately; flagged for a later
    // format proposal rather than fixed here.
    let mine = Effect::GainLife {
        amount: Quantity::fixed(3),
        player: None,
    };
    assert_eq!(
        serde_json::to_value(&mine).unwrap(),
        json!({"type": "GainLife", "amount": {"type": "Fixed", "value": 3}})
    );
    let theirs = Effect::GainLife {
        amount: Quantity::fixed(3),
        player: Some(TargetFilter::Player),
    };
    assert_eq!(
        serde_json::to_value(&theirs).unwrap(),
        json!({
            "type": "GainLife",
            "amount": {"type": "Fixed", "value": 3},
            "player": {"type": "Player"}
        })
    );
}

#[test]
fn exile_is_spelled_as_a_zone_change() {
    let e = Effect::ChangeZone {
        origin: None,
        destination: ZoneName::Exile,
        target: creature(),
        owner_library: false,
        enter_transformed: false,
        enter_tapped: false,
        enters_attacking: false,
    };
    assert_eq!(
        serde_json::to_value(&e).unwrap(),
        json!({
            "type": "ChangeZone",
            "origin": null,
            "destination": "Exile",
            "target": {
                "type": "Typed",
                "type_filters": ["Creature"],
                "controller": null,
                "properties": []
            },
            "owner_library": false,
            "enter_transformed": false,
            "enter_tapped": false,
            "enters_attacking": false
        })
    );
}

#[test]
fn a_multi_type_target_is_a_disjunction_of_typed_filters() {
    // "target artifact, creature, or land" — Icy Manipulator.
    let e = TargetFilter::Or {
        filters: vec![
            TargetFilter::of_type(TypeFilter::Artifact),
            TargetFilter::of_type(TypeFilter::Creature),
            TargetFilter::of_type(TypeFilter::Land),
        ],
    };
    let v = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "Or");
    assert_eq!(v["filters"].as_array().unwrap().len(), 3);
    assert_eq!(v["filters"][0]["type_filters"], json!(["Artifact"]));
}

#[test]
fn a_subtype_prints_as_an_object_and_a_core_type_as_a_bare_string() {
    assert_eq!(
        serde_json::to_value(TypeFilter::Creature).unwrap(),
        json!("Creature")
    );
    assert_eq!(
        serde_json::to_value(TypeFilter::Subtype("Elf".into())).unwrap(),
        json!({"Subtype": "Elf"})
    );
    assert_eq!(
        serde_json::to_value(TypeFilter::Non(Box::new(TypeFilter::Land))).unwrap(),
        json!({"Non": "Land"})
    );
}

#[test]
fn counter_types_print_as_flat_strings() {
    assert_eq!(
        serde_json::to_value(CounterType::Plus1Plus1).unwrap(),
        json!("P1P1")
    );
    assert_eq!(
        serde_json::to_value(CounterType::Minus1Minus1).unwrap(),
        json!("M1M1")
    );
    assert_eq!(
        serde_json::to_value(CounterType::Named("charge".into())).unwrap(),
        json!("charge")
    );
}

#[test]
fn x_is_a_reference_not_a_constant() {
    // The engine's two-level QuantityExpr is correct and the mirror keeps it:
    // a constant and a lookup of a live game value are not the same thing.
    assert_eq!(
        serde_json::to_value(Quantity::variable_x()).unwrap(),
        json!({"type": "Ref", "qty": {"type": "Variable", "name": "X"}})
    );
    assert_eq!(
        serde_json::to_value(Quantity::fixed(2)).unwrap(),
        json!({"type": "Fixed", "value": 2})
    );
}

#[test]
fn set_tap_state_carries_scope_and_state_separately() {
    let e = Effect::SetTapState {
        target: creature(),
        scope: TapScope::Single,
        state: TapState::Tap,
    };
    let v = serde_json::to_value(&e).unwrap();
    assert_eq!(v["scope"], json!({"type": "Single"}));
    assert_eq!(v["state"], json!({"type": "Tap"}));
}
