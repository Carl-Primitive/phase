//! How close is the schema to being a drop-in for the existing parser's output?
//!
//! Usage: cargo run --release --example bridge_diff --features corpus -- <corpus.json>

use phase_card_schema::Effect;
use phase_oracle_parse::{bridge::bridge, parse_card};
use serde_json::Value;
use std::collections::BTreeMap;

/// Compare only the fields both representations carry. `description` is
/// excluded: it is prose the engine regenerates, not semantic content.
fn normalize(mut v: Value) -> Value {
    if let Some(obj) = v.as_object_mut() {
        obj.remove("description");
        obj.remove("is_mana_ability");
    }
    v
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: bridge_diff <corpus.json>");
    let raw = std::fs::read_to_string(&path).expect("read corpus");
    let cards: Vec<Value> = serde_json::from_str(&raw).expect("parse corpus");

    let mut fully_parsed = 0usize;
    let mut bridgeable = 0usize;
    let mut exact = 0usize;
    let mut kw_exact = 0usize;
    let mut mismatch_reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut examples: Vec<String> = Vec::new();

    for card in &cards {
        let name = card["n"].as_str().unwrap_or("");
        let text = card["t"].as_str().unwrap_or("");
        let parsed = parse_card(name, text);
        if parsed.is_empty() || parsed.iter().any(|c| matches!(c.effect, Effect::Unparsed { .. })) {
            continue;
        }
        fully_parsed += 1;

        let b = bridge(&parsed);
        if b.unbridged > 0 {
            *mismatch_reasons.entry("no engine counterpart in schema".into()).or_default() += 1;
            continue;
        }
        bridgeable += 1;

        let ref_kw: Vec<String> = card["kw"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let ref_ab: Vec<Value> = card["ab"].as_array().cloned().unwrap_or_default();

        let kw_ok = {
            let mut a = b.keywords.clone();
            let mut c = ref_kw.clone();
            a.sort();
            c.sort();
            a == c
        };
        if kw_ok {
            kw_exact += 1;
        }

        let mine: Vec<Value> = b.abilities.iter().cloned().map(normalize).collect();
        let theirs: Vec<Value> = ref_ab.iter().cloned().map(normalize).collect();

        if kw_ok && mine == theirs {
            exact += 1;
        } else {
            let reason = if !kw_ok {
                "keyword list differs"
            } else if mine.len() != theirs.len() {
                "ability count differs"
            } else {
                "ability body differs"
            };
            *mismatch_reasons.entry(reason.into()).or_default() += 1;
            if !kw_ok && examples.len() < 8 {
                examples.push(format!("{name}  mine_kw={:?} theirs_kw={:?}", b.keywords, ref_kw));
            }
            if false {
                examples.push(format!(
                    "{name}\n      mine:   {}\n      theirs: {}",
                    serde_json::to_string(&mine).unwrap_or_default().chars().take(190).collect::<String>(),
                    serde_json::to_string(&theirs).unwrap_or_default().chars().take(190).collect::<String>()
                ));
            }
        }
    }

    println!("cards fully parsed by the new grammar   {fully_parsed}");
    println!("  of those, fully bridgeable to engine  {bridgeable}");
    println!("  keyword list matches exactly          {kw_exact}");
    println!("  WHOLE CARD matches engine output      {exact}");
    println!(
        "  exact-match rate among bridgeable     {:.1}%",
        100.0 * exact as f64 / bridgeable.max(1) as f64
    );
    println!("\nmismatch reasons:");
    let mut m: Vec<_> = mismatch_reasons.iter().collect();
    m.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
    for (k, n) in m {
        println!("  {n:>7}  {k}");
    }
    println!("\nbody-difference examples:");
    for e in &examples {
        println!("  - {e}");
    }
}
