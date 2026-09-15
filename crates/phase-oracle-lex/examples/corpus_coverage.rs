//! Run the lexer over every card in the corpus and assert total byte coverage.
//!
//! Usage: cargo run --example corpus_coverage --features corpus -- <corpus.json>

use phase_oracle_lex::{lex, verify_coverage, TokenKind};
use std::collections::BTreeMap;

fn main() {
    let path = std::env::args().nth(1).expect("usage: corpus_coverage <corpus.json>");
    let raw = std::fs::read_to_string(&path).expect("read corpus");
    let cards: Vec<serde_json::Value> = serde_json::from_str(&raw).expect("parse corpus");

    let mut cards_seen = 0usize;
    let mut tokens_total = 0usize;
    let mut failures = Vec::new();
    let mut kinds: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut unterminated_reminder = 0usize;
    let mut unterminated_quote = 0usize;
    let mut other_chars: BTreeMap<String, usize> = BTreeMap::new();

    for card in &cards {
        let name = card["n"].as_str().unwrap_or("<unnamed>");
        let text = card["t"].as_str().unwrap_or("");
        cards_seen += 1;

        let tokens = lex(text);
        tokens_total += tokens.len();

        for t in &tokens {
            let label = match t.kind {
                TokenKind::Word => "Word",
                TokenKind::Number => "Number",
                TokenKind::Symbol => "Symbol",
                TokenKind::Loyalty { .. } => "Loyalty",
                TokenKind::PtPair { .. } => "PtPair",
                TokenKind::Reminder { terminated } => {
                    if !terminated { unterminated_reminder += 1; }
                    "Reminder"
                }
                TokenKind::Quoted { terminated } => {
                    if !terminated { unterminated_quote += 1; }
                    "Quoted"
                }
                TokenKind::Bullet => "Bullet",
                TokenKind::EmDash => "EmDash",
                TokenKind::Period => "Period",
                TokenKind::Comma => "Comma",
                TokenKind::Semicolon => "Semicolon",
                TokenKind::Colon => "Colon",
                TokenKind::Slash => "Slash",
                TokenKind::Plus => "Plus",
                TokenKind::Hyphen => "Hyphen",
                TokenKind::Pipe => "Pipe",
                TokenKind::Newline => "Newline",
                TokenKind::Other => {
                    *other_chars.entry(t.text(text).to_string()).or_default() += 1;
                    "Other"
                }
            };
            *kinds.entry(label).or_default() += 1;
        }

        if let Err(e) = verify_coverage(text, &tokens) {
            failures.push((name.to_string(), e));
        }
    }

    println!("cards lexed          {cards_seen}");
    println!("tokens emitted       {tokens_total}");
    println!("coverage failures    {}", failures.len());
    println!("unterminated ()      {unterminated_reminder}");
    println!("unterminated \"\"      {unterminated_quote}");
    println!("\ntoken kind census:");
    let mut by_count: Vec<_> = kinds.iter().collect();
    by_count.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
    for (k, n) in by_count {
        println!("  {n:>9}  {k}");
    }
    if !other_chars.is_empty() {
        println!("\nOther-token characters ({} distinct):", other_chars.len());
        let mut o: Vec<_> = other_chars.iter().collect();
        o.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
        for (c, n) in o.iter().take(25) {
            println!("  {n:>7}  {c:?}");
        }
    }
    for (name, e) in failures.iter().take(15) {
        println!("\nFAIL {name}: {e:?}");
    }
    if !failures.is_empty() {
        std::process::exit(1);
    }
}
