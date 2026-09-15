//! Versioned, engine-independent card definition schema.
//!
//! This crate is the published contract between the Oracle parser and any
//! consumer. It deliberately shares no types with a game engine: an engine may
//! refactor its internal representation freely without invalidating a corpus
//! of definitions built against this schema, and a definition may be
//! hand-edited without knowing what any engine does with it.
//!
//! Compatibility rule: within a major version, consumers must tolerate unknown
//! variants and absent optional fields. [`SCHEMA_VERSION`] moves with the shape
//! of these types, never with the parser that fills them.

pub mod effect;
pub mod filter;
pub mod quantity;

pub use effect::{Duration, Effect, PtChange};
pub use filter::{CardZone, Controller, ObjectFilter, Target, TypeName};
pub use quantity::{CounterKind, Quantity};

use serde::{Deserialize, Serialize};

/// Semantic version of the schema shape.
pub const SCHEMA_VERSION: &str = "0.1.0";

/// One parsed clause together with the source span it came from.
///
/// The span is part of the contract, not a debugging aid: it is what lets a
/// consumer show which words produced a definition, and what lets the parser's
/// totality check be stated in terms a reader can verify.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clause {
    pub effect: Effect,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<Duration>,
    pub source: SourceSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub start: usize,
    pub end: usize,
}

impl SourceSpan {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}
