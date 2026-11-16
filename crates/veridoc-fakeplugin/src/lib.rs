//! A scripted veridoc plugin. It answers the whole protocol from a built-in conforming
//! script, and any single method can be overridden by the `VERIDOC_FAKE_SCRIPT` environment
//! variable holding a JSON object keyed by method name, which is how the engine's negative
//! controls are driven.
//!
//! Script keys beyond the method names:
//! - `"_sleepMs": {"run": 5000}` — sleep before answering that method.
//! - `"_raw": {"handshake": {...}}` — write the given response verbatim, id included, so a
//!   test can answer a request with the wrong id.
//! - `"_malformed": ["collect"]` — write a non-JSON line instead of the response.
//! - `"_stderr": ["line"]` — write these lines to stderr before answering anything.
//!
//! The same binary is its own conformance fixture: `--emit-conformance <dir>` renders the
//! emitter contract (`veridoc-conformance.json`), and `collect` adds the manifest's five
//! bivalence probes when it collects a root that holds one. The engine's happy-path tests
//! collect roots without a manifest, so they keep seeing the single-claim script.

use std::io::{BufRead, Write};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use veridoc_protocol::*;

/// `c` + the first 16 hex characters of `sha256(referent_id + "\u{0}" + predicate)` — a copy
/// of `veridoc_engine::hash::claim_id`. A plugin must recognise the claim ids the engine
/// sends, and this mirrors what a real plugin re-implements in its own language; the
/// `claim_id_matches_the_engine` test is the drift gate.
pub fn claim_id(referent_id: &str, predicate: &str) -> String {
    let mut bytes = Vec::with_capacity(referent_id.len() + predicate.len() + 2);
    bytes.extend_from_slice(referent_id.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(predicate.as_bytes());
    let digest = Sha256::digest(&bytes);
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("c{}", &hex[..16])
}

/// The five bivalence probes: the four spec negatives and one positive, answered by table.
/// `(predicate, bivalent, method)`.
pub const PROBES: &[(&str, bool, &str)] = &[
    ("always-true", false, "structural"),
    ("or-true", false, "structural"),
    ("self-equal", false, "structural"),
    ("length-nonneg", false, "structural"),
    ("length-positive", true, "static-analysis"),
];

/// The conformance manifest the emitter writes and `collect` reads. `veridoc-conformance.json`
/// is the emitter's contract with `veridoc-conformance`; the fields mirror its `Manifest`.
pub fn conformance_manifest() -> Value {
    json!({
        "scenarioClaim": "rejects-missing-project-root",
        "scenarioId": "config/rejects-missing-root",
        "fixture": "empty-directory",
        "action": "run-probe",
        "observables": ["exit.code"],
        "citationStore": "session",
        "citationId": "conformance",
        "citationTurn": 1,
        "bivalence": PROBES
            .iter()
            .map(|(predicate, bivalent, method)| {
                json!({ "predicate": predicate, "bivalent": bivalent, "method": method })
            })
            .collect::<Vec<_>>()
    })
}

/// `<emitter> --out <dir>`: render the fixture project. The fake plugin's project is virtual —
/// its claims are built in — so the rendered project is the manifest that names them.
pub fn emit_conformance(dir: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let path = dir.join("veridoc-conformance.json");
    let text = serde_json::to_string_pretty(&conformance_manifest())
        .map_err(|e| format!("cannot render the manifest: {e}"))?;
    std::fs::write(&path, format!("{text}\n"))
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// The probe claims, reported only when the collected root holds a conformance manifest.
/// Each is a channel-less `ensures` claim on the same resolved symbol; the conformance suite
/// drives them through `bivalence`, never through a scenario.
fn probe_claims() -> Vec<Value> {
    PROBES
        .iter()
        .map(|(predicate, _, _)| {
            json!({
                "kind": "ensures",
                "subject": { "kind": "symbol", "name": "probe", "from": "src/cli.ts" },
                "concept": "project-scan",
                "predicate": predicate,
                "text": format!("The bivalence probe \"{predicate}\" settles as declared."),
                "channel": {},
                "supports": [],
                "file": "src/cli.doc.ts",
                "line": 4
            })
        })
        .collect()
}

/// The default `collect` answer: one concept, one document node, one `ensures` claim with a
/// bivalent assertion, one scenario, two observables, one action, and no skipped sidecars.
/// The engine's happy path runs against this without Node. A root that holds a
/// `veridoc-conformance.json` also gets the five bivalence probes.
pub fn default_collect(req: &Value) -> Value {
    let mut claims = vec![json!({
        "kind": "ensures",
        "subject": { "kind": "symbol", "name": "probe", "from": "src/cli.ts" },
        "concept": "project-scan",
        "predicate": "rejects-missing-project-root",
        "text": "A missing project root is refused with a diagnostic that names the searched path.",
        "channel": {
            "scenario": "config/rejects-missing-root",
            "asserts": { "obs": "exit.code", "is": { "equals": 2 } }
        },
        "supports": ["docs/architecture.spec.md#refuses"],
        "file": "src/cli.doc.ts",
        "line": 3
    })];
    let root = req["root"].as_str().unwrap_or_default();
    if !root.is_empty() && std::path::Path::new(root).join("veridoc-conformance.json").is_file() {
        claims.extend(probe_claims());
    }
    json!({
        "concepts": ["project-scan"],
        "documents": [{
            "path": "docs/architecture.spec.md",
            "audience": "an author writing a new plugin",
            "question": "What must my plugin answer, and in what order?",
            "selects": { "concept": "project-scan" }
        }],
        "claims": claims,
        "observables": [
            { "name": "exit.code", "type": "number" },
            { "name": "stderr.text", "type": "string" }
        ],
        "actions": [{
            "name": "run-probe",
            "calls": { "kind": "symbol", "name": "probe", "from": "src/cli.ts" },
            "produces": ["exit.code", "stderr.text"]
        }],
        "fixtures": ["empty-directory"],
        "skipped": []
    })
}

pub fn default_handshake() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "name": "veridoc-fakeplugin",
        "version": "0.1.0",
        "languages": ["typescript"],
        "capabilities": [
            "collect", "resolution", "derivability", "bivalence", "execution", "citation", "surface"
        ]
    })
}

pub fn default_resolve(req: &Value) -> Value {
    let refs = req["refs"].as_array().cloned().unwrap_or_default();
    let mut resolved = Vec::new();
    let mut unresolved = Vec::new();
    for reference in refs {
        let reference = reference.as_str().unwrap_or_default();
        match reference {
            "src/cli.ts|probe" => resolved.push(json!({
                "ref": reference,
                "referentId": "vd:r:probe",
                "signature": "function probe(dir: string): number"
            })),
            "concept|project-scan" => resolved.push(json!({
                "ref": reference,
                "referentId": "vd:r:project-scan",
                "signature": "concept project-scan"
            })),
            other => unresolved.push(json!({
                "ref": other,
                "reason": "no exported member"
            })),
        }
    }
    json!({ "resolved": resolved, "unresolved": unresolved })
}

/// Facts a reader can already see in the declaration. None of them equals the default claim's
/// text, so the default claim survives the derivability ban.
pub fn default_derivable() -> Value {
    json!({
        "facts": [
            "probe is exported from src/cli.ts.",
            "probe takes one parameter named dir of type string and returns number."
        ]
    })
}

/// The probe claims answer from the table by their claim id; every other claim keeps the
/// scripted structural answer, which is what the engine's happy path expects.
pub fn default_bivalence(req: &Value) -> Value {
    let claim = req["claimId"].as_str().unwrap_or_default();
    for (predicate, bivalent, method) in PROBES {
        if claim_id("vd:r:probe", predicate) == claim {
            return json!({
                "bivalent": bivalent,
                "method": method,
                "reason": format!("the scripted verdict for the probe \"{predicate}\"")
            });
        }
    }
    json!({ "bivalent": true, "method": "structural" })
}

/// The scenario answers with `exit.code = 2` and marks every leaf the engine issued as
/// satisfied, so the default claim's tree holds.
pub fn default_run(req: &Value) -> Value {
    let leaves = req["leaves"].as_array().cloned().unwrap_or_default();
    let satisfied: Vec<Value> = leaves
        .iter()
        .map(|leaf| json!({ "leafId": leaf["leafId"], "ok": true }))
        .collect();
    json!({
        "ok": true,
        "observed": [
            { "observable": "exit.code", "value": "2" },
            { "observable": "stderr.text", "value": "no .veridoc/config.json above /tmp/x" }
        ],
        "satisfied": satisfied
    })
}

/// The conformance fixture's citation resolves; anything else is a normal unresolved answer,
/// which is how the engine's citation-channel tests drive the miss.
pub fn default_cite(req: &Value) -> Value {
    let citations = req["citations"].as_array().cloned().unwrap_or_default();
    let mut resolved = Vec::new();
    let mut unresolved = Vec::new();
    for citation in citations {
        let store = citation["store"].as_str().unwrap_or_default();
        let id = citation["id"].as_str().unwrap_or_default();
        if store == "session" && id == "conformance" {
            resolved.push(json!({
                "id": id,
                "text": "The conformance fixture records this exchange.",
                "state": "decided"
            }));
        } else {
            unresolved.push(json!({ "id": id, "reason": format!("no citation {id} in store {store}") }));
        }
    }
    json!({ "resolved": resolved, "unresolved": unresolved })
}

pub fn default_surface() -> Value {
    json!({
        "referents": [
            { "referentId": "vd:r:probe", "name": "probe", "public": true },
            { "referentId": "vd:r:helper", "name": "helper", "public": true }
        ]
    })
}

fn default_result(method: Method, req: &Value) -> Value {
    match method {
        Method::Handshake => default_handshake(),
        Method::Collect => default_collect(req),
        Method::Resolve => default_resolve(req),
        Method::Derivable => default_derivable(),
        Method::Bivalence => default_bivalence(req),
        Method::Run => default_run(req),
        Method::Cite => default_cite(req),
        Method::Surface => default_surface(),
    }
}

/// Answer requests until the pipe closes. stdout carries protocol only; diagnostics go to
/// stderr.
pub fn serve(reader: impl BufRead, writer: &mut impl Write) {
    let script: Value = std::env::var("VERIDOC_FAKE_SCRIPT")
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null);
    for line in script["_stderr"].as_array().into_iter().flatten() {
        if let Some(line) = line.as_str() {
            eprintln!("{line}");
        }
    }
    let mut reader = reader;
    let mut buf = String::new();
    while let Ok(Some(line)) = read_line(&mut reader, &mut buf) {
        let Ok(req) = parse_request(&line) else {
            // A malformed request is answered with a parse error, as the TypeScript plugin is.
            let _ = write_line(writer, &Response::err(0, PARSE_ERROR, "malformed json"));
            continue;
        };
        let Some(method) = Method::from_wire(&req.method) else {
            let _ = write_line(
                writer,
                &Response::err(
                    req.id,
                    METHOD_NOT_FOUND,
                    format!("unknown method {}", req.method),
                ),
            );
            continue;
        };
        if script["_malformed"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|m| m.as_str() == Some(method.name()))
        {
            let _ = writer.write_all(b"this line is not json\n");
            let _ = writer.flush();
            continue;
        }
        if let Some(raw) = script["_raw"].get(method.name()) {
            let _ = write_line(writer, raw);
            continue;
        }
        if let Some(ms) = script["_sleepMs"][method.name()].as_u64() {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
        let params = req.params.clone().unwrap_or(Value::Null);
        let result = match script.get(method.name()) {
            Some(override_result) if !override_result.is_null() => override_result.clone(),
            _ => default_result(method, &params),
        };
        let _ = write_line(writer, &Response::ok(req.id, result));
    }
}
