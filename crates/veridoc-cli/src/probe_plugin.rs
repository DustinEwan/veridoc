//! `veridoc probe-plugin`: the transport proof. Spawn, handshake, collect, resolve, and
//! print the claim and referent tables so a harness author sees exactly what the engine
//! sees.

use std::path::Path;
use std::time::Duration;

use serde_json::Value;
use veridoc_engine::bind::CollectWire;
use veridoc_engine::config;
use veridoc_engine::plugin::{sentence, PluginClient};
use veridoc_protocol::Method;

pub fn run(project: &Path) -> Result<i32, String> {
    let path = config::discover(project).map_err(|e| e.to_string())?;
    let loaded = config::load(&path).map_err(|e| e.to_string())?;
    let root = &loaded.root;
    let launch = loaded.launch().map_err(|e| e.to_string())?;
    let (client, handshake) = match PluginClient::connect(
        &launch,
        Duration::from_millis(loaded.config.plugin.handshake_timeout_ms),
    ) {
        Ok(pair) => pair,
        Err(lost) => {
            eprintln!("run failed: plugin: {lost}");
            return Ok(2);
        }
    };
    println!(
        "plugin {} {} protocol {} languages [{}] capabilities [{}]",
        handshake.name,
        handshake.version,
        handshake.protocol_version,
        handshake.languages.join(", "),
        handshake
            .capabilities
            .iter()
            .map(|c| format!("{c:?}").to_lowercase())
            .collect::<Vec<_>>()
            .join(", "),
    );

    let raw = match sentence(
        "collect",
        client.request::<Value>(
            Method::Collect,
            Some(serde_json::json!({ "root": root.to_string_lossy() })),
            Duration::from_millis(loaded.config.plugin.collect_timeout_ms),
        ),
    ) {
        Ok(raw) => raw,
        Err(why) => {
            eprintln!("run failed: plugin: {why}");
            return Ok(2);
        }
    };
    let collect: CollectWire = serde_json::from_value(raw).map_err(|e| e.to_string())?;

    let mut keys: Vec<String> = Vec::new();
    for claim in &collect.claims {
        let key = claim.subject.key();
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    let resolved = match sentence("resolve", client.resolve(&keys)) {
        Ok(resolved) => resolved,
        Err(why) => {
            eprintln!("run failed: plugin: {why}");
            return Ok(2);
        }
    };

    println!("claims:");
    for claim in &collect.claims {
        println!(
            "  {}  {:?}  {}:{}  {}",
            claim.id, claim.kind, claim.file, claim.line, claim.text
        );
    }
    println!("referents:");
    for reference in &resolved.resolved {
        println!(
            "  {}  {}  {}",
            reference.referent_id, reference.signature, reference.reference
        );
    }
    for gap in &resolved.unresolved {
        println!("  unresolved  {}  {}", gap.reference, gap.reason);
    }
    Ok(0)
}
