//! veridoc wire types and JSON-RPC framing. No policy lives here.

pub mod contract;
pub mod rpc;

use std::path::PathBuf;

pub use contract::*;
pub use rpc::*;

/// The only protocol revision this build speaks.
pub const PROTOCOL_VERSION: &str = "veridoc/1";
/// Revisions this build accepts from a plugin. A plugin reporting anything else is refused at
/// connect time, not misread at message time.
pub const SUPPORTED_VERSIONS: &[&str] = &[PROTOCOL_VERSION];

/// The committed schema directory for a protocol revision: `veridoc/1` becomes `veridoc-1`.
pub fn schema_dir(version: &str) -> PathBuf {
    PathBuf::from(version.replace('/', "-"))
}
