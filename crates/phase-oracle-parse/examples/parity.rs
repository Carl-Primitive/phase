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

/// Does some printed line consist of nothing but reminder text?
fn reminder_only_line(text: &str) -> bool {
    text.lines().any(|l| {
        let t = l.trim();
        t.starts_with('(') && t.ends_with(')')
    })
}

/// A cheap deterministic spread, so samples come from across the corpus rather
/// than from the first cards alphabetically.
fn fastrand_ish(name: &str) -> bool {
    name.bytes().map(usize::from).sum::<usize>() % 37 == 0
}

/// Every `type` tag appearing on an effect anywhere inside a bucket.
fn collect_effect_kinds(v: Option<&Value>, out: &mut std::collections::BTreeSet<String>) {
    match v {
        Some(Value::Object(o)) => {
            if let Some(Value::String(t)) = o.get("type") {
                out.insert(t.clone());
            }
            for inner in o.values() {
                collect_effect_kinds(Some(inner), out);
            }
        }
        Some(Value::Array(a)) => {
            for inner in a {
                collect_effect_kinds(Some(inner), out);
            }
        }
        _ => {}
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
    let mut improvements = 0usize;
    let mut type_line = 0usize;
    let mut declined_lines = 0usize;
    let mut by_production: BTreeMap<&'static str, usize> = BTreeMap::new();
    // Every Nth decline per production, so the sample spans the corpus instead
    // of being the first twenty cards alphabetically.
    let mut decline_samples: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    let mut blocked_by: BTreeMap<String, usize> = BTreeMap::new();
    let mut near_miss: BTreeMap<String, usize> = BTreeMap::new();
    let mut near_samples: Vec<String> = Vec::new();
    let mut one_line_short = 0usize;
    let blocking = std::env::var("PARITY_BLOCKING").ok();
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
            // Group samples by the line's HEAD WORD rather than only by
            // production: a decline on a verb the grammar already has is a
            // near-miss worth far more than one on a verb it has never seen.
            let head = d
                .text
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase();
            if std::env::var("PARITY_HEAD").ok().as_deref() == Some(head.as_str()) {
                let bucket = decline_samples.entry(d.production).or_default();
                if bucket.len() < 25 {
                    bucket.push(d.text.chars().take(110).collect());
                }
            } else if std::env::var("PARITY_HEAD").is_err() && *seen % 97 == 1 {
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
            // What is BLOCKING this card? The engine's own effect vocabulary
            // for a card the grammar could not finish is the most direct work
            // list there is: it names what to build, ranked by how many cards
            // each unlock would reach.
            // Cards blocked by exactly ONE line are the near misses: each is a
            // single production away from being comparable at all.
            if p.declines.len() == 1 {
                one_line_short += 1;
                let d = &p.declines[0];
                let head = d
                    .text
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_matches(|c: char| !c.is_alphanumeric() && c != '{' && c != '~')
                    .to_lowercase();
                *near_miss
                    .entry(format!("{:<16} {head}", d.production))
                    .or_default() += 1;
                if near_samples.len() < 40 && fastrand_ish(name) {
                    near_samples.push(d.text.chars().take(96).collect::<String>());
                }
            }
            if blocking.is_some() {
                let mut kinds = std::collections::BTreeSet::new();
                for bucket in ["abilities", "triggers", "static_abilities"] {
                    collect_effect_kinds(card.get(bucket), &mut kinds);
                }
                for k in kinds {
                    *blocked_by.entry(k).or_default() += 1;
                }
            }
            continue;
        }
        complete += 1;

        // A bucket the grammar never emits must also be empty on their side,
        // otherwise "complete" would be claiming a card whose replacements we
        // silently dropped.
        let untouched_buckets = ["additional_cost"];
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
        let rep_ok = if p.out.replacements.is_empty() {
            card.get("replacements").is_none()
        } else {
            mine(&p.out.replacements) == arr(card, "replacements")
        };
        let modal_ok = match &p.out.modal {
            None => card.get("modal").is_none(),
            Some(m) => mine(m) == arr(card, "modal"),
        };

        if kw_ok && ab_ok && tr_ok && st_ok && modal_ok && rep_ok && their_extra.is_none() {
            exact += 1;
            continue;
        }

        // A card the OLD parser lowered to something, where the new parser
        // claims completeness but disagrees. This is the stop-the-line bucket.
        // The engine emits `Unimplemented` where ITS parser gave up. Where we
        // produce a real parse instead, that is a WIN, not a regression, and
        // counting it as a disagreement would hide the thing this rewrite
        // exists to do. (+2 Mace: the engine's name-normalization eats "+2/+2"
        // into "~/~" and the line fails its static parser.)
        let theirs_unimplemented = [
            arr(card, "abilities"),
            arr(card, "triggers"),
            arr(card, "static_abilities"),
        ]
        .iter()
        .any(|v| {
            serde_json::to_string(v)
                .unwrap_or_default()
                .contains("\"Unimplemented\"")
        });

        let bucket = if theirs_unimplemented {
            improvements += 1;
            "engine declined, we parsed"
        } else if reminder_only_line(text) && !ab_ok {
            // A line that is ENTIRELY reminder text carries no rules content,
            // yet the engine has abilities for it — they come from the card's
            // TYPE LINE (a dual land's mana abilities), which this parser is
            // never given. Not a grammar gap: the information is not in the
            // input. Named separately so it cannot be mistaken for one.
            "type-line ability, not in the text"
        } else if let Some(b) = their_extra {
            match *b {
                _ => "dropped: additional_cost",
            }
        } else if !kw_ok {
            "keywords differ"
        } else if !tr_ok {
            "triggers differ"
        } else if !st_ok {
            "static abilities differ"
        } else if !modal_ok {
            "modal differs"
        } else if !rep_ok {
            "replacements differ"
        } else {
            "abilities differ"
        };
        // Two buckets are NOT grammar defects and are counted apart from the
        // stop-the-line number, which exists to mean "a card the engine gets
        // right and we get wrong":
        //   - the engine itself declined the card;
        //   - the engine's abilities come from the card's TYPE LINE, which this
        //     parser is never given.
        // Everything else counts.
        match bucket {
            "engine declined, we parsed" => {}
            "type-line ability, not in the text" => type_line += 1,
            _ => regressions += 1,
        }
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
                "modal differs" => ("modal", mine(&p.out.modal), arr(card, "modal")),
                "replacements differ" => (
                    "replacements",
                    mine(&p.out.replacements),
                    arr(card, "replacements"),
                ),
                _ => ("abilities", mine(&p.out.abilities), arr(card, "abilities")),
            };
            examples.push(format!(
                "--- {name} [{bucket}]\n    text:  {}\n    {k} mine:   {}\n    {k} their:  {}",
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
    println!("  of those, engine declined, we parsed {improvements}");
    println!("  of those, type-line only (not in text) {type_line}");
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

    if blocking.is_some() {
        println!("\nengine effect kinds on cards the grammar could NOT finish:");
        let mut v: Vec<_> = blocked_by.iter().collect();
        v.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
        for (k, n) in v.into_iter().take(40) {
            println!("  {n:>7}  {k}");
        }
    }

    println!("\ncards one line short of parsing      {one_line_short}");
    if blocking.is_some() {
        println!("\nnear misses by production and head:");
        let mut v: Vec<_> = near_miss.iter().collect();
        v.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
        for (k, n) in v.into_iter().take(28) {
            println!("  {n:>6}  {k}");
        }
        println!("\nnear-miss samples:");
        for s in near_samples.iter().take(24) {
            println!("   {s}");
        }
    }

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
