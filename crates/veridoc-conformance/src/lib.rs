//! Conformance suite for veridoc plugins.
//!
//! `run` drives one plugin over the protocol exactly as a check drives it — the same
//! `Launch`/`PluginClient` transport the engine uses — and reports, rule by rule, whether it
//! conforms. Every response is validated as an instance of the committed schema in
//! `docs/schema/vd-1/` before it is deserialized, so a silent protocol drift (a field added,
//! renamed, or retyped on one side) is a rule failure, not a serde surprise.
//!
//! The fixture project is rendered by the plugin's own conformance emitter, whose contract is
//! fixed in `docs/spec.md` ("Engine and plugin boundary"): `<emitter> --out <dir>` renders an
//! entire veridoc project — claims, one working scenario, a citation store, and the bivalence
//! probes — plus a `veridoc-conformance.json` naming them. A rule that does not apply to a
//! project cannot silently pass: the manifest names every subject the suite needs, and a
//! missing or unresolvable entry is a violation.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use veridoc_engine::hash;
use veridoc_engine::plugin::{sentence, Launch, PluginClient};
use veridoc_protocol::{contract::*, Method, MethodName, METHOD_NOT_FOUND, PROTOCOL_VERSION};

/// Every rule the suite can report, in the order it drives them.
pub const RULES: &[&str] = &[
    "handshake-schema",
    "collect-schema",
    "resolve-known",
    "derivable-schema",
    "bivalence-negatives",
    "run-returns-values",
    "cite-schema",
    "surface-schema",
    "unknown-method-frame",
    "framing-intact",
];

/// The handshake and the framing probes are not scenario work; they get the engine's own
/// plugin start-up bound.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

/// A reference planted in the `resolve` request that names nothing in any project. Resolving
/// it is the plugin inventing a referent.
const BOGUS_REF: &str = "src/__veridoc_absent__.ts|absentSymbol";

/// One observation of non-conformance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Violation {
    pub rule: String,
    pub detail: String,
}

/// What one suite run found. The report carries no policy: the CLI decides the exit code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub plugin: String,
    pub protocol_version: String,
    /// Claims the plugin's `collect` reported, so a suite that silently found nothing is visible.
    pub claims: usize,
    pub violations: Vec<Violation>,
    /// Rules the suite could not evaluate because an upstream rule failed. They are counted,
    /// never shown as passes.
    pub not_applicable: usize,
}

impl Report {
    pub fn passed(&self) -> bool {
        self.violations.is_empty()
    }

    pub fn rules_failed(&self) -> Vec<&str> {
        let seen: BTreeSet<&str> = self.violations.iter().map(|v| v.rule.as_str()).collect();
        RULES.iter().copied().filter(|r| seen.contains(r)).collect()
    }

    /// An aligned FAIL table plus the summary line.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for v in &self.violations {
            out.push_str(&format!(
                "FAIL  {:<22} {}\n",
                v.rule,
                v.detail.replace('\n', " ").trim()
            ));
        }
        let failed = self.rules_failed().len();
        out.push_str(&format!(
            "{} rules passed, {} violated, {} not applicable (plugin {} {}, {} claims)\n",
            RULES.len() - failed,
            failed,
            self.not_applicable,
            self.plugin,
            self.protocol_version,
            self.claims
        ));
        out
    }
}

/// What one suite run needs: how to spawn the plugin, the rendered fixture project, and the
/// per-call deadline the gate and scenario calls get.
pub struct Check {
    pub launch: Launch,
    /// The directory the emitter rendered, holding `veridoc-conformance.json`.
    pub project: PathBuf,
    pub deadline_ms: u64,
}

/// `veridoc-conformance.json`, written by the emitter: the names the suite drives. The suite
/// reads no source of the target language; every subject it touches is named here or came
/// from `collect`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    /// Predicate of the claim whose scenario channel the suite runs.
    pub scenario_claim: String,
    pub scenario_id: String,
    pub fixture: String,
    pub action: String,
    /// Observables the run must report, declared names verbatim.
    pub observables: Vec<String>,
    pub citation_store: String,
    pub citation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citation_turn: Option<u32>,
    /// The bivalence probes, by predicate, with the expected answer.
    pub bivalence: Vec<BivalenceExpect>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BivalenceExpect {
    pub predicate: String,
    pub bivalent: bool,
    /// Expected method in its wire spelling (`"static-analysis"`), when the answer must name one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
}

fn read_manifest(project: &Path) -> Result<Manifest, String> {
    let path = project.join("veridoc-conformance.json");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("the emitted project has no readable veridoc-conformance.json: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("veridoc-conformance.json is invalid: {e}"))
}

/// The committed schemas, embedded at compile time. The suite must validate with only the
/// executable present — an installed binary has no repository tree to read — and the xtask
/// drift test is what keeps these bytes equal to `docs/schema/vd-1`.
const SCHEMAS: &[(&str, &str)] = &[
    (
        "handshake.json",
        include_str!("../../../docs/schema/vd-1/handshake.json"),
    ),
    (
        "collect-result.json",
        include_str!("../../../docs/schema/vd-1/collect-result.json"),
    ),
    (
        "resolve-result.json",
        include_str!("../../../docs/schema/vd-1/resolve-result.json"),
    ),
    (
        "derivable-result.json",
        include_str!("../../../docs/schema/vd-1/derivable-result.json"),
    ),
    (
        "bivalence-result.json",
        include_str!("../../../docs/schema/vd-1/bivalence-result.json"),
    ),
    (
        "cite-result.json",
        include_str!("../../../docs/schema/vd-1/cite-result.json"),
    ),
    (
        "surface-result.json",
        include_str!("../../../docs/schema/vd-1/surface-result.json"),
    ),
    (
        "run-response.json",
        include_str!("../../../docs/schema/vd-1/run-response.json"),
    ),
];

fn schema(name: &str) -> Result<jsonschema::Validator, String> {
    let text = SCHEMAS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, text)| *text)
        .ok_or_else(|| format!("no embedded schema {name}"))?;
    let value: Value = serde_json::from_str(text)
        .map_err(|e| format!("committed schema {name} is not JSON: {e}"))?;
    jsonschema::validator_for(&value).map_err(|e| format!("schema {name} is invalid: {e}"))
}

/// The first schema error of `value`, if any, as one line.
fn schema_error(validator: &jsonschema::Validator, value: &Value) -> Option<String> {
    validator.iter_errors(value).next().map(|e| {
        let pointer = e.instance_path().to_string();
        if pointer.is_empty() {
            e.to_string()
        } else {
            format!("{pointer}: {e}")
        }
    })
}

/// Validate against the committed schema first, then deserialize. A response that passes the
/// schema but not the Rust type is a protocol drift inside the repository, and says so.
fn call_ok<T: serde::de::DeserializeOwned>(
    client: &PluginClient,
    method: Method,
    params: Option<Value>,
    timeout: Duration,
    validator: &jsonschema::Validator,
) -> Result<T, String> {
    let name = method.name();
    let raw = sentence(name, client.request::<Value>(method, params, timeout))?;
    if let Some(err) = schema_error(validator, &raw) {
        return Err(format!("answer is not schema-valid: {err}"));
    }
    serde_json::from_value(raw).map_err(|e| format!("schema-valid answer failed to decode: {e}"))
}

/// True when any object key in the tree is `outcome`: a plugin that answers verdicts instead
/// of values is the exact failure the protocol forbids.
fn has_outcome_key(value: &Value) -> bool {
    match value {
        Value::Object(map) => map
            .iter()
            .any(|(k, v)| k == "outcome" || has_outcome_key(v)),
        Value::Array(items) => items.iter().any(has_outcome_key),
        _ => false,
    }
}

/// Run every rule against one plugin and report. `Err` is reserved for the suite's own
/// preconditions — an unreadable fixture — because reporting them as a plugin violation
/// would blame the plugin for a broken emitter.
pub fn run(check: &Check) -> Result<Report, String> {
    let manifest = read_manifest(&check.project)?;
    let deadline = Duration::from_millis(check.deadline_ms);
    let mut violations: Vec<Violation> = Vec::new();
    let mut not_applicable = 0usize;
    let mut claims = 0usize;
    let mut plugin = String::from("(unknown)");
    let mut protocol_version = String::from("(unknown)");
    let mut fail = |rule: &str, detail: String, skipped: usize| {
        violations.push(Violation {
            rule: rule.to_string(),
            detail,
        });
        not_applicable += skipped;
    };

    // 1. handshake-schema.
    let (client, handshake) = match PluginClient::connect(&check.launch, STARTUP_TIMEOUT) {
        Ok(pair) => pair,
        Err(lost) => {
            fail(
                "handshake-schema",
                format!("cannot start the plugin: {lost}"),
                RULES.len() - 1,
            );
            return Ok(report(
                plugin,
                protocol_version,
                claims,
                violations,
                not_applicable,
            ));
        }
    };
    plugin = handshake.name.clone();
    protocol_version = handshake.protocol_version.clone();
    let hs_value = serde_json::to_value(&handshake).expect("Handshake is serializable");
    match schema("handshake.json") {
        Ok(v) => {
            if let Some(err) = schema_error(&v, &hs_value) {
                fail("handshake-schema", err, 0);
            }
        }
        Err(e) => fail("handshake-schema", e, 0),
    }
    if handshake.protocol_version != PROTOCOL_VERSION {
        fail(
            "handshake-schema",
            format!(
                "handshake offered protocol \"{}\"; the suite speaks \"{PROTOCOL_VERSION}\"",
                handshake.protocol_version
            ),
            0,
        );
    }

    // 2. collect-schema. Everything downstream reads the collect answer, so a failure here
    // stops the suite rather than reporting nine symptoms of one cause.
    let collect: CollectResult = match schema("collect-result.json").and_then(|v| {
        call_ok(
            &client,
            Method::Collect,
            Some(json!({ "root": check.project.display().to_string() })),
            deadline,
            &v,
        )
    }) {
        Ok(result) => result,
        Err(e) => {
            fail("collect-schema", e, RULES.len() - 2);
            return Ok(report(
                plugin,
                protocol_version,
                claims,
                violations,
                not_applicable,
            ));
        }
    };
    claims = collect.claims.len();
    if collect.claims.is_empty() {
        fail(
            "collect-schema",
            "collect reported no claims from the emitted project".to_string(),
            0,
        );
    }
    if !collect.skipped.is_empty() {
        let names: Vec<&str> = collect.skipped.iter().map(|s| s.file.as_str()).collect();
        fail(
            "collect-schema",
            format!(
                "the emitted project must import cleanly; skipped sidecars: {}",
                names.join(", ")
            ),
            0,
        );
    }

    // 3. resolve-known. Every real subject resolves with an id and a signature; the planted
    // reference must come back unresolved with a reason, never resolved.
    let keys: Vec<String> = collect
        .claims
        .iter()
        .map(|c| c.subject.key())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut refs = keys.clone();
    refs.push(BOGUS_REF.to_string());
    let resolve: ResolveResult = match schema("resolve-result.json").and_then(|v| {
        call_ok(
            &client,
            Method::Resolve,
            Some(json!({ "refs": refs })),
            deadline,
            &v,
        )
    }) {
        Ok(result) => result,
        Err(e) => {
            fail("resolve-known", e, RULES.len() - 3);
            return Ok(report(
                plugin,
                protocol_version,
                claims,
                violations,
                not_applicable,
            ));
        }
    };
    let referents: BTreeMap<String, (String, String)> = resolve
        .resolved
        .iter()
        .map(|r| (r.reference.clone(), (r.referent_id.clone(), r.signature.clone())))
        .collect();
    let mut resolve_bad = Vec::new();
    for key in &keys {
        match referents.get(key) {
            Some((id, signature)) if id.is_empty() => {
                resolve_bad.push(format!("resolved {key} carries an empty referentId"))
            }
            Some((_, signature)) if signature.is_empty() => {
                resolve_bad.push(format!("resolved {key} carries an empty signature"))
            }
            Some(_) => {}
            None => {
                let explained = resolve
                    .unresolved
                    .iter()
                    .any(|u| u.reference == *key && !u.reason.trim().is_empty());
                if explained {
                    resolve_bad.push(format!(
                        "{key} is a declared subject reported unresolved: {}",
                        resolve
                            .unresolved
                            .iter()
                            .find(|u| u.reference == *key)
                            .map(|u| u.reason.as_str())
                            .unwrap_or("")
                    ));
                } else {
                    resolve_bad.push(format!("{key} is neither resolved nor unresolved"));
                }
            }
        }
    }
    if let Some((id, _)) = referents.get(BOGUS_REF) {
        resolve_bad.push(format!(
            "the plugin resolved \"{BOGUS_REF}\" to {id}; the reference names nothing that exists"
        ));
    } else if !resolve
        .unresolved
        .iter()
        .any(|u| u.reference == BOGUS_REF && !u.reason.trim().is_empty())
    {
        resolve_bad.push(format!("\"{BOGUS_REF}\" is not in unresolved with a reason"));
    }
    if !resolve_bad.is_empty() {
        fail("resolve-known", resolve_bad.join("; "), 0);
    }

    // 4. derivable-schema. The first symbol subject must yield visible facts; an empty list
    // means the plugin does not implement the gate.
    let first_symbol = collect
        .claims
        .iter()
        .find(|c| matches!(c.subject, RefToken::Symbol { .. }))
        .map(|c| c.subject.key());
    match first_symbol.and_then(|key| referents.get(&key).map(|(id, _)| id.clone())) {
        Some(id) => {
            let outcome: Result<DerivableResult, String> = schema("derivable-result.json").and_then(|v| {
                call_ok(
                    &client,
                    Method::Derivable,
                    Some(json!({ "referentId": id })),
                    deadline,
                    &v,
                )
            });
            match outcome {
                Ok(r) if r.facts.is_empty() => fail(
                    "derivable-schema",
                    "derivable returned no facts for a declared symbol".to_string(),
                    0,
                ),
                Ok(_) => {}
                Err(e) => fail("derivable-schema", e, 0),
            }
        }
        None => fail(
            "derivable-schema",
            "the emitted project has no symbol-subject claim with a resolved referent".to_string(),
            0,
        ),
    }

    // 5. bivalence-negatives. The spec negatives must come back bivalent=false and the
    // positives bivalent=true; the manifest names each probe by predicate.
    let mut biv_bad = Vec::new();
    for expect in &manifest.bivalence {
        let outcome: Result<BivalenceResult, String> = match collect
            .claims
            .iter()
            .find(|c| c.predicate == expect.predicate)
            .map(|c| claim_id_for(&referents, c))
        {
            None => Err(format!(
                "collect reported no claim with predicate \"{}\"",
                expect.predicate
            )),
            Some(Err(e)) => Err(e),
            Some(Ok(claim_id)) => schema("bivalence-result.json").and_then(|v| {
                call_ok(
                    &client,
                    Method::Bivalence,
                    Some(json!({ "claimId": claim_id })),
                    deadline,
                    &v,
                )
            }),
        };
        match outcome {
            Ok(answer) => {
                if answer.bivalent != expect.bivalent {
                    biv_bad.push(format!(
                        "\"{}\": answered bivalent={}, expected {}",
                        expect.predicate, answer.bivalent, expect.bivalent
                    ));
                }
                if let Some(want) = &expect.method {
                    let got = method_name(answer.method);
                    if &got != want {
                        biv_bad.push(format!(
                            "\"{}\": answered method \"{got}\", expected \"{want}\"",
                            expect.predicate
                        ));
                    }
                }
            }
            Err(e) => biv_bad.push(format!("\"{}\": {e}", expect.predicate)),
        }
    }
    if !biv_bad.is_empty() {
        fail("bivalence-negatives", biv_bad.join("; "), 0);
    }

    // 6. run-returns-values. The engine evaluates the assertion tree, so the answer must be
    // values: ok, every declared observable, one answer per leaf, and no verdict field.
    if let Err(detail) = run_scenario(&manifest, &client, deadline, &collect, &referents) {
        fail("run-returns-values", detail, 0);
    }

    // 7. cite-schema. The store is declared in the emitted project, so the citation must
    // resolve with text; an unresolved answer is the plugin failing, not a legitimate miss.
    let cite_params = json!({ "citations": [ { "store": manifest.citation_store,
        "id": manifest.citation_id, "turn": manifest.citation_turn } ] });
    let cite: Result<CiteResult, String> = schema("cite-result.json").and_then(|v| {
        call_ok(&client, Method::Cite, Some(cite_params), deadline, &v)
    });
    match cite {
        Ok(result) => {
            if result.resolved.is_empty() {
                fail(
                    "cite-schema",
                    format!(
                        "cite did not resolve {}/{} declared by the emitted project",
                        manifest.citation_store, manifest.citation_id
                    ),
                    0,
                );
            } else if result.resolved.iter().any(|r| r.text.trim().is_empty()) {
                fail(
                    "cite-schema",
                    "a resolved citation carried empty text".to_string(),
                    0,
                );
            }
        }
        Err(e) => fail("cite-schema", e, 0),
    }

    // 8. surface-schema.
    match schema("surface-result.json").and_then(|v| -> Result<SurfaceResult, String> {
        call_ok(&client, Method::Surface, Some(json!({})), deadline, &v)
    }) {
        Ok(result) if result.referents.is_empty() => fail(
            "surface-schema",
            "surface reported no referents for the emitted project".to_string(),
            0,
        ),
        Ok(_) => {}
        Err(e) => fail("surface-schema", e, 0),
    }

    // 9./10. framing. These rules need raw lines, not the typed client: an unknown method must
    // be answered with an error frame, and a malformed line must be dropped, not fatal.
    match raw_dialogue(
        &check.launch,
        &[json!({"jsonrpc":"2.0","id":9001,"method":"nope"}).to_string()],
        9001,
    ) {
        Ok(frame) => {
            match frame
                .get("error")
                .and_then(|e| e.get("code"))
                .and_then(Value::as_i64)
            {
                Some(code) if code == METHOD_NOT_FOUND => {}
                Some(code) => fail(
                    "unknown-method-frame",
                    format!(
                        "unknown method answered with error code {code}, expected {METHOD_NOT_FOUND}"
                    ),
                    0,
                ),
                None => fail(
                    "unknown-method-frame",
                    format!("unknown method answered with a non-error frame: {frame}"),
                    0,
                ),
            }
        }
        Err(e) => fail("unknown-method-frame", e, 0),
    }
    match raw_dialogue(
        &check.launch,
        &[
            "this line is not json".to_string(),
            json!({"jsonrpc":"2.0","id":9002,"method":"handshake"}).to_string(),
        ],
        9002,
    ) {
        Ok(frame) if frame.get("result").is_some() => {}
        Ok(frame) => fail(
            "framing-intact",
            format!("the plugin did not answer after a malformed line: {frame}"),
            0,
        ),
        Err(e) => fail("framing-intact", e, 0),
    }

    client.kill();
    Ok(report(
        plugin,
        protocol_version,
        claims,
        violations,
        not_applicable,
    ))
}

fn report(
    plugin: String,
    protocol_version: String,
    claims: usize,
    violations: Vec<Violation>,
    not_applicable: usize,
) -> Report {
    Report {
        plugin,
        protocol_version,
        claims,
        violations,
        not_applicable,
    }
}

fn method_name(method: BivalenceMethod) -> String {
    match method {
        BivalenceMethod::Structural => "structural".to_string(),
        BivalenceMethod::StaticAnalysis => "static-analysis".to_string(),
        BivalenceMethod::Witnesses => "witnesses".to_string(),
    }
}

/// The engine's claim id for a collected claim: the same `hash::claim_id` the bind pass uses,
/// so the plugin sees the id a real run would send.
fn claim_id_for(referents: &BTreeMap<String, (String, String)>, claim: &RawClaim) -> Result<String, String> {
    let key = claim.subject.key();
    referents
        .get(&key)
        .map(|(id, _)| hash::claim_id(id, &claim.predicate))
        .ok_or_else(|| format!("{key} has no resolved referent to build the claim id from"))
}

/// Drive the manifest's scenario and check the shape of the answer: values, not verdicts.
fn run_scenario(
    manifest: &Manifest,
    client: &PluginClient,
    deadline: Duration,
    collect: &CollectResult,
    referents: &BTreeMap<String, (String, String)>,
) -> Result<(), String> {
    let claim = collect
        .claims
        .iter()
        .find(|c| c.predicate == manifest.scenario_claim)
        .ok_or_else(|| {
            format!(
                "collect reported no claim with predicate \"{}\"",
                manifest.scenario_claim
            )
        })?;
    let claim_id = claim_id_for(referents, claim)?;
    let asserts = claim
        .channel
        .asserts
        .as_ref()
        .ok_or_else(|| format!("claim {claim_id} has no assertion tree to run"))?;
    let leaves: Vec<RunLeaf> = asserts
        .leaves()
        .into_iter()
        .map(|(index, _)| RunLeaf {
            leaf_id: format!("{claim_id}#l{index}"),
        })
        .collect();
    if leaves.is_empty() {
        return Err(format!("claim {claim_id} asserts nothing"));
    }
    let request = RunRequest {
        scenario_id: manifest.scenario_id.clone(),
        fixture: manifest.fixture.clone(),
        action: manifest.action.clone(),
        args: Vec::new(),
        deadline_ms: deadline.as_millis() as u64,
        leaves,
    };
    let params = serde_json::to_value(&request).expect("RunRequest is serializable");
    let raw = sentence("run", client.request::<Value>(Method::Run, Some(params), deadline))?;
    let validator = schema("run-response.json")?;
    if let Some(err) = schema_error(&validator, &raw) {
        return Err(format!("answer is not schema-valid: {err}"));
    }
    if has_outcome_key(&raw) {
        return Err("the answer carries an outcome field: run answers values, the engine judges".to_string());
    }
    let answer: RunResponse =
        serde_json::from_value(raw).map_err(|e| format!("schema-valid answer failed to decode: {e}"))?;
    if !answer.ok {
        let detail = answer
            .failure
            .map(|f| format!("{:?}: {}", f.kind, f.detail))
            .unwrap_or_else(|| "no failure given".to_string());
        return Err(format!("the fixture scenario failed: {detail}"));
    }
    let observed: BTreeSet<&str> = answer.observed.iter().map(|o| o.observable.as_str()).collect();
    let missing: Vec<&str> = manifest
        .observables
        .iter()
        .map(String::as_str)
        .filter(|name| !observed.contains(name))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "the run reported none of the declared observables: {}",
            missing.join(", ")
        ));
    }
    let answered: BTreeSet<&str> = answer.satisfied.iter().map(|s| s.leaf_id.as_str()).collect();
    let unanswered: Vec<&str> = request
        .leaves
        .iter()
        .map(|l| l.leaf_id.as_str())
        .filter(|id| !answered.contains(id))
        .collect();
    if !unanswered.is_empty() {
        return Err(format!(
            "the run answered no value for the leaves: {}",
            unanswered.join(", ")
        ));
    }
    Ok(())
}

/// One raw line-level exchange with a fresh plugin instance: write every line, then read
/// frames until the one with `want` arrives. The typed client drops unparseable lines by
/// contract, so the framing rules cannot be driven through it.
fn raw_dialogue(launch: &Launch, lines: &[String], want: u64) -> Result<Value, String> {
    let (argv0, argv_rest) = launch
        .argv
        .split_first()
        .ok_or("the plugin launch has no argv")?;
    let mut command = Command::new(argv0);
    command.args(argv_rest).envs(&launch.env);
    // An empty `cwd` would make spawn fail; only a set directory is passed on.
    if let Some(cwd) = &launch.cwd {
        command.current_dir(cwd);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot spawn the plugin: {e}"))?;
    let mut writer = child
        .stdin
        .take()
        .ok_or("the plugin has no stdin")?;
    for line in lines {
        writeln!(writer, "{line}").map_err(|e| format!("cannot write to the plugin: {e}"))?;
    }
    writer.flush().map_err(|e| e.to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or("the plugin has no stdout")?;
    let (sender, receiver) = mpsc::channel::<String>();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            let trimmed = line.trim_end_matches(['\n', '\r']).to_string();
            line.clear();
            if sender.send(trimmed).is_err() {
                break;
            }
        }
    });
    // Read frames until the probe's own id arrives or the pipe ends; lines the plugin emits
    // for other requests are skipped, not errors.
    let answered = loop {
        match receiver.recv_timeout(STARTUP_TIMEOUT) {
            Ok(line) => {
                if let Ok(frame) = serde_json::from_str::<Value>(&line) {
                    if frame.get("id").and_then(Value::as_u64) == Some(want) {
                        break Some(frame);
                    }
                }
            }
            Err(_) => break None,
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    answered.ok_or(format!(
        "the plugin never answered the probe request {want} within {} seconds",
        STARTUP_TIMEOUT.as_secs()
    ))
}
