//! veridoc policy: bind, gates, outcomes, brief, judge, cache, result, formatters.
//!
//! The engine never reads a source file of the target language. The only file formats it
//! reads are `.veridoc/config.json`, `.spec.md`, `.feature`, `.veridoc/verdicts/*.json`, and
//! the artefact and citation-store files its own claims name. The
//! `tests/no_language_knowledge.rs` test asserts that boundary mechanically.

pub mod artefact;
pub mod bivalence;
pub mod bind;
pub mod brief;
pub mod cite;
pub mod config;
pub mod diagnostic;
pub mod format;
pub mod derivability;
pub mod execute;
pub mod feature;
pub mod git;
pub mod hash;
pub mod install;
pub mod judge;
pub mod plugin;
pub mod plugins;
pub mod pipeline;
pub mod result;
pub mod spec_md;
pub mod verdicts;
