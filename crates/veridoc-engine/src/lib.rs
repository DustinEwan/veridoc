//! veridoc policy: bind, gates, outcomes, brief, judge, cache, result, formatters.
//!
//! The engine never reads a source file of the target language, and it owns no scenario grammar:
//! the only file formats it reads are `.veridoc/config.json`, `.spec.md`,
//! `.veridoc/verdicts/*.json`, and the artefact and citation-store files its own claims name. The
//! `tests/no_language_knowledge.rs` test asserts that boundary mechanically. The one place the
//! engine touches a project source file is `inputs`, which hashes every tracked file, parsing
//! none of them, to answer whether this checkout is the one the last run read.

pub mod artefact;
pub mod bind;
pub mod bivalence;
pub mod brief;
pub mod cite;
pub mod config;
pub mod derivability;
pub mod diagnostic;
pub mod execute;
pub mod format;
pub mod git;
pub mod hash;
pub mod inputs;
pub mod install;
pub mod judge;
pub mod pipeline;
pub mod plugin;
pub mod plugins;
pub mod result;
pub mod spec_md;
pub mod verdicts;
