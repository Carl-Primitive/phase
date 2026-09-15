//! Parse one card from the command line and show what happened.
//!
//! Usage: cargo run --example explain -- "Card Name" "Oracle text"
//!
//! The fastest way to turn a corpus-level decline count back into a concrete
//! production to fix: it prints the lowered JSON and every refusal with the
//! production that made it.

use phase_oracle_parse::parse_card;

fn main() {
    let mut args = std::env::args().skip(1);
    let name = args.next().unwrap_or_default();
    let text = args.next().unwrap_or_default();

    let p = parse_card(&name, &text);

    println!("== {name}");
    for line in text.lines() {
        println!("   | {line}");
    }

    if p.out.is_empty() {
        println!("\n-- nothing lowered --");
    } else {
        println!(
            "\n{}",
            serde_json::to_string_pretty(&p.out).expect("serialize")
        );
    }

    if p.declines.is_empty() {
        println!("\nno declines: every line was claimed");
    } else {
        println!("\ndeclines:");
        for d in &p.declines {
            println!(
                "   {:<18} {:?}  [{}..{}]  {}",
                d.production, d.reason, d.start, d.end, d.text
            );
        }
    }
}
