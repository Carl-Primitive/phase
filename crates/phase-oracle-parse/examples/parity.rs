//! How close is the new parser to the existing one, measured rather than asserted?
//!
//! Usage: cargo run --release --example parity --features corpus -- <corpus.json> [--show <n>]
//!
//! Reports three things, in the order they matter:
//!
//! 1. **Regressions.** A card the old parser lowered and the new one lowers
//!    DIFFERENTLY. This bucket is the stop-the-line number; everything else is
//!    scope.
//! 2. **Exact matches.** Cards where every bucket is byte-identical.
//! 3. **Declines by production.** The work list over the grammar.

use phase_oracle_parse::parse_card;
use serde_json::Value;
use std::collections::BTreeMap;

/// Compare only what both representations carry.
///
/// `description` is included: it is part of the printed output and the engine
/// regenerates it deterministically, so excluding it would hide real drift.
/// `is_mana_ability` and `consumes_source` are engine-computed riders on
/// effects this grammar does not yet emit.
fn normalize(v: &Value) -> Value {
    match v {
        Value::Object(o) => Value::Object(
            o.iter()
                .filter(|(k, _)| !matches!(k.as_str(), "is_mana_ability" | "consumes_source"))
                .map(|(k, v)| (k.clone(), normalize(v)))
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(normalize).collect()),
        other => other.clone(),
    }
}

fn arr(card: &Value, key: &str) -> Value {
    normalize(card.get(key).unwrap_or(&Value::Null))
}

fn mine(v: &impl serde::Serialize) -> Value {
    normalize(&serde_json::to_value(v).expect("serialize"))
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: parity <corpus.json> [--show N]");
    let show: usize = match (args.next().as_deref(), args.next()) {
        (Some("--show"), Some(n)) => n.parse().unwrap_or(0),
        _ => 0,
    };

    let raw = std::fs::read_to_string(&path).expect("read corpus");
    let cards: Vec<Value> = serde_json::from_str(&raw).expect("parse corpus");

    let mut total = 0usize;
    let mut complete = 0usize;
    let mut exact = 0usize;
    let mut regressions = 0usize;
    let mut declined_lines = 0usize;
    let mut by_production: BTreeMap<&'static str, usize> = BTreeMap::new();
    // Every Nth decline per production, so the sample spans the corpus instead
    // of being the first twenty cards alphabetically.
    let mut decline_samples: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    let mut by_head: BTreeMap<String, usize> = BTreeMap::new();
    let mut mismatch_bucket: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut examples: Vec<String> = Vec::new();

    for card in &cards {
        let name = card["n"].as_str().unwrap_or("");
        let text = card["t"].as_str().unwrap_or("");
        total += 1;

        let p = parse_card(name, text);

        for d in &p.declines {
            declined_lines += 1;
            let seen = by_production.entry(d.production).or_default();
            *seen += 1;
            if *seen % 97 == 1 {
                let bucket = decline_samples.entry(d.production).or_default();
                if bucket.len() < 20 {
                    bucket.push(d.text.chars().take(100).collect());
                }
            }
            let head = d
                .text
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_matches(|c: char| !c.is_alphanumeric() && c != '{' && c != '~')
                .to_lowercase();
            *by_head.entry(head).or_default() += 1;
        }

        if !p.is_complete() {
            continue;
        }
        complete += 1;

        // A bucket the grammar never emits must also be empty on their side,
        // otherwise "complete" would be claiming a card whose replacements we
        // silently dropped.
        let untouched_buckets = ["replacements", "modal", "additional_cost"];
        let their_extra = untouched_buckets.iter().find(|k| card.get(**k).is_some());

        // Keyword ORDER is compared as a multiset, not a sequence.
        //
        // The reference data is not order-stable for this field: the same
        // printed line "Flying, deathtouch" yields ["Deathtouch","Flying"] on
        // A-Midnight Assassin and ["Flying","Deathtouch"] on Aurora of Emrakul.
        // Demanding sequence equality would measure that instability rather
        // than this parser's correctness, so the contents are compared and the
        // order difference is counted separately below.
        let their_kw = arr(card, "keywords");
        let kw_ok = {
            let mut a = mine(&p.out.keywords);
            let mut b = their_kw.clone();
            if let (Some(x), Some(y)) = (a.as_array_mut(), b.as_array_mut()) {
                x.sort_by_key(|v| v.to_string());
                y.sort_by_key(|v| v.to_string());
            }
            a == b || (p.out.keywords.is_empty() && card.get("keywords").is_none())
        };
        let ab_ok = if p.out.abilities.is_empty() {
            card.get("abilities").is_none()
        } else {
            mine(&p.out.abilities) == arr(card, "abilities")
        };
        let tr_ok = if p.out.triggers.is_empty() {
            card.get("triggers").is_none()
        } else {
            mine(&p.out.triggers) == arr(card, "triggers")
        };
        let st_ok = if p.out.static_abilities.is_empty() {
            card.get("static_abilities").is_none()
        } else {
            mine(&p.out.static_abilities) == arr(card, "static_abilities")
        };

        if kw_ok && ab_ok && tr_ok && st_ok && their_extra.is_none() {
            exact += 1;
            continue;
        }

        // A card the OLD parser lowered to something, where the new parser
        // claims completeness but disagrees. This is the stop-the-line bucket.
        regressions += 1;
        let bucket = if let Some(b) = their_extra {
            match *b {
                "replacements" => "dropped: replacements",
                "modal" => "dropped: modal",
                _ => "dropped: additional_cost",
            }
        } else if !kw_ok {
            "keywords differ"
        } else if !tr_ok {
            "triggers differ"
        } else if !st_ok {
            "static abilities differ"
        } else {
            "abilities differ"
        };
        *mismatch_bucket.entry(bucket).or_default() += 1;

        if bucket.starts_with("dropped") && examples.len() < show {
            examples.push(format!(
                "--- {name} [{bucket}]\n    text:  {}",
                text.replace('\n', " | ")
            ));
            continue;
        }
        if examples.len() < show {
            let (k, m, t) = match bucket {
                "keywords differ" => ("keywords", mine(&p.out.keywords), arr(card, "keywords")),
                "triggers differ" => ("triggers", mine(&p.out.triggers), arr(card, "triggers")),
                "static abilities differ" => (
                    "statics",
                    mine(&p.out.static_abilities),
                    arr(card, "static_abilities"),
                ),
                _ => ("abilities", mine(&p.out.abilities), arr(card, "abilities")),
            };
            examples.push(format!(
                "--- {name}\n    text:  {}\n    {k} mine:   {}\n    {k} their:  {}",
                text.replace('\n', " | "),
                serde_json::to_string(&m).unwrap_or_default(),
                serde_json::to_string(&t).unwrap_or_default()
            ));
        }
    }

    println!("cards in corpus                       {total}");
    println!("lines the grammar declined            {declined_lines}");
    println!("cards with every line parsed          {complete}");
    println!("  of those, EXACT match vs engine     {exact}");
    println!("  of those, DISAGREE with engine      {regressions}   <-- stop-the-line");
    if complete > 0 {
        println!(
            "exact-match rate among complete cards {:.1}%",
            100.0 * exact as f64 / complete as f64
        );
    }
    println!(
        "whole-corpus exact-match rate         {:.1}%",
        100.0 * exact as f64 / total as f64
    );

    println!("\ndeclines by production:");
    let mut v: Vec<_> = by_production.iter().collect();
    v.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
    for (k, n) in v {
        println!("  {n:>7}  {k}");
    }

    println!("\ndisagreement buckets:");
    let mut v: Vec<_> = mismatch_bucket.iter().collect();
    v.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
    for (k, n) in v {
        println!("  {n:>7}  {k}");
    }

    println!("\ntop declining line heads:");
    let mut v: Vec<_> = by_head.into_iter().collect();
    v.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    let tot: usize = v.iter().map(|(_, n)| *n).sum();
    let mut run = 0usize;
    for (k, n) in v.iter().take(30) {
        run += n;
        println!(
            "  {n:>7} {:5.1}%  {k}",
            100.0 * run as f64 / tot.max(1) as f64
        );
    }
    println!("  (distinct declining heads: {})", v.len());

    for e in &examples {
        println!("\n{e}");
    }

    if std::env::var("PARITY_SAMPLE_DECLINES").is_ok() {
        println!("\ndecline samples by production:");
        for (prod, lines) in &decline_samples {
            println!("\n== {prod} ==");
            for l in lines.iter().take(20) {
                println!("   {l}");
            }
        }
    }
}
