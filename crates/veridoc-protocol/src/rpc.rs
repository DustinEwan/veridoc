//! JSON-RPC 2.0 framing: one message per line on stdio, request `id` as `u64`, eight methods.
//!
//! The framing itself — the envelope, the error codes, the line reader and writer, the loss
//! reasons — and the stdio-process transport that starts a plugin and supervises it live in the
//! `plugin_rpc` mechanism crate, which veridoc and autofuzz both drive, so both are re-exported
//! here rather than restated. Only the eight method names below are veridoc's own: a plugin
//! author writing in Rust imports this module, gets the framing for free, and answers the
//! vocabulary this profile defines. The process transport is re-exported for the host side, so
//! the engine names the mechanism's types through this one path too.

pub use plugin_rpc::{
    buffered, buffered_writer, parse_request, parse_response, read_line, write_line, Call,
    ErrorObject, Failure, JsonRpcVersion, Loss, MessageError, MethodName, Request, Response,
    Success, INVALID_REQUEST, METHOD_NOT_FOUND, PARSE_ERROR, PLUGIN_ERROR, UNUSABLE_RESULT,
};

pub use plugin_rpc::process::{Plugin, PluginConfig, StartError, Timeouts};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The eight methods a veridoc plugin may answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Method {
    Handshake,
    Collect,
    Resolve,
    Derivable,
    Bivalence,
    Run,
    Cite,
    Surface,
}

/// The wire names a veridoc plugin answers. `from_wire` is what a plugin server uses to answer
/// `METHOD_NOT_FOUND` for a name outside this vocabulary.
impl MethodName for Method {
    fn name(&self) -> &'static str {
        match self {
            Method::Handshake => "handshake",
            Method::Collect => "collect",
            Method::Resolve => "resolve",
            Method::Derivable => "derivable",
            Method::Bivalence => "bivalence",
            Method::Run => "run",
            Method::Cite => "cite",
            Method::Surface => "surface",
        }
    }

    fn from_wire(name: &str) -> Option<Self> {
        match name {
            "handshake" => Some(Method::Handshake),
            "collect" => Some(Method::Collect),
            "resolve" => Some(Method::Resolve),
            "derivable" => Some(Method::Derivable),
            "bivalence" => Some(Method::Bivalence),
            "run" => Some(Method::Run),
            "cite" => Some(Method::Cite),
            "surface" => Some(Method::Surface),
            _ => None,
        }
    }
}
