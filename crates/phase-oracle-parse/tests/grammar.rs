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
    // This is the 738-card class the existing parser's post-hoc auditor is
    // blind to. Dropping "with mana value 3 or less" would silently turn a
    // narrow removal spell into an unconditional one, and nothing downstream
    // could tell. Here it is structurally impossible: the qualifier's tokens
    // are unconsumed, so the line declines with a span.
    let p = parse_card(
        "Doom Whisper",
        "Destroy target creature with mana value 3 or less.",
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
