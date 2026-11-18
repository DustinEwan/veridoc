//! Parses the `export interface VeridocPlugin { … }` block out of `docs/spec.md` and compares
//! its method names, its `capabilities` union members, and its `protocolVersion` literal with
//! the Rust vocabulary in `veridoc-protocol`. `docs/spec.md` is the authority for the plugin
//! contract, so the spec text cannot drift from the compiled contract without this check
//! failing.

use std::path::Path;

use veridoc_protocol::{Capability, Method, MethodName, PROTOCOL_VERSION};

/// Method names as the Rust vocabulary spells them on the wire, in declaration order.
fn protocol_methods() -> Vec<&'static str> {
    [
        Method::Handshake,
        Method::Init,
        Method::Collect,
        Method::Resolve,
        Method::Derivable,
        Method::Bivalence,
        Method::Run,
        Method::Cite,
        Method::Surface,
    ]
    .iter()
    .map(Method::name)
    .collect()
}

fn protocol_capabilities() -> Vec<String> {
    [
        Capability::Initialization,
        Capability::Collect,
        Capability::Resolution,
        Capability::Derivability,
        Capability::Bivalence,
        Capability::Execution,
        Capability::Citation,
        Capability::Surface,
    ]
    .iter()
    .map(|c| {
        serde_json::to_value(c)
            .expect("a capability is a string")
            .as_str()
            .expect("a capability serializes to a string")
            .to_string()
    })
    .collect()
}

/// Quoted string literals on one line, in order.
fn quoted(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            match bytes[i + 1..].iter().position(|&b| b == b'"') {
                Some(rel) => {
                    out.push(line[i + 1..i + 1 + rel].to_string());
                    i += rel + 2;
                }
                None => break,
            }
        } else {
            i += 1;
        }
    }
    out
}

pub fn check_spec(spec: &Path) -> Result<(), String> {
    let text =
        std::fs::read_to_string(spec).map_err(|e| format!("read {}: {e}", spec.display()))?;
    let block = extract_interface(&text).ok_or_else(|| {
        format!(
            "no `export interface VeridocPlugin {{` block found in {}",
            spec.display()
        )
    })?;

    let mut problems = Vec::new();

    // Method declarations sit at two-space indentation inside the interface body.
    let spec_methods: Vec<String> = block
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("  ")?;
            let name = rest.split('(').next()?;
            if name.chars().all(|c| c.is_ascii_alphabetic()) && !name.is_empty() {
                Some(name.to_string())
            } else {
                None
            }
        })
        .collect();
    let want = protocol_methods();
    if spec_methods != want {
        problems.push(format!(
            "spec methods {spec_methods:?} do not match the protocol methods {want:?}"
        ));
    }

    let caps_line = block
        .lines()
        .find(|l| l.contains("capabilities: Capability[]"))
        .ok_or("no `capabilities: Capability[]` line in the interface block")?;
    let spec_caps = quoted(caps_line);
    let want_caps = protocol_capabilities();
    if spec_caps != want_caps {
        problems.push(format!(
            "spec capabilities {spec_caps:?} do not match the protocol capabilities {want_caps:?}"
        ));
    }

    let version_line = block
        .lines()
        .find(|l| l.contains("protocolVersion: string"))
        .ok_or("no `protocolVersion: string` line in the interface block")?;
    match quoted(version_line).first().map(String::as_str) {
        Some(PROTOCOL_VERSION) => {}
        other => problems.push(format!(
            "spec protocolVersion {other:?} does not match the protocol version {PROTOCOL_VERSION:?}"
        )),
    }

    if problems.is_empty() {
        println!("contract ok: docs/spec.md matches veridoc-protocol");
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

/// The text between `export interface VeridocPlugin {` and the closing `}` at column zero.
fn extract_interface(text: &str) -> Option<String> {
    let start = text.find("export interface VeridocPlugin {")?;
    let body = &text[start..];
    let end = body.find("\n}")?;
    Some(body[..end].to_string())
}
