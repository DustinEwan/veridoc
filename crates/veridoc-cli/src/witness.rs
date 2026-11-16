//! `veridoc witness <claim-id>`: the current sha256 of the claim's artefact and nothing
//! else, so the author records a new hash by hand and the engine never writes a project
//! file (`docs/spec.md:617`).

use std::path::Path;
use std::time::Duration;

use serde_json::Value;
use veridoc_engine::artefact::sha256_of;
use veridoc_engine::bind::CollectWire;
use veridoc_engine::config;
use veridoc_engine::hash;
use veridoc_engine::plugin::PluginClient;
use veridoc_protocol::Method;

pub fn run(project: &Path, claim_id: &str) -> Result<i32, String> {
    let path = config::discover(project).map_err(|e| e.to_string())?;
    let loaded = config::load(&path).map_err(|e| e.to_string())?;
    let root = &loaded.root;
    let launch = loaded.launch().map_err(|e| e.to_string())?;
    let (client, _handshake) = match PluginClient::connect(
        &launch,
        Duration::from_millis(loaded.config.plugin.handshake_timeout_ms),
    ) {
        Ok(pair) => pair,
        Err(lost) => {
            eprintln!("run failed: plugin: {lost}");
            return Ok(2);
        }
    };
    let raw = match client.request::<Value>(
        Method::Collect,
        Some(serde_json::json!({ "root": root.to_string_lossy() })),
        Duration::from_millis(loaded.config.plugin.collect_timeout_ms),
    ) {
        Ok(raw) => raw,
        Err(lost) => {
            eprintln!("run failed: plugin: {lost}");
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
    let resolved = match client.resolve(&keys) {
        Ok(resolved) => resolved,
        Err(lost) => {
            eprintln!("run failed: plugin: {lost}");
            return Ok(2);
        }
    };

    for claim in &collect.claims {
        let Some(reference) =
            resolved.resolved.iter().find(|r| r.reference == claim.subject.key())
        else {
            continue;
        };
        if hash::claim_id(&reference.referent_id, &claim.predicate) != claim_id {
            continue;
        }
        let Some(artefact) = &claim.channel.artefact else {
            return Err(format!("claim {claim_id} does not carry an artefact channel"));
        };
        return match sha256_of(root, &artefact.path) {
            Ok(digest) => {
                println!("{digest}");
                Ok(0)
            }
            Err(why) => Err(why),
        };
    }
    Err(format!("no claim {claim_id} in this project"))
}
