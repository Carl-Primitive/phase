//! Grammar tests, stated against the ENGINE's printed JSON.
//!
//! Every expectation here is a verbatim copy of what `phase-engine` produces
//! for that card in `data/card-data.json`. That is deliberate: the goal is a
//! like-for-like replacement, so a test that passed against a prettier shape of
//! our own would be testing the wrong thing.
//!
//! Tests name a CLASS and use one card as its witness, not the other way round.

use phase_oracle_parse::parse_card;
use serde_json::{json, Value};

/// Parse a card and require every line to have been claimed.
fn parsed(name: &str, text: &str) -> Value {
    let p = parse_card(name, text);
    assert!(
        p.is_complete(),
        "expected {name:?} to parse completely, but declined: {:#?}",
        p.declines
    );
    serde_json::to_value(&p.out).expect("serialize")
}

fn abilities(name: &str, text: &str) -> Value {
    parsed(name, text)["abilities"].clone()
}

fn triggers(name: &str, text: &str) -> Value {
    parsed(name, text)["triggers"].clone()
}

/// The eleven fields the engine prints on every ability, at their defaults.
fn spell(effect: Value, description: &str) -> Value {
    json!({
        "kind": "Spell",
        "effect": effect,
        "cost": null,
        "sub_ability": null,
        "duration": null,
        "description": description,
        "target_prompt": null,
        "condition": null,
        "optional_targeting": false,
        "optional": false,
        "forward_result": false
    })
}

// ---------------------------------------------------------------------------
// Totality: the property the whole design rests on
// ---------------------------------------------------------------------------

#[test]
fn a_clause_with_an_unparsed_qualifier_declines_rather_than_widening_its_target() {
    // The invariant, stated on a qualifier the grammar cannot YET express.
    // Dropping it would silently turn a narrow removal spell into an
    // unconditional one and nothing downstream could tell. Here that is
    // structurally impossible: the qualifier's tokens go unconsumed, so the
    // line declines with a span instead of widening the target.
    //
    // The mana-value form this test used to pin now PARSES — see
    // `a_relative_clause_narrows_the_target_it_follows`. That the gap was a
    // visible decline first, rather than a silent behaviour difference, is the
    // whole point of the totality rule.
    let p = parse_card(
        "Whatever",
        "Destroy target creature that was dealt damage this turn.",
    );
    assert!(
        !p.is_complete(),
        "must not claim a clause it only half understands"
    );
    assert_eq!(p.declines.len(), 1);
    assert_eq!(p.out.abilities.len(), 0);
}

#[test]
fn a_decline_names_a_production_and_carries_a_span() {
    let p = parse_card("Whatever", "Manifest the top card of your library.");
    let d = p.declines.first().expect("one decline");
    assert!(!d.production.is_empty());
    assert!(d.end > d.start, "a decline must carry a real span");
    assert_eq!(d.text, "Manifest the top card of your library.");
}

#[test]
fn a_half_understood_cost_declines_the_whole_ability() {
    // A cost the grammar only partly reads would make the ability activatable
    // for less than it prints, which is worse than not offering it at all.
    let p = parse_card(
        "Whatever",
        "{2}, Remove a page counter from it: Draw a card.",
    );
    assert!(!p.is_complete());
    assert!(p.out.abilities.is_empty());
}

// ---------------------------------------------------------------------------
// Spell abilities
// ---------------------------------------------------------------------------

#[test]
fn destroy_with_a_colour_restriction() {
    assert_eq!(
        abilities("Doom Blade", "Destroy target nonblack creature."),
        json!([spell(
            json!({
                "type": "Destroy",
                "target": {
                    "type": "Typed",
                    "type_filters": ["Creature"],
                    "controller": null,
                    "properties": [{"type": "NotColor", "color": "Black"}]
                },
                "cant_regenerate": false
            }),
            "Destroy target nonblack creature."
        )])
    );
}

#[test]
fn a_multi_type_target_becomes_a_disjunction() {
    let v = abilities("Naturalize", "Destroy target artifact or enchantment.");
    assert_eq!(v[0]["effect"]["target"]["type"], "Or");
    assert_eq!(
        v[0]["effect"]["target"]["filters"][0]["type_filters"],
        json!(["Artifact"])
    );
    assert_eq!(
        v[0]["effect"]["target"]["filters"][1]["type_filters"],
        json!(["Enchantment"])
    );
}

#[test]
fn a_mass_instruction_picks_the_all_scoped_variant() {
    // Scope is carried on the subject and translated ONCE at emission, so the
    // same `destroy` production serves both forms.
    let one = abilities("Doom Blade", "Destroy target creature.");
    let all = abilities("Wrath of God", "Destroy all creatures.");
    assert_eq!(one[0]["effect"]["type"], "Destroy");
    assert_eq!(all[0]["effect"]["type"], "DestroyAll");
    // Same filter either way.
    assert_eq!(one[0]["effect"]["target"], all[0]["effect"]["target"]);
}

#[test]
fn exile_is_emitted_as_a_zone_change() {
    let v = abilities("Whatever", "Exile target creature.");
    assert_eq!(v[0]["effect"]["type"], "ChangeZone");
    assert_eq!(v[0]["effect"]["destination"], "Exile");
    assert_eq!(v[0]["effect"]["origin"], json!(null));
}

#[test]
fn a_battlefield_hand_bounce_is_bounce_but_a_graveyard_one_is_a_zone_change() {
    let b = abilities("Unsummon", "Return target creature to its owner's hand.");
    assert_eq!(b[0]["effect"]["type"], "Bounce");
    assert_eq!(b[0]["effect"]["destination"], json!(null));

    let z = abilities(
        "Raise Dead",
        "Return target creature card from your graveyard to your hand.",
    );
    assert_eq!(z[0]["effect"]["type"], "ChangeZone");
    assert_eq!(z[0]["effect"]["origin"], "Graveyard");
    assert_eq!(z[0]["effect"]["destination"], "Hand");
}

#[test]
fn the_noun_card_is_not_a_type_when_a_real_type_is_present() {
    // CR 108.1: "target creature card" is a creature, not a Creature AND a Card.
    let v = abilities(
        "Raise Dead",
        "Return target creature card from your graveyard to your hand.",
    );
    assert_eq!(
        v[0]["effect"]["target"]["type_filters"],
        json!(["Creature"])
    );

    let s = abilities(
        "Whatever",
        "Return target Spirit card from your graveyard to your hand.",
    );
    assert_eq!(
        s[0]["effect"]["target"]["type_filters"],
        json!([{"Subtype": "Spirit"}])
    );
}

#[test]
fn a_possessive_zone_phrase_constrains_both_zone_and_controller() {
    let v = abilities(
        "Raise Dead",
        "Return target creature card from your graveyard to your hand.",
    );
    let t = &v[0]["effect"]["target"];
    assert_eq!(t["controller"], "You");
    assert_eq!(
        t["properties"],
        json!([{"type": "InZone", "zone": "Graveyard"}])
    );
}

#[test]
fn a_pump_carries_a_duration_and_a_pure_pt_change_is_not_a_static_ability() {
    assert_eq!(
        abilities(
            "Giant Growth",
            "Target creature gets +3/+3 until end of turn."
        ),
        json!([{
            "kind": "Spell",
            "effect": {
                "type": "Pump",
                "power": {"type": "Fixed", "value": 3},
                "toughness": {"type": "Fixed", "value": 3},
                "target": {
                    "type": "Typed",
                    "type_filters": ["Creature"],
                    "controller": null,
                    "properties": []
                }
            },
            "cost": null,
            "sub_ability": null,
            "duration": "UntilEndOfTurn",
            "description": "Target creature gets +3/+3 until end of turn.",
            "target_prompt": null,
            "condition": null,
            "optional_targeting": false,
            "optional": false,
            "forward_result": false
        }])
    );
}

#[test]
fn granting_a_keyword_lowers_to_a_static_ability_not_to_a_keyword_effect() {
    // The engine has no "grant keyword" effect: a granted keyword is a
    // continuous modification (CR 611), wrapped so the chosen object can be
    // reached through `ParentTarget`.
    let v = abilities(
        "Accelerate",
        "Target creature gains haste until end of turn.",
    );
    let e = &v[0]["effect"];
    assert_eq!(e["type"], "GenericEffect");
    assert_eq!(e["duration"], "UntilEndOfTurn");
    assert_eq!(e["target"]["type_filters"], json!(["Creature"]));
    assert_eq!(
        e["static_abilities"][0]["affected"],
        json!({"type": "ParentTarget"})
    );
    assert_eq!(
        e["static_abilities"][0]["modifications"],
        json!([{"type": "AddKeyword", "keyword": "Haste"}])
    );
    assert_eq!(e["static_abilities"][0]["description"], "gain haste");
}

#[test]
fn a_conjunction_reuses_its_subject_and_folds_into_one_static_ability() {
    let v = abilities(
        "Arborea Pegasus",
        "Target creature gets +1/+1 and gains flying until end of turn.",
    );
    let sa = &v[0]["effect"]["static_abilities"][0];
    assert_eq!(
        sa["modifications"],
        json!([
            {"type": "AddPower", "value": 1},
            {"type": "AddToughness", "value": 1},
            {"type": "AddKeyword", "keyword": "Flying"}
        ])
    );
    // Only the LEADING verb is put into the infinitive. That asymmetry is the
    // engine's and is reproduced rather than tidied.
    assert_eq!(sa["description"], "get +1/+1 and gains flying");
}

#[test]
fn a_target_is_chosen_once_and_referred_back_to() {
    // CR 601.2c. Repeating the filter would announce a second target.
    let v = abilities(
        "Ancestral Craving",
        "Target player draws three cards and loses 3 life.",
    );
    assert_eq!(v[0]["effect"]["target"], json!({"type": "Player"}));
    assert_eq!(
        v[0]["sub_ability"]["effect"]["target"],
        json!({"type": "ParentTarget"})
    );
}

#[test]
fn life_gain_by_the_controller_omits_the_player_field() {
    // Absence encodes "you". Reproduced deliberately; flagged for a later
    // format proposal rather than fixed in passing.
    let v = abilities("Whatever", "You gain 3 life.");
    assert_eq!(
        v[0]["effect"],
        json!({"type": "GainLife", "amount": {"type": "Fixed", "value": 3}})
    );
}

#[test]
fn every_spell_line_of_a_card_folds_into_one_definition() {
    // The engine treats a spell's whole printed body as ONE ability whose
    // description carries the newlines, chained by `SequentialSibling`.
    let v = abilities(
        "Accelerate",
        "Target creature gains haste until end of turn.\nDraw a card.",
    );
    assert_eq!(v.as_array().expect("array").len(), 1);
    assert_eq!(
        v[0]["description"],
        "Target creature gains haste until end of turn.\nDraw a card."
    );
    assert_eq!(v[0]["sub_ability"]["effect"]["type"], "Draw");
    assert_eq!(v[0]["sub_ability"]["sub_link"], "SequentialSibling");
}

#[test]
fn each_sentence_keeps_its_own_duration() {
    let v = abilities(
        "Agony Warp",
        "Target creature gets -3/-0 until end of turn.\nTarget creature gets -0/-3 until end of turn.",
    );
    assert_eq!(v[0]["duration"], "UntilEndOfTurn");
    assert_eq!(v[0]["sub_ability"]["duration"], "UntilEndOfTurn");
}

#[test]
fn a_then_inside_a_sentence_is_a_continuation_not_a_sibling() {
    // CR 608.2c: the boundary KIND decides the link, and a continuation is the
    // engine's default, so it is omitted from the printed JSON entirely.
    let v = abilities("Whatever", "Draw a card, then shuffle.");
    assert_eq!(v[0]["sub_ability"]["effect"]["type"], "Shuffle");
    assert!(v[0]["sub_ability"].get("sub_link").is_none());
}

// ---------------------------------------------------------------------------
// Keyword lines
// ---------------------------------------------------------------------------

#[test]
fn a_bare_keyword_line_hoists_into_the_keywords_array() {
    let v = parsed("Serra Angel", "Flying\nVigilance");
    assert_eq!(v["keywords"], json!(["Flying", "Vigilance"]));
    assert!(
        v.get("abilities").is_none(),
        "a keyword line is not an ability"
    );
}

#[test]
fn a_comma_separated_keyword_line_is_one_line_not_a_decline() {
    let v = parsed("Whatever", "First strike, trample, lifelink");
    assert_eq!(v["keywords"], json!(["FirstStrike", "Trample", "Lifelink"]));
}

#[test]
fn a_two_word_keyword_wins_over_its_first_word() {
    let v = parsed("Whatever", "First strike");
    assert_eq!(v["keywords"], json!(["FirstStrike"]));
}

// ---------------------------------------------------------------------------
// Activated abilities
// ---------------------------------------------------------------------------

#[test]
fn a_tap_ability() {
    assert_eq!(
        abilities(
            "Prodigal Sorcerer",
            "{T}: This creature deals 1 damage to any target."
        ),
        json!([{
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
        }])
    );
}

#[test]
fn a_run_of_mana_symbols_is_one_cost_component() {
    // `{2}{R}{R}` is one `ManaCost`, not three components.
    let v = abilities("Whatever", "{2}{R}{R}: Draw a card.");
    assert_eq!(
        v[0]["cost"],
        json!({"type": "Mana", "cost": {"type": "Cost", "shards": ["Red", "Red"], "generic": 2}})
    );
}

#[test]
fn a_comma_separated_cost_list_becomes_a_composite() {
    let v = abilities(
        "Icy Manipulator",
        "{1}, {T}: Tap target artifact, creature, or land.",
    );
    assert_eq!(
        v[0]["cost"],
        json!({
            "type": "Composite",
            "costs": [
                {"type": "Mana", "cost": {"type": "Cost", "shards": [], "generic": 1}},
                {"type": "Tap"}
            ]
        })
    );
    assert_eq!(v[0]["effect"]["scope"], json!({"type": "Single"}));
    assert_eq!(v[0]["effect"]["state"], json!({"type": "Tap"}));
}

#[test]
fn a_hybrid_mana_symbol_is_one_shard() {
    let v = abilities(
        "Riveteers Initiate",
        "{B/G}: Riveteers Initiate gains deathtouch until end of turn.",
    );
    assert_eq!(
        v[0]["cost"],
        json!({"type": "Mana", "cost": {"type": "Cost", "shards": ["BlackGreen"], "generic": 0}})
    );
}

#[test]
fn a_sacrifice_cost_reads_its_own_object() {
    let v = abilities("Whatever", "{G}, Sacrifice ~: Draw a card.");
    assert_eq!(
        v[0]["cost"]["costs"][1],
        json!({"type": "Sacrifice", "target": {"type": "SelfRef"}, "count": 1})
    );
}

// ---------------------------------------------------------------------------
// Triggered abilities
// ---------------------------------------------------------------------------

#[test]
fn an_enters_trigger() {
    assert_eq!(
        triggers(
            "Elvish Visionary",
            "When this creature enters, draw a card."
        ),
        json!([{
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
        }])
    );
}

#[test]
fn a_self_dies_trigger_functions_from_the_graveyard_but_another_dying_does_not() {
    // CR 603.6: the source is already in the graveyard when its own death
    // triggers, so that is where the ability has to function.
    let own = triggers("Haywire Mite", "When ~ dies, you gain 3 life.");
    assert_eq!(own[0]["origin"], "Battlefield");
    assert_eq!(own[0]["destination"], "Graveyard");
    assert_eq!(own[0]["trigger_zones"], json!(["Graveyard"]));

    let other = triggers(
        "Whatever",
        "Whenever another creature dies, you gain 1 life.",
    );
    assert_eq!(other[0]["trigger_zones"], json!(["Battlefield"]));
}

#[test]
fn a_leaves_the_battlefield_trigger_functions_from_three_zones() {
    let v = triggers(
        "Circuit Mender",
        "When ~ leaves the battlefield, draw a card.",
    );
    assert_eq!(v[0]["mode"], "LeavesBattlefield");
    assert_eq!(
        v[0]["trigger_zones"],
        json!(["Battlefield", "Graveyard", "Exile"])
    );
}

#[test]
fn a_possessive_upkeep_trigger_carries_a_turn_constraint() {
    // "your upkeep" restricts the trigger to the controller's turn; "the end
    // step" does not, and the difference is recorded rather than guessed.
    let yours = triggers(
        "Phyrexian Arena",
        "At the beginning of your upkeep, draw a card.",
    );
    assert_eq!(yours[0]["mode"], "Phase");
    assert_eq!(yours[0]["phase"], "Upkeep");
    assert_eq!(
        yours[0]["constraint"],
        json!({"type": "OnlyDuringYourTurn"})
    );

    let anyone = triggers(
        "Ball Lightning",
        "At the beginning of the end step, sacrifice ~.",
    );
    assert_eq!(anyone[0]["phase"], "End");
    assert_eq!(anyone[0]["constraint"], json!(null));
}

#[test]
fn you_may_marks_both_the_trigger_and_the_ability_it_executes() {
    // CR 603.2c. The trigger decides whether it goes on the stack; the ability
    // decides whether the controller performs the action.
    let v = triggers(
        "Angler Drake",
        "When ~ enters, you may return target creature to its owner's hand.",
    );
    assert_eq!(v[0]["optional"], true);
    assert_eq!(v[0]["execute"]["optional"], true);
}

#[test]
fn a_targeted_player_inside_a_trigger_fills_the_triggers_target_slot() {
    // CR 603.3d: a trigger's targets are chosen as it is put on the stack.
    let targeted = triggers(
        "Abyssal Horror",
        "When ~ enters, target player discards two cards.",
    );
    assert_eq!(targeted[0]["valid_target"], json!({"type": "Player"}));

    // "each opponent" is a scope, not a target, and must NOT fill the slot
    // even though it lowers to a player-shaped filter.
    let scoped = triggers("Whatever", "When ~ enters, each opponent loses 1 life.");
    assert_eq!(scoped[0]["valid_target"], json!(null));
}

#[test]
fn a_damage_trigger_splits_source_from_victim() {
    let v = triggers(
        "Hypnotic Specter",
        "Whenever ~ deals damage to an opponent, that player discards a card.",
    );
    // The victim belongs in the target slot, not in the watched-object slot.
    assert_eq!(v[0]["mode"], "DamageDone");
    assert_eq!(v[0]["valid_source"], json!({"type": "SelfRef"}));
}

#[test]
fn an_attack_trigger_watches_its_own_source() {
    let v = triggers("Hero of Bladehold", "Whenever ~ attacks, draw a card.");
    assert_eq!(v[0]["mode"], "Attacks");
    assert_eq!(v[0]["valid_card"], json!({"type": "SelfRef"}));
}

// ---------------------------------------------------------------------------
// Self-reference normalization
// ---------------------------------------------------------------------------

#[test]
fn a_cards_own_name_and_its_generic_self_reference_are_the_same_token() {
    let by_name = abilities("Shock", "Shock deals 2 damage to any target.");
    let by_phrase = abilities("Zap", "This creature deals 2 damage to any target.");
    assert_eq!(by_name[0]["effect"], by_phrase[0]["effect"]);
    assert_eq!(by_name[0]["description"], "~ deals 2 damage to any target.");
}

// ---------------------------------------------------------------------------
// Mana abilities
// ---------------------------------------------------------------------------

#[test]
fn a_coloured_mana_ability_lists_its_symbols_and_is_flagged() {
    // CR 605.1a: `is_mana_ability` is computed from the effect, never parsed,
    // so it cannot disagree with what the ability actually does.
    let v = abilities("Llanowar Elves", "{T}: Add {G}.");
    assert_eq!(
        v[0]["effect"],
        json!({"type": "Mana", "produced": {"type": "Fixed", "colors": ["Green"]}})
    );
    assert_eq!(v[0]["is_mana_ability"], true);
}

#[test]
fn colourless_mana_is_a_count_but_coloured_mana_is_a_list() {
    // `{C}{C}` is two of ONE thing; `{B}{B}{B}` is three separate symbols. The
    // engine draws that distinction and the grammar has to as well.
    let c = abilities("Whatever", "{T}: Add {C}{C}.");
    assert_eq!(
        c[0]["effect"]["produced"],
        json!({"type": "Colorless", "count": {"type": "Fixed", "value": 2}})
    );
    let b = abilities("Dark Ritual", "Add {B}{B}{B}.");
    assert_eq!(
        b[0]["effect"]["produced"],
        json!({"type": "Fixed", "colors": ["Black", "Black", "Black"]})
    );
}

#[test]
fn mana_of_any_color_carries_the_colours_that_may_be_chosen() {
    let v = abilities("Whatever", "{T}: Add one mana of any color.");
    assert_eq!(
        v[0]["effect"]["produced"],
        json!({
            "type": "AnyOneColor",
            "count": {"type": "Fixed", "value": 1},
            "color_options": ["White", "Blue", "Black", "Red", "Green"]
        })
    );
}

#[test]
fn an_unmodelled_mana_shape_declines_rather_than_being_guessed_at() {
    // Hybrid and Phyrexian production are real shapes with no production here.
    let p = parse_card("Whatever", "{T}: Add {W/U}.");
    assert!(!p.is_complete());
}

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

#[test]
fn a_creature_token_reads_its_whole_printed_body() {
    assert_eq!(
        abilities(
            "Raise the Alarm",
            "Create two 1/1 white Soldier creature tokens."
        )[0]["effect"],
        json!({
            "type": "Token",
            "name": "Soldier",
            "power": {"type": "Fixed", "value": 1},
            "toughness": {"type": "Fixed", "value": 1},
            "types": ["Creature", "Soldier"],
            "colors": ["White"],
            "keywords": [],
            "tapped": false,
            "count": {"type": "Fixed", "value": 2},
            "owner": {"type": "Controller"},
            "enters_attacking": false
        })
    );
}

#[test]
fn a_token_keeps_core_types_before_subtypes_whatever_the_printed_order() {
    // Printed: "1/1 white Spirit creature token". Engine: ["Creature","Spirit"].
    let v = abilities(
        "Lingering Souls",
        "Create two 1/1 white Spirit creature tokens with flying.",
    );
    assert_eq!(v[0]["effect"]["types"], json!(["Creature", "Spirit"]));
    assert_eq!(v[0]["effect"]["keywords"], json!(["Flying"]));
}

#[test]
fn a_token_can_be_printed_with_more_than_one_colour() {
    let v = abilities(
        "Aqueous Aria",
        "Create a 3/3 blue and red Elemental creature token with flying.",
    );
    assert_eq!(v[0]["effect"]["colors"], json!(["Blue", "Red"]));
}

#[test]
fn a_noncreature_token_has_no_power_or_toughness_printed() {
    let v = abilities("Whatever", "Create a Treasure artifact token.");
    assert_eq!(v[0]["effect"]["name"], "Treasure");
    assert_eq!(v[0]["effect"]["types"], json!(["Artifact", "Treasure"]));
    assert_eq!(
        v[0]["effect"]["power"],
        json!({"type": "Fixed", "value": 0})
    );
}

#[test]
fn a_token_that_enters_tapped_and_attacking_records_both() {
    let v = triggers(
        "Hero of Bladehold",
        "Whenever ~ attacks, create two 1/1 white Soldier creature tokens that are tapped and attacking.",
    );
    let e = &v[0]["execute"]["effect"];
    assert_eq!(e["tapped"], true);
    assert_eq!(e["enters_attacking"], true);
}

// ---------------------------------------------------------------------------
// Player classes versus player targets
// ---------------------------------------------------------------------------

#[test]
fn damage_to_a_class_of_players_is_not_a_mass_object_effect() {
    // CR 102.1: a player is not an object, so "each opponent" cannot share a
    // target slot with "all creatures".
    let players = abilities("Boltwave", "Boltwave deals 3 damage to each opponent.");
    assert_eq!(
        players[0]["effect"],
        json!({
            "type": "DamageEachPlayer",
            "amount": {"type": "Fixed", "value": 3},
            "player_filter": {"type": "Opponent"}
        })
    );

    let objects = abilities("Whatever", "~ deals 3 damage to each creature.");
    assert_eq!(objects[0]["effect"]["type"], "DamageAll");
}

#[test]
fn an_iterated_player_class_is_a_scope_not_a_target() {
    // CR 101.4: "each opponent mills a card" is a CONTROLLER-shaped mill run
    // once per opponent, which is why the effect's own target says "Controller".
    let v = triggers(
        "Altar of the Brood",
        "Whenever another permanent you control enters, each opponent mills a card.",
    );
    let ex = &v[0]["execute"];
    assert_eq!(ex["effect"]["target"], json!({"type": "Controller"}));
    assert_eq!(ex["player_scope"], json!({"type": "Opponent"}));
}

// ---------------------------------------------------------------------------
// Attachment, back-references and coordination
// ---------------------------------------------------------------------------

#[test]
fn an_auras_own_host_and_any_enchanted_object_are_different_predicates() {
    // Singular names THIS source's host; plural names anything carrying an Aura.
    let host = abilities("Holy Strength", "Enchanted creature gets +1/+2.");
    let _ = host;
    let statics =
        parsed("Holy Strength", "Enchanted creature gets +1/+2.")["static_abilities"].clone();
    assert_eq!(
        statics[0]["affected"]["properties"],
        json!([{"type": "EnchantedBy"}])
    );

    let any = parsed(
        "A Tale for the Ages",
        "Enchanted creatures you control get +2/+2.",
    )["static_abilities"]
        .clone();
    assert_eq!(
        any[0]["affected"]["properties"],
        json!([{"type": "HasAttachment", "kind": "Aura"}])
    );
}

#[test]
fn an_attachment_adjective_works_in_target_position_too() {
    let v = abilities(
        "Cut the Earthly Bond",
        "Return target enchanted permanent to its owner's hand.",
    );
    assert_eq!(
        v[0]["effect"]["target"]["type_filters"],
        json!(["Permanent"])
    );
    assert_eq!(
        v[0]["effect"]["target"]["properties"],
        json!([{"type": "EnchantedBy"}])
    );
}

#[test]
fn a_trailing_qualifier_applies_to_every_alternative_it_coordinates() {
    // "target instant or sorcery card from your graveyard" — the graveyard
    // qualifies both, which is how English coordination works and what the
    // engine records.
    let v = triggers(
        "Archaeomancer",
        "When ~ enters, return target instant or sorcery card from your graveyard to your hand.",
    );
    let t = &v[0]["execute"]["effect"]["target"];
    assert_eq!(t["type"], "Or");
    for n in 0..2 {
        assert_eq!(t["filters"][n]["controller"], "You");
        assert_eq!(
            t["filters"][n]["properties"],
            json!([{"type": "InZone", "zone": "Graveyard"}])
        );
    }
}

#[test]
fn another_is_printed_once_and_distributes_over_the_whole_list() {
    // CR 109.5. "Sacrifice another creature or artifact" excludes the source
    // from BOTH alternatives.
    let v = abilities(
        "Ahriman",
        "{3}, Sacrifice another creature or artifact: Draw a card.",
    );
    let t = &v[0]["cost"]["costs"][1]["target"];
    assert_eq!(t["filters"][0]["properties"], json!([{"type": "Another"}]));
    assert_eq!(t["filters"][1]["properties"], json!([{"type": "Another"}]));
}

#[test]
fn a_later_clause_refers_back_to_the_object_already_chosen() {
    let v = abilities(
        "Burning Cloak",
        "Target creature gets +2/+0 until end of turn. Burning Cloak deals 2 damage to that creature.",
    );
    assert_eq!(
        v[0]["sub_ability"]["effect"]["target"],
        json!({"type": "ParentTarget"})
    );
}

#[test]
fn the_mass_zone_change_carries_fewer_fields_than_the_single_one() {
    // Verified against every ChangeZoneAll in the corpus: the engine prints no
    // battlefield-entry riders on the mass form.
    let one = abilities(
        "Raise Dead",
        "Return target creature card from your graveyard to your hand.",
    );
    assert!(one[0]["effect"].get("enter_tapped").is_some());

    let all = abilities(
        "Crystal Chimes",
        "Return all enchantment cards from your graveyard to your hand.",
    );
    assert_eq!(all[0]["effect"]["type"], "ChangeZoneAll");
    assert!(all[0]["effect"].get("enter_tapped").is_none());
}

// ---------------------------------------------------------------------------
// Spells on the stack, timing, and the life-effect asymmetry
// ---------------------------------------------------------------------------

#[test]
fn a_spell_is_a_stack_object_not_a_card_type() {
    // CR 111.1: a spell is zone-dependent and has no type-line spelling, so
    // "target spell" can never be a `type_filters` entry.
    let bare = abilities("Counterspell", "Counter target spell.");
    assert_eq!(
        bare[0]["effect"],
        json!({"type": "Counter", "target": {"type": "StackSpell"}})
    );

    // A type restriction CONJOINS with the stack-object filter.
    let typed = abilities("Whatever", "Counter target creature spell.");
    let t = &typed[0]["effect"]["target"];
    assert_eq!(t["type"], "And");
    assert_eq!(t["filters"][0], json!({"type": "StackSpell"}));
    assert_eq!(t["filters"][1]["type_filters"], json!(["Creature"]));
}

#[test]
fn other_and_another_are_the_same_exclusion() {
    // CR 109.5. They differ only in the number of the noun they modify.
    let plural = parsed("Whatever", "Other black creatures get +1/+1.")["static_abilities"].clone();
    assert_eq!(
        plural[0]["affected"]["properties"],
        json!([{"type": "HasColor", "color": "Black"}, {"type": "Another"}])
    );
}

#[test]
fn a_step_possessive_can_be_printed_before_or_after_the_step() {
    // "your upkeep" and "combat on your turn" mean the same restriction and
    // must reach one representation.
    let before = triggers("Whatever", "At the beginning of your upkeep, draw a card.");
    let after = triggers(
        "Whatever",
        "At the beginning of combat on your turn, draw a card.",
    );
    assert_eq!(
        before[0]["constraint"],
        json!({"type": "OnlyDuringYourTurn"})
    );
    assert_eq!(
        after[0]["constraint"],
        json!({"type": "OnlyDuringYourTurn"})
    );
    assert_eq!(after[0]["phase"], "BeginCombat");
}

#[test]
fn an_activation_restriction_is_lifted_off_the_effect_body() {
    // CR 602.5d: "Activate only as a sorcery" says WHEN, not what.
    let v = abilities("Whatever", "{T}: Draw a card. Activate only as a sorcery.");
    assert_eq!(
        v[0]["activation_restrictions"],
        json!([{"type": "AsSorcery"}])
    );
    assert_eq!(v[0]["effect"]["type"], "Draw");
    assert_eq!(
        v[0]["sub_ability"],
        json!(null),
        "the restriction is not a second effect"
    );
}

#[test]
fn an_unrecognized_activation_restriction_declines_instead_of_vanishing() {
    // Leaving it in the body is the totality rule applied to a FIELD: a timing
    // restriction the grammar cannot express must not be silently dropped.
    let p = parse_card(
        "Whatever",
        "{T}: Draw a card. Activate only if you control a Forest.",
    );
    assert!(!p.is_complete());
    assert!(p.out.abilities.is_empty());
}

#[test]
fn a_loyalty_ability_is_sorcery_speed_without_the_card_saying_so() {
    // CR 606.3. Inherent to the cost, so it is derived rather than parsed.
    let v = abilities(
        "Domri, City Smasher",
        "[-3]: ~ deals 3 damage to any target.",
    );
    assert_eq!(v[0]["cost"], json!({"type": "Loyalty", "amount": -3}));
    assert_eq!(
        v[0]["activation_restrictions"],
        json!([{"type": "AsSorcery"}])
    );
}

#[test]
fn the_two_life_effects_disagree_about_printing_their_subject() {
    // GainLife OMITS the controller; LoseLife PRINTS it. The engine is
    // inconsistent here and the mirror follows each of them rather than
    // tidying either — a like-for-like replacement cannot smuggle in a fix.
    let v = abilities("Whatever", "You gain 10 life.\nYou lose 10 life.");
    assert_eq!(
        v[0]["effect"],
        json!({"type": "GainLife", "amount": {"type": "Fixed", "value": 10}})
    );
    assert_eq!(
        v[0]["sub_ability"]["effect"],
        json!({
            "type": "LoseLife",
            "amount": {"type": "Fixed", "value": 10},
            "target": {"type": "Controller"}
        })
    );
}

#[test]
fn an_iterated_clause_leaves_its_player_slot_empty() {
    // CR 101.4: `player_scope` already names who acts, so the effect's own
    // player slot would be saying it twice.
    let v = triggers(
        "Whatever",
        "Whenever ~ attacks, each opponent loses 3 life.",
    );
    let e = &v[0]["execute"];
    assert_eq!(
        e["effect"],
        json!({"type": "LoseLife", "amount": {"type": "Fixed", "value": 3}})
    );
    assert_eq!(e["player_scope"], json!({"type": "Opponent"}));
}

// ---------------------------------------------------------------------------
// Reminder text, keyword vocabulary, predefined tokens
// ---------------------------------------------------------------------------

#[test]
fn reminder_text_is_dropped_before_the_grammar_not_just_before_the_description() {
    // A reminder sitting after a sentence's period would otherwise read as a
    // second, unparseable sentence and fail the whole line. This is the one
    // span the grammar may discard: the lexer proved it is a complete
    // parenthesised unit, so dropping it cannot lose a partial clause.
    let v = triggers(
        "Automatic Librarian",
        "When this creature enters, scry 2. (Look at the top two cards of your library, then put any number of them on the bottom and the rest on top in any order.)",
    );
    assert_eq!(v[0]["execute"]["effect"]["type"], "Scry");
    assert_eq!(v[0]["description"], "When ~ enters, scry 2.");
}

#[test]
fn a_keyword_that_also_creates_a_trigger_is_not_hoisted_as_a_bare_keyword() {
    // Evolve prints as one word but stands for a triggered ability. Hoisting it
    // into `keywords` would silently drop the behaviour, so it declines until
    // the trigger it stands for can be synthesized.
    let p = parse_card("Adaptive Snapjaw", "Evolve");
    assert!(
        !p.is_complete(),
        "must not claim a keyword whose behaviour it drops"
    );
    assert!(p.out.keywords.is_empty());
}

#[test]
fn landwalk_is_one_parameterized_keyword_not_five_lookalikes() {
    // CR 702.14. "Swampwalk" is not a keyword named Swampwalk; it is Landwalk
    // of Swamp, and flattening it to a string loses the land type.
    let v = parsed("Anaconda", "Swampwalk");
    assert_eq!(v["keywords"], json!([{"Landwalk": "Swamp"}]));
    let i = parsed("Whatever", "Islandwalk");
    assert_eq!(i["keywords"], json!([{"Landwalk": "Island"}]));
}

#[test]
fn a_predefined_token_supplies_the_card_type_its_sentence_omits() {
    // CR 111.9: "create a Treasure token" names only the subtype.
    let v = abilities("Whatever", "Create a Treasure token.");
    assert_eq!(v[0]["effect"]["types"], json!(["Artifact", "Treasure"]));
    assert_eq!(v[0]["effect"]["name"], "Treasure");
}

#[test]
fn a_tapped_token_can_say_so_inline_or_in_a_trailing_clause() {
    let inline = abilities("Argothian Opportunist", "Create a tapped Powerstone token.");
    assert_eq!(inline[0]["effect"]["tapped"], true);
    assert_eq!(
        inline[0]["effect"]["types"],
        json!(["Artifact", "Powerstone"])
    );

    let trailing = abilities(
        "Whatever",
        "Create a 1/1 white Soldier creature token that's tapped.",
    );
    assert_eq!(trailing[0]["effect"]["tapped"], true);
}

#[test]
fn a_spell_type_restriction_distributes_into_each_alternative() {
    // "artifact or enchantment spell" is (spell AND artifact) or (spell AND
    // enchantment) — the engine's shape is the distributed one.
    let v = abilities("Annul", "Counter target artifact or enchantment spell.");
    let t = &v[0]["effect"]["target"];
    assert_eq!(t["type"], "Or");
    assert_eq!(t["filters"][0]["type"], "And");
    assert_eq!(t["filters"][0]["filters"][0], json!({"type": "StackSpell"}));
    assert_eq!(
        t["filters"][0]["filters"][1]["type_filters"],
        json!(["Artifact"])
    );
    assert_eq!(
        t["filters"][1]["filters"][1]["type_filters"],
        json!(["Enchantment"])
    );
}

#[test]
fn attacking_with_your_team_is_a_different_event_from_one_creature_attacking() {
    // "Whenever you attack" watches the attack step as a whole and has no
    // watched object; "whenever ~ attacks" watches one creature.
    let team = triggers("Bard, Heir of Girion", "Whenever you attack, draw a card.");
    assert_eq!(team[0]["mode"], "YouAttack");
    assert_eq!(team[0]["valid_card"], json!(null));

    let one = triggers("Whatever", "Whenever ~ attacks, draw a card.");
    assert_eq!(one[0]["mode"], "Attacks");
    assert_eq!(one[0]["valid_card"], json!({"type": "SelfRef"}));
}

// ---------------------------------------------------------------------------
// Ability words, parameterized keywords, and position-dependent spellings
// ---------------------------------------------------------------------------

#[test]
fn an_ability_word_is_flavour_and_is_dropped_including_from_the_description() {
    // CR 207.2c: an ability word has no rules meaning.
    let v = triggers(
        "Storm-Kiln Artist",
        "Magecraft — Whenever you cast a spell, create a Treasure token.",
    );
    assert_eq!(
        v[0]["description"],
        "Whenever you cast a spell, create a Treasure token."
    );
}

#[test]
fn a_spaced_em_dash_is_flavour_but_an_unspaced_one_joins_a_keyword_to_its_argument() {
    // Magic's templating is bimodal here and the corpus confirms it: 3,518
    // spaced against 439 unspaced, nothing in between. "Cumulative upkeep—Put a
    // -1/-1 counter" must NOT lose its label, because the label is the keyword.
    let p = parse_card(
        "Aboroth",
        "Cumulative upkeep—Put a -1/-1 counter on this creature.",
    );
    assert!(
        !p.is_complete(),
        "an unspaced dash joins a keyword to its argument and must not be stripped as flavour"
    );
}

#[test]
fn a_saga_chapter_head_is_not_an_ability_word() {
    // CR 714.2: the numeral is structural.
    let p = parse_card("Whatever", "I — Draw a card.");
    assert!(p.out.abilities.is_empty() && !p.is_complete());
}

#[test]
fn enchant_carries_the_filter_an_aura_may_attach_to() {
    // CR 702.5a. A keyword with an argument, not a keyword with a name.
    let v = parsed("Holy Strength", "Enchant creature");
    assert_eq!(
        v["keywords"],
        json!([{
            "Enchant": {
                "type": "Typed",
                "type_filters": ["Creature"],
                "controller": null,
                "properties": []
            }
        }])
    );
}

#[test]
fn equip_lowers_to_the_ability_the_keyword_stands_for() {
    // CR 702.6b: none of "attach to a creature you control, at sorcery speed"
    // is printed on the card, so all of it is derived from the keyword.
    let v = abilities("Bonesplitter", "Equip {1}");
    assert_eq!(
        v[0]["effect"],
        json!({
            "type": "Attach",
            "target": {
                "type": "Typed",
                "type_filters": ["Creature"],
                "controller": "You",
                "properties": []
            }
        })
    );
    assert_eq!(v[0]["kind"], "Activated");
    assert_eq!(
        v[0]["cost"],
        json!({"type": "Mana", "cost": {"type": "Cost", "shards": [], "generic": 1}})
    );
    assert_eq!(
        v[0]["activation_restrictions"],
        json!([{"type": "AsSorcery"}])
    );
    assert_eq!(v[0]["ability_tag"], json!({"type": "Equip"}));
    assert_eq!(v[0]["description"], "Equip {1}");
}

#[test]
fn an_untargeted_filter_subject_pumps_as_the_mass_form() {
    // CR 613.4b. Targeting, not plurality, separates the two: "enchanted
    // creature gets +0/+1" reaches its object through a filter.
    let untargeted = abilities(
        "Armor of Faith",
        "{W}: Enchanted creature gets +0/+1 until end of turn.",
    );
    assert_eq!(untargeted[0]["effect"]["type"], "PumpAll");

    let targeted = abilities(
        "Giant Growth",
        "Target creature gets +3/+3 until end of turn.",
    );
    assert_eq!(targeted[0]["effect"]["type"], "Pump");

    // The source itself is neither.
    let own = abilities("Shivan Dragon", "{R}: ~ gets +1/+0 until end of turn.");
    assert_eq!(own[0]["effect"]["type"], "Pump");
    assert_eq!(own[0]["effect"]["target"], json!({"type": "SelfRef"}));
}

#[test]
fn the_attached_host_is_spelled_one_way_in_a_trigger_and_another_in_a_filter() {
    // Same printed words, two positions. A trigger watches the one object this
    // Aura is on; a static ability reaches it through the filter.
    let watched = triggers(
        "Demonic Vigor",
        "When enchanted creature dies, draw a card.",
    );
    assert_eq!(watched[0]["valid_card"], json!({"type": "AttachedTo"}));

    let affected =
        parsed("Holy Strength", "Enchanted creature gets +1/+2.")["static_abilities"].clone();
    assert_eq!(
        affected[0]["affected"]["properties"],
        json!([{"type": "EnchantedBy"}])
    );
}

#[test]
fn multicolored_is_a_colour_count_rather_than_a_flag() {
    // CR 105.4: so "multicolored" and "two or more colors" share one shape.
    let v = abilities(
        "Psychotic Fury",
        "Target multicolored creature gains double strike until end of turn.",
    );
    assert_eq!(
        v[0]["effect"]["target"]["properties"],
        json!([{"type": "ColorCount", "comparator": "GE", "count": 2}])
    );
}

// ---------------------------------------------------------------------------
// Cost components
// ---------------------------------------------------------------------------

#[test]
fn a_counter_removal_cost_reads_its_kind_and_its_count() {
    // CR 122.1 + CR 601.2h.
    let v = abilities(
        "Whatever",
        "{1}, Remove two charge counters from ~: Draw a card.",
    );
    assert_eq!(
        v[0]["cost"]["costs"][1],
        json!({
            "type": "RemoveCounter",
            "count": 2,
            "counter_type": {"type": "OfType", "data": "charge"},
            "target": null,
            "selection": "SingleObject"
        })
    );
}

#[test]
fn an_untyped_counter_removal_stays_untyped() {
    // "Remove a counter from ~" names no kind; the payment resolves one.
    let v = abilities("Whatever", "Remove a counter from ~: Draw a card.");
    assert_eq!(v[0]["cost"]["counter_type"], json!({"type": "Any"}));
}

#[test]
fn tapping_other_objects_is_a_different_cost_from_the_sources_own_tap_symbol() {
    // CR 601.2b. And being untapped is what the cost REQUIRES, not what it
    // selects for, so the engine leaves it out of the filter.
    let v = abilities(
        "Bramblesnap",
        "Tap an untapped creature you control: ~ gets +1/+1 until end of turn.",
    );
    assert_eq!(
        v[0]["cost"],
        json!({
            "type": "TapCreatures",
            "requirement": {"requirement": "count", "count": 1},
            "filter": {
                "type": "Typed",
                "type_filters": ["Creature"],
                "controller": "You",
                "properties": []
            }
        })
    );

    let own = abilities("Prodigal Sorcerer", "{T}: ~ deals 1 damage to any target.");
    assert_eq!(own[0]["cost"], json!({"type": "Tap"}));
}

#[test]
fn a_sacrifice_cost_reads_a_count_greater_than_one() {
    let v = abilities("Whatever", "Sacrifice three other creatures: Draw a card.");
    assert_eq!(
        v[0]["cost"],
        json!({
            "type": "Sacrifice",
            "target": {
                "type": "Typed",
                "type_filters": ["Creature"],
                "controller": null,
                "properties": [{"type": "Another"}]
            },
            "count": 3
        })
    );
}

#[test]
fn discarding_at_random_is_part_of_the_cost_not_a_separate_clause() {
    let v = abilities(
        "Whatever",
        "{1}{R}, Discard a card at random: ~ deals 1 damage to any target.",
    );
    assert_eq!(
        v[0]["cost"]["costs"][1],
        json!({
            "type": "Discard",
            "count": {"type": "Fixed", "value": 1},
            "filter": null,
            "random": true,
            "self_ref": false
        })
    );
}

// ---------------------------------------------------------------------------
// Relative clauses, bare subtypes, and riders
// ---------------------------------------------------------------------------

#[test]
fn a_relative_clause_narrows_the_target_it_follows() {
    // The 738-card class, now BUILT rather than merely declined honestly.
    // CR 205 + CR 202.3.
    let cmc = abilities(
        "Doom Whisper",
        "Destroy target creature with mana value 3 or less.",
    );
    assert_eq!(
        cmc[0]["effect"]["target"]["properties"],
        json!([{"type": "Cmc", "comparator": "LE", "value": {"type": "Fixed", "value": 3}}])
    );

    let kw = abilities("Whatever", "Destroy target creature with flying.");
    assert_eq!(
        kw[0]["effect"]["target"]["properties"],
        json!([{"type": "WithKeyword", "value": "Flying"}])
    );

    let power = abilities(
        "Whatever",
        "Destroy target creature with power 4 or greater.",
    );
    assert_eq!(
        power[0]["effect"]["target"]["properties"],
        json!([{
            "type": "PtComparison",
            "stat": "Power",
            "scope": "Current",
            "comparator": "GE",
            "value": {"type": "Fixed", "value": 4}
        }])
    );
}

#[test]
fn a_bare_subtype_needs_no_type_word() {
    // CR 205.3. Capitalization is the signal, and the closed subtype list is
    // what recovers the singular: no English rule gets "Elves", "Allies",
    // "Zombies" and "Plains" all right at once.
    let plural = abilities("Whatever", "Destroy all Forests.");
    assert_eq!(plural[0]["effect"]["type"], "DestroyAll");
    assert_eq!(
        plural[0]["effect"]["target"]["type_filters"],
        json!([{"Subtype": "Forest"}])
    );

    let elves = parsed("Canopy Tactician", "Other Elves you control get +1/+1.")
        ["static_abilities"]
        .clone();
    // A STATIC ability's `affected` slot spells out the implied type line; an
    // effect's `target` slot does not. Measured: 620 to 106 one way, 636 to
    // 108 the other. Same printed words, two slots, two shapes.
    assert_eq!(
        elves[0]["affected"]["type_filters"],
        json!(["Creature", {"Subtype": "Elf"}])
    );
    // The exclusion survives in the filter, so the prose drops the word.
    assert_eq!(elves[0]["description"], "Elves you control get +1/+1.");
}

#[test]
fn other_survives_in_the_prose_only_before_a_bare_creatures() {
    // Not a rule anyone would guess, and not one the parser invented: the
    // engine keeps the word 76 times out of 100 before "creatures" and drops
    // it 257 times before a subtype, a colour or "permanents".
    let kept = parsed(
        "Aang, Air Nomad",
        "Other creatures you control have vigilance.",
    )["static_abilities"]
        .clone();
    assert_eq!(
        kept[0]["description"],
        "Other creatures you control have vigilance."
    );

    let dropped = parsed("Whatever", "Other red creatures you control get +1/+1.")
        ["static_abilities"]
        .clone();
    assert_eq!(
        dropped[0]["description"],
        "red creatures you control get +1/+1."
    );
}

#[test]
fn a_cant_be_regenerated_rider_folds_into_the_destruction_before_it() {
    // CR 701.15b: printed as its own sentence, but recorded as a FIELD.
    let v = abilities(
        "Whatever",
        "Destroy target nonblack creature. It can't be regenerated.",
    );
    assert_eq!(v.as_array().expect("array").len(), 1);
    assert_eq!(v[0]["effect"]["cant_regenerate"], true);
    assert_eq!(
        v[0]["sub_ability"],
        json!(null),
        "the rider is not a second effect"
    );
}

#[test]
fn a_pronoun_subject_declines_because_its_referent_is_not_local() {
    // "It" means the chosen target after "Untap target creature" and the SOURCE
    // after "Whenever this creature attacks". Which one is decided by whether
    // an earlier clause chose a target — context the subject grammar does not
    // have. Declining is honest; guessing would silently mis-aim the effect.
    let p = parse_card(
        "Aim High",
        "Untap target creature. It gets +2/+2 until end of turn.",
    );
    assert!(!p.is_complete());
}

// ---------------------------------------------------------------------------
// Parse context: the same words, two referents
// ---------------------------------------------------------------------------

#[test]
fn a_back_reference_resolves_in_a_spell_body_and_declines_in_a_trigger() {
    // "That creature" means the target an earlier clause chose inside a SPELL
    // body. Inside a TRIGGER it means the object the event was about, which the
    // engine spells three different ways depending on the event
    // (`TriggeringSource`, `EventTarget`, `ParentTarget`). Picking one would be
    // a guess, so the trigger declines and the gap stays visible.
    let spell = abilities(
        "Burning Cloak",
        "Target creature gets +2/+0 until end of turn. ~ deals 2 damage to that creature.",
    );
    assert_eq!(
        spell[0]["sub_ability"]["effect"]["target"],
        json!({"type": "ParentTarget"})
    );

    let trigger = parse_card(
        "Dinosaur Hunter",
        "Whenever this creature deals damage to a Dinosaur, destroy that creature.",
    );
    assert!(!trigger.is_complete());
}

#[test]
fn a_positional_subtype_must_also_be_a_real_subtype() {
    // "Goblin creature" and "that creature" have the SAME shape: a word before
    // a core type. Position alone once made "destroy that creature" parse as a
    // creature of subtype "That", which is exactly the kind of silent wrong
    // answer the vocabulary check exists to stop.
    let real = abilities("Whatever", "Destroy target Goblin creature.");
    assert_eq!(
        real[0]["effect"]["target"]["type_filters"],
        json!(["Creature", {"Subtype": "Goblin"}])
    );

    // "that creature" takes the back-reference reading, NOT a creature of
    // subtype "That" — which is what position alone used to produce.
    let back_ref = abilities("Whatever", "Destroy that creature.");
    assert_eq!(
        back_ref[0]["effect"]["target"],
        json!({"type": "ParentTarget"})
    );
}

#[test]
fn one_grant_verb_can_carry_a_list_of_keywords() {
    // "gains lifelink and hexproof" is ONE grant of two keywords, not two
    // grants — the verb is printed once.
    let v = abilities(
        "Whatever",
        "Target creature gets +2/+2 and gains lifelink and hexproof until end of turn.",
    );
    assert_eq!(
        v[0]["effect"]["static_abilities"][0]["modifications"],
        json!([
            {"type": "AddPower", "value": 2},
            {"type": "AddToughness", "value": 2},
            {"type": "AddKeyword", "keyword": "Lifelink"},
            {"type": "AddKeyword", "keyword": "Hexproof"}
        ])
    );
}

#[test]
fn a_conjunction_that_names_a_new_subject_is_a_new_clause() {
    // "target player loses 4 life AND YOU GAIN 4 life" switches subject, so it
    // is a second clause. Contrast the keyword case above, where the subject
    // carries and the whole thing stays one clause.
    let v = abilities(
        "Whatever",
        "Target player loses 4 life and you gain 4 life.",
    );
    assert_eq!(v[0]["effect"]["type"], "LoseLife");
    assert_eq!(v[0]["effect"]["target"], json!({"type": "Player"}));
    assert_eq!(v[0]["sub_ability"]["effect"]["type"], "GainLife");
    // GainLife by the controller omits its player field entirely.
    assert!(v[0]["sub_ability"]["effect"].get("player").is_none());
}

#[test]
fn a_tagged_ability_word_keeps_its_tag_but_loses_its_word() {
    // CR 702.142b: "boast" and friends look like ability words but name a
    // CLASS of ability other cards refer to, so the engine drops the word from
    // the prose and keeps a tag.
    let v = abilities(
        "Aerial Doombot",
        "Power-up — {5}{U}: Put three +1/+1 counters on ~.",
    );
    assert_eq!(v[0]["ability_tag"], json!({"type": "PowerUp"}));
    assert_eq!(
        v[0]["description"],
        "{5}{U}: Put three +1/+1 counters on ~."
    );

    // An untagged ability word leaves nothing behind.
    let plain = triggers(
        "Whatever",
        "Magecraft — Whenever you cast a spell, draw a card.",
    );
    assert_eq!(
        plain[0]["description"],
        "Whenever you cast a spell, draw a card."
    );
}

// ---------------------------------------------------------------------------
// Modal spells
// ---------------------------------------------------------------------------

#[test]
fn a_modal_spell_records_its_counts_and_lowers_each_mode_to_an_ability() {
    // CR 700.2. The modes are ordinary abilities — the modal block only says
    // how many of them get chosen, which is why each mode's own `description`
    // is null while their printed text is repeated in `mode_descriptions`.
    let v = parsed(
        "Cryptic Command",
        "Choose two —\n• Counter target spell.\n• Draw a card.\n• Tap all creatures your opponents control.",
    );
    assert_eq!(
        v["modal"],
        json!({
            "min_choices": 2,
            "max_choices": 2,
            "mode_count": 3,
            "mode_descriptions": [
                "Counter target spell.",
                "Draw a card.",
                "Tap all creatures your opponents control."
            ],
            "allow_repeat_modes": false,
            "chooser": {"type": "Controller"}
        })
    );
    assert_eq!(v["abilities"].as_array().expect("array").len(), 3);
    assert_eq!(v["abilities"][0]["effect"]["type"], "Counter");
    assert_eq!(v["abilities"][0]["description"], json!(null));
}

#[test]
fn choose_one_or_more_caps_at_the_number_of_modes_printed() {
    let v = parsed(
        "Whatever",
        "Choose one or more —\n• Draw a card.\n• You gain 2 life.",
    );
    assert_eq!(v["modal"]["min_choices"], 1);
    assert_eq!(v["modal"]["max_choices"], 2);
    assert_eq!(v["modal"]["mode_count"], 2);
}

#[test]
fn choose_one_or_both_is_not_choose_one() {
    let one = parsed(
        "Whatever",
        "Choose one —\n• Draw a card.\n• You gain 2 life.",
    );
    assert_eq!(one["modal"]["max_choices"], 1);

    let both = parsed(
        "Whatever",
        "Choose one or both —\n• Draw a card.\n• You gain 2 life.",
    );
    assert_eq!(both["modal"]["max_choices"], 2);
}

#[test]
fn an_instruction_that_merely_starts_with_choose_is_not_a_modal_header() {
    // The em dash is what makes it a header. Without one this is an ordinary
    // instruction, and the grammar has no production for it yet.
    let p = parse_card("Whatever", "Choose a creature type.");
    assert!(p.out.modal.is_none());
    assert!(!p.is_complete());
}

#[test]
fn a_bullet_with_no_header_declines_rather_than_floating_free() {
    let p = parse_card("Whatever", "• Draw a card.");
    assert!(!p.is_complete());
    assert!(p.out.abilities.is_empty());
}
