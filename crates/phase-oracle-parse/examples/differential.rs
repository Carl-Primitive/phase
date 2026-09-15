//! Run the new grammar over the corpus and compare against the existing parser.
//!
//! Usage: cargo run --release --example differential --features corpus -- <corpus.json>

use phase_card_schema::effect::DeclineReason;
use phase_card_schema::Effect;
use phase_oracle_parse::parse_card;
use std::collections::BTreeMap;

fn main() {
    let path = std::env::args().nth(1).expect("usage: differential <corpus.json>");
    let raw = std::fs::read_to_string(&path).expect("read corpus");
    let cards: Vec<serde_json::Value> = serde_json::from_str(&raw).expect("parse corpus");

    let mut clauses_total = 0usize;
    let mut clauses_parsed = 0usize;
    let mut decline: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut effect_census: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut decline_heads: BTreeMap<String, usize> = BTreeMap::new();
    let mut trailing_examples: Vec<String> = Vec::new();

    // Card-level buckets, restricted to the slice the grammar targets:
    // cards whose whole text is plain effect sentences (no trigger, no
    // activated cost, no static). Those are the cards a like-for-like
    // comparison is meaningful on.
    let mut slice_cards = 0usize;
    let mut slice_full = 0usize;
    let mut new_better = Vec::new();
    let mut new_worse = Vec::new();
    let mut agree = 0usize;

    for card in &cards {
        let name = card["n"].as_str().unwrap_or("");
        let text = card["t"].as_str().unwrap_or("");
        let old_supported = card["sup"].as_bool();

        let parsed = parse_card(name, text);
        let mut all_ok = true;
        for c in &parsed {
            clauses_total += 1;
            match &c.effect {
                Effect::Unparsed { reason, .. } => {
                    all_ok = false;
                    let label = match reason {
                        DeclineReason::UnknownVerb => "UnknownVerb",
                        DeclineReason::UnparsedTarget => "UnparsedTarget",
                        DeclineReason::UnparsedQuantity => "UnparsedQuantity",
                        DeclineReason::TrailingTokens => "TrailingTokens",
                    };
                    *decline.entry(label).or_default() += 1;
                    if let Effect::Unparsed { text, .. } = &c.effect {
                        let head: String = text
                            .split_whitespace()
                            .next()
                            .unwrap_or("")
                            .trim_matches(|ch: char| !ch.is_alphanumeric())
                            .to_lowercase();
                        if !head.is_empty() {
                            *decline_heads.entry(head).or_default() += 1;
                        }
                        if *reason == DeclineReason::TrailingTokens && trailing_examples.len() < 3000 {
                            trailing_examples.push(text.clone());
                        }
                    }
                }
                other => {
                    clauses_parsed += 1;
                    let label: &'static str = match other {
                        Effect::Destroy { .. } => "Destroy",
                        Effect::Exile { .. } => "Exile",
                        Effect::DealDamage { .. } => "DealDamage",
                        Effect::Draw { .. } => "Draw",
                        Effect::GainLife { .. } => "GainLife",
                        Effect::LoseLife { .. } => "LoseLife",
                        Effect::ModifyPt { .. } => "ModifyPt",
                        Effect::PutCounter { .. } => "PutCounter",
                        Effect::SetTapped { .. } => "SetTapped",
                        Effect::CounterSpell { .. } => "CounterSpell",
                        Effect::Sacrifice { .. } => "Sacrifice",
                        Effect::Discard { .. } => "Discard",
                        Effect::Mill { .. } => "Mill",
                        Effect::ReturnToHand { .. } => "ReturnToHand",
                        Effect::CreateToken { .. } => "CreateToken",
                        Effect::GainKeyword { .. } => "GainKeyword",
                        Effect::Sequence { .. } => "Sequence",
                        Effect::Unparsed { .. } => unreachable!(),
                    };
                    *effect_census.entry(label).or_default() += 1;
                }
            }
        }

        // Slice membership: plain effect text only.
        let low = text.to_lowercase();
        let in_slice = !parsed.is_empty()
            && !low.contains("when ")
            && !low.contains("whenever ")
            && !low.starts_with("at ")
            && !text.contains(':')
            && !low.contains("as long as")
            && !low.contains("enchant ");
        if in_slice {
            slice_cards += 1;
            if all_ok {
                slice_full += 1;
                match old_supported {
                    Some(false) => new_better.push(name.to_string()),
                    Some(true) => agree += 1,
                    None => {}
                }
            } else if old_supported == Some(true) {
                new_worse.push(name.to_string());
            }
        }
    }

    println!("== clause level, whole corpus ==");
    println!("clauses seen         {clauses_total}");
    println!(
        "clauses parsed       {clauses_parsed}  ({:.1}%)",
        100.0 * clauses_parsed as f64 / clauses_total as f64
    );
    println!("\ndeclines by production:");
    let mut d: Vec<_> = decline.iter().collect();
    d.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
    for (k, n) in d {
        println!("  {n:>8}  {k}");
    }
    println!("\neffects produced:");
    let mut e: Vec<_> = effect_census.iter().collect();
    e.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
    for (k, n) in e {
        println!("  {n:>8}  {k}");
    }

    println!("\ntop 30 declining clause heads:");
    let mut h: Vec<_> = decline_heads.iter().collect();
    h.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
    let head_total: usize = decline_heads.values().sum();
    let mut run = 0usize;
    for (k, n) in h.iter().take(30) {
        run += **n;
        println!("  {n:>7}  {:>5.1}%  {k}", 100.0 * run as f64 / head_total as f64);
    }
    println!("  (distinct declining heads: {})", decline_heads.len());

    println!("\nTrailingTokens examples (grammar matched a verb, could not finish):");
    for t in trailing_examples.iter().take(12) {
        println!("    {}", &t[..t.len().min(96)]);
    }

    println!("\n== card level, within the targeted slice ==");
    println!("slice cards          {slice_cards}");
    println!(
        "fully parsed         {slice_full}  ({:.1}% of slice)",
        100.0 * slice_full as f64 / slice_cards.max(1) as f64
    );
    println!("agree with old       {agree}");
    println!("NEW BETTER           {}  (old said unsupported, new parses fully)", new_better.len());
    println!("NEW WORSE            {}  (old supported, new declines)", new_worse.len());
    for n in new_better.iter().take(10) {
        println!("    better: {n}");
    }
}
