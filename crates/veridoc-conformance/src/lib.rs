//! Conformance suite for veridoc plugins.
//!
//! `run` drives one plugin over the protocol exactly as a check drives it — the same
//! `Launch`/`PluginClient` transport the engine uses — and reports, rule by rule, whether it
//! conforms. Every response is validated as an instance of the committed schema in
//! `docs/schema/veridoc-1/` before it is deserialized, so a silent protocol drift (a field added,
//! renamed, or retyped on one side) is a rule failure, not a serde surprise.
//!
//! The fixture project is rendered by the plugin's own conformance emitter, whose contract is
//! fixed in `docs/spec.md` ("Engine and plugin boundary"): `<emitter> --out <dir>` renders an
//! entire veridoc project — claims, one working scenario, a citation store, and the bivalence
//! probes — plus a `veridoc-conformance.json` naming them. A rule that does not apply to a
//! project cannot silently pass: the manifest names every subject the suite needs, and a
//! missing or unresolvable entry is a violation.
//!
//! The same [`run`] also answers the pluginrpc mechanism's suite-program contract: given the
//! directory the mechanism wrote, [`suite::run`] reads the profile spec and the manifest from
//! it, drives the plugin they name, and writes `verdicts.json` back (spec §12 item 6).

pub mod suite;

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use veridoc_engine::plugin::{sentence, Launch, PluginClient};
use veridoc_engine::plugins;
use veridoc_protocol::{
    contract::*, Method, MethodName, Timeouts, METHOD_NOT_FOUND, PROTOCOL_VERSION,
};

/// Every rule the suite can report, in the order it drives them.
pub const RULES: &[&str] = &[
    "handshake-schema",
    "collect-schema",
    "resolve-known",
    "derivable-schema",
    "bivalence-negatives",
    "scenarios-declare-values",
    "run-returns-values",
    "cite-schema",
    "surface-schema",
    "init-creates-the-project",
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

/// A rule the suite could not run, and the gate that stopped it (§12 item 3): a capability the
/// plugin did not declare, or an earlier rule whose answer this one reads. It is a record and not
/// a count, so a rule that never ran can never be read as a pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotApplicable {
    pub rule: String,
    pub reason: String,
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
    /// Rules the suite could not run, each naming the gate that stopped it. They are kept apart
    /// from the passes, and apart from the violations, which are the plugin's (§12).
    pub not_applicable: Vec<NotApplicable>,
}

impl Report {
    /// Every rule that could be run passed. A rule that could not run is neither a failure nor a
    /// pass: it is in `not_applicable`, where the reader can see it (§12).
    pub fn passed(&self) -> bool {
        self.violations.is_empty()
    }

    pub fn rules_failed(&self) -> Vec<&str> {
        let seen: BTreeSet<&str> = self.violations.iter().map(|v| v.rule.as_str()).collect();
        RULES.iter().copied().filter(|r| seen.contains(r)).collect()
    }

    pub fn rules_skipped(&self) -> Vec<&str> {
        RULES
            .iter()
            .copied()
            .filter(|r| self.not_applicable.iter().any(|n| n.rule == *r))
            .collect()
    }

    /// An aligned FAIL and SKIP table plus the summary line.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for v in &self.violations {
            out.push_str(&format!(
                "FAIL  {:<22} {}\n",
                v.rule,
                v.detail.replace('\n', " ").trim()
            ));
        }
        for n in &self.not_applicable {
            out.push_str(&format!(
                "SKIP  {:<22} {}\n",
                n.rule,
                n.reason.replace('\n', " ").trim()
            ));
        }
        let failed = self.rules_failed().len();
        let skipped = self.rules_skipped().len();
        out.push_str(&format!(
            "{} rules passed, {} violated, {} not applicable (plugin {} {}, {} claims)\n",
            RULES.len() - failed - skipped,
            failed,
            skipped,
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
/// from `collect`. Nothing here addresses a scenario: the scenario belongs to a claim, so the
/// manifest names the claim by predicate and the suite asks for its id from `collect`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    /// Predicate of the claim whose scenario the suite runs. Its id comes from `collect`.
    pub scenario_claim: String,
    /// The value names that claim's `run` must answer, declared verbatim.
    pub values: Vec<String>,
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
    let text = std::fs::read_to_string(&path).map_err(|e| {
        format!("the emitted project has no readable veridoc-conformance.json: {e}")
    })?;
    serde_json::from_str(&text).map_err(|e| format!("veridoc-conformance.json is invalid: {e}"))
}

/// The committed schemas, embedded at compile time. The suite must validate with only the
/// executable present — an installed binary has no repository tree to read — and the xtask
/// drift test is what keeps these bytes equal to `docs/schema/veridoc-1`.
const SCHEMAS: &[(&str, &str)] = &[
    (
        "handshake.json",
        include_str!("../../../docs/schema/veridoc-1/handshake.json"),
    ),
    (
        "collect-result.json",
        include_str!("../../../docs/schema/veridoc-1/collect-result.json"),
    ),
    (
        "resolve-result.json",
        include_str!("../../../docs/schema/veridoc-1/resolve-result.json"),
    ),
    (
        "derivable-result.json",
        include_str!("../../../docs/schema/veridoc-1/derivable-result.json"),
    ),
    (
        "bivalence-result.json",
        include_str!("../../../docs/schema/veridoc-1/bivalence-result.json"),
    ),
    (
        "cite-result.json",
        include_str!("../../../docs/schema/veridoc-1/cite-result.json"),
    ),
    (
        "init-result.json",
        include_str!("../../../docs/schema/veridoc-1/init-result.json"),
    ),
    (
        "surface-result.json",
        include_str!("../../../docs/schema/veridoc-1/surface-result.json"),
    ),
    (
        "run-response.json",
        include_str!("../../../docs/schema/veridoc-1/run-response.json"),
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

/// `params` built from the published request type. The suite sends exactly the shape the profile
/// spec names and the engine sends, so a params field no schema describes cannot slip in.
fn params<T: Serialize>(request: &T) -> Option<Value> {
    Some(serde_json::to_value(request).expect("the request types are serializable"))
}

/// The gate on one rule (§12 item 3): the capability whose method it calls, and the rules whose
/// answer it reads, in the order [`RULES`] drives them. `veridoc-1.profile.json` states the same
/// relation as `requiresCapability` and `dependsOn`, and `tests/profile_spec.rs` pins the two
/// tables together.
pub const GATES: &[(&str, Option<Capability>, &[&str])] = &[
    ("handshake-schema", None, &[]),
    ("collect-schema", Some(Capability::Collect), &[]),
    (
        "resolve-known",
        Some(Capability::Resolution),
        &["collect-schema"],
    ),
    (
        "derivable-schema",
        Some(Capability::Derivability),
        &["collect-schema", "resolve-known"],
    ),
    (
        "bivalence-negatives",
        Some(Capability::Bivalence),
        &["collect-schema", "resolve-known"],
    ),
    (
        "scenarios-declare-values",
        Some(Capability::Collect),
        &["collect-schema"],
    ),
    (
        "run-returns-values",
        Some(Capability::Execution),
        &["collect-schema", "resolve-known"],
    ),
    (
        "cite-schema",
        Some(Capability::Citation),
        &["collect-schema"],
    ),
    (
        "surface-schema",
        Some(Capability::Surface),
        &["collect-schema"],
    ),
    (
        "init-creates-the-project",
        Some(Capability::Initialization),
        &[],
    ),
    ("unknown-method-frame", None, &[]),
    ("framing-intact", None, &[]),
];

/// The rules this run has decided, and the gate that stopped each undecided one. Nothing records
/// a pass, so the only way a rule reaches the report as a pass is by having been run (§12 item 3).
struct Suite {
    violations: Vec<Violation>,
    not_applicable: Vec<NotApplicable>,
}

impl Suite {
    /// Open a run. `declared` is the handshake's capability list, or `None` when the plugin never
    /// answered: with no handshake there is nothing to gate on, and the run stopped for the reason
    /// the caller has already recorded.
    fn new(declared: Option<&[Capability]>) -> Suite {
        let mut suite = Suite {
            violations: Vec::new(),
            not_applicable: Vec::new(),
        };
        let Some(declared) = declared else {
            return suite;
        };
        for (rule, capability, _) in GATES {
            let Some(capability) = capability else {
                continue;
            };
            if !declared.contains(capability) {
                suite.skip(
                    rule,
                    format!(
                        "the plugin declared no `{}` capability, so its method was never called",
                        capability.name()
                    ),
                );
            }
        }
        suite
    }

    /// Whether this rule already has an outcome, which is how the caller knows to leave the
    /// plugin alone: a gated rule is never called.
    fn decided(&self, rule: &str) -> bool {
        self.violations.iter().any(|v| v.rule == rule)
            || self.not_applicable.iter().any(|n| n.rule == rule)
    }

    fn fail(&mut self, rule: &str, detail: impl Into<String>) {
        self.violations.push(Violation {
            rule: rule.to_string(),
            detail: detail.into(),
        });
    }

    /// Record that `rule` never ran, and stop every rule that reads its answer.
    fn skip(&mut self, rule: &str, reason: impl Into<String>) {
        if self.decided(rule) {
            return;
        }
        let reason = reason.into();
        self.not_applicable.push(NotApplicable {
            rule: rule.to_string(),
            reason: reason.clone(),
        });
        for (dependent, _, dependencies) in GATES {
            if dependencies.contains(&rule) {
                self.skip(dependent, format!("`{rule}` did not run: {reason}"));
            }
        }
    }

    /// Everything after `rule` in the order the suite drives them: the run ended there, so those
    /// rules never ran.
    fn skip_after(&mut self, rule: &str, reason: &str) {
        for later in RULES.iter().skip_while(|r| **r != rule).skip(1) {
            self.skip(later, reason);
        }
    }

    fn finish(self, plugin: String, protocol_version: String, claims: usize) -> Report {
        Report {
            plugin,
            protocol_version,
            claims,
            violations: self.violations,
            not_applicable: self.not_applicable,
        }
    }
}

/// Run every rule against one plugin and report. `Err` is reserved for the suite's own
/// preconditions — an unreadable fixture — because reporting them as a plugin violation
/// would blame the plugin for a broken emitter.
pub fn run(check: &Check) -> Result<Report, String> {
    let manifest = read_manifest(&check.project)?;
    let deadline = Duration::from_millis(check.deadline_ms);

    // 1. handshake-schema.
    let (client, handshake) = match PluginClient::connect(&check.launch, STARTUP_TIMEOUT) {
        Ok(pair) => pair,
        Err(lost) => {
            let mut suite = Suite::new(None);
            suite.fail(
                "handshake-schema",
                format!("cannot start the plugin: {lost}"),
            );
            suite.skip_after(
                "handshake-schema",
                "the plugin never started, so no method was ever called",
            );
            return Ok(suite.finish(String::from("(unknown)"), String::from("(unknown)"), 0));
        }
    };
    let plugin = handshake.name.clone();
    let protocol_version = handshake.protocol_version.clone();
    let mut suite = Suite::new(Some(handshake.capabilities.as_slice()));
    let hs_value = serde_json::to_value(&handshake).expect("Handshake is serializable");
    match schema("handshake.json") {
        Ok(v) => {
            if let Some(err) = schema_error(&v, &hs_value) {
                suite.fail("handshake-schema", err);
            }
        }
        Err(e) => suite.fail("handshake-schema", e),
    }
    if handshake.protocol_version != PROTOCOL_VERSION {
        suite.fail(
            "handshake-schema",
            format!(
                "handshake offered protocol \"{}\"; the suite speaks \"{PROTOCOL_VERSION}\"",
                handshake.protocol_version
            ),
        );
    }

    let claims = scenario(&mut suite, check, &manifest, &client, deadline);

    // 10./11. framing. These rules need raw lines, not the typed client: an unknown method must
    // be answered with an error frame, and a malformed line must be dropped, not fatal. Each one
    // starts its own plugin, so only a run that never started stops them.
    if !suite.decided("unknown-method-frame") {
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
                    Some(code) => suite.fail(
                    "unknown-method-frame",
                    format!(
                        "unknown method answered with error code {code}, expected {METHOD_NOT_FOUND}"
                    ),
                ),
                    None => suite.fail(
                        "unknown-method-frame",
                        format!("unknown method answered with a non-error frame: {frame}"),
                    ),
                }
            }
            Err(e) => suite.fail("unknown-method-frame", e),
        }
    }
    if !suite.decided("framing-intact") {
        match raw_dialogue(
            &check.launch,
            &[
                "this line is not json".to_string(),
                json!({"jsonrpc":"2.0","id":9002,"method":"handshake"}).to_string(),
            ],
            9002,
        ) {
            Ok(frame) if frame.get("result").is_some() => {}
            Ok(frame) => suite.fail(
                "framing-intact",
                format!("the plugin did not answer after a malformed line: {frame}"),
            ),
            Err(e) => suite.fail("framing-intact", e),
        }
    }

    client.kill();
    Ok(suite.finish(plugin, protocol_version, claims))
}

/// Rules 2 to 9: the ones that read the `collect` answer. A failure that leaves the suite with
/// nothing to read stops the section rather than reporting seven symptoms of one cause, and every
/// rule it stopped is recorded as not applicable with that failure named (§12 item 3).
fn scenario(
    suite: &mut Suite,
    check: &Check,
    manifest: &Manifest,
    client: &PluginClient,
    deadline: Duration,
) -> usize {
    // 2. collect-schema. A plugin that declared no `collect` capability was gated before the
    // first call and is never reached here.
    if suite.decided("collect-schema") {
        return 0;
    }
    let collect: CollectResult = match schema("collect-result.json").and_then(|v| {
        call_ok(
            client,
            Method::Collect,
            params(&CollectReq {
                root: check.project.display().to_string(),
            }),
            deadline,
            &v,
        )
    }) {
        Ok(result) => result,
        Err(e) => {
            suite.fail("collect-schema", e);
            suite.skip_after(
                "collect-schema",
                "collect answered nothing usable, and every rule after it reads its answer",
            );
            return 0;
        }
    };
    let claims = collect.claims.len();
    if collect.claims.is_empty() {
        suite.fail(
            "collect-schema",
            "collect reported no claims from the emitted project",
        );
    }
    if !collect.skipped.is_empty() {
        let names: Vec<&str> = collect.skipped.iter().map(|s| s.file.as_str()).collect();
        suite.fail(
            "collect-schema",
            format!(
                "the emitted project must import cleanly; skipped sidecars: {}",
                names.join(", ")
            ),
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
            client,
            Method::Resolve,
            params(&ResolveReq { refs }),
            deadline,
            &v,
        )
    }) {
        Ok(result) => result,
        Err(e) => {
            suite.fail("resolve-known", e);
            suite.skip_after(
                "resolve-known",
                "resolve answered nothing usable, and every rule after it reads its answer",
            );
            return claims;
        }
    };
    let referents: BTreeMap<String, (String, String)> = resolve
        .resolved
        .iter()
        .map(|r| {
            (
                r.reference.clone(),
                (r.referent_id.clone(), r.signature.clone()),
            )
        })
        .collect();
    let mut resolve_bad = Vec::new();
    for key in &keys {
        match referents.get(key) {
            Some((id, _)) if id.is_empty() => {
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
        resolve_bad.push(format!(
            "\"{BOGUS_REF}\" is not in unresolved with a reason"
        ));
    }
    if !resolve_bad.is_empty() {
        suite.fail("resolve-known", resolve_bad.join("; "));
    }

    // 4. derivable-schema. The first symbol subject must yield visible facts; an empty list
    // means the plugin does not implement the gate.
    if !suite.decided("derivable-schema") {
        let first_symbol = collect
            .claims
            .iter()
            .find(|c| matches!(c.subject, RefToken::Symbol { .. }))
            .map(|c| c.subject.key());
        match first_symbol.and_then(|key| referents.get(&key).map(|(id, _)| id.clone())) {
            Some(id) => {
                let outcome: Result<DerivableResult, String> = schema("derivable-result.json")
                    .and_then(|v| {
                        call_ok(
                            client,
                            Method::Derivable,
                            params(&DerivableReq { referent_id: id }),
                            deadline,
                            &v,
                        )
                    });
                match outcome {
                    Ok(r) if r.facts.is_empty() => suite.fail(
                        "derivable-schema",
                        "derivable returned no facts for a declared symbol",
                    ),
                    Ok(_) => {}
                    Err(e) => suite.fail("derivable-schema", e),
                }
            }
            None => suite.fail(
                "derivable-schema",
                "the emitted project has no symbol-subject claim with a resolved referent",
            ),
        }
    }

    // 5. bivalence-negatives. The spec negatives must come back bivalent=false and the
    // positives bivalent=true; the manifest names each probe by predicate.
    if !suite.decided("bivalence-negatives") {
        let mut biv_bad = Vec::new();
        for expect in &manifest.bivalence {
            let outcome: Result<BivalenceResult, String> = match collect
                .claims
                .iter()
                .find(|c| c.predicate == expect.predicate)
                .map(|c| c.id.clone())
            {
                None => Err(format!(
                    "collect reported no claim with predicate \"{}\"",
                    expect.predicate
                )),
                Some(claim_id) => schema("bivalence-result.json").and_then(|v| {
                    call_ok(
                        client,
                        Method::Bivalence,
                        params(&BivalenceReq { claim_id }),
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
            suite.fail("bivalence-negatives", biv_bad.join("; "));
        }
    }

    // 6. scenarios-declare-values. The assertion stays data the engine evaluates, so every value
    // an assertion reads must be declared by that claim's own scenario, with a type the check can
    // hold. These are the two checks bind makes, seen from outside the plugin.
    if !suite.decided("scenarios-declare-values") {
        let mut declared = Vec::new();
        for claim in &collect.claims {
            let Some(asserts) = claim.channel.asserts.as_ref() else {
                continue;
            };
            let Some(scenario) = collect.scenarios.get(&claim.id) else {
                declared.push(format!(
                    "claim {} declares an assertion with no scenario beside it",
                    claim.id
                ));
                continue;
            };
            for (_, leaf) in asserts.leaves() {
                let Some(value) = scenario.values.iter().find(|v| v.name == leaf.obs) else {
                    declared.push(format!(
                        "claim {} reads {}, which its scenario does not declare",
                        claim.id, leaf.obs
                    ));
                    continue;
                };
                if matches!(leaf.r#is, veridoc_protocol::Check::Contains(_))
                    && !value.kind.holds_contains()
                {
                    declared.push(format!(
                        "claim {} asserts contains over {}, which its scenario declares as {}",
                        claim.id,
                        leaf.obs,
                        value.kind.name()
                    ));
                }
            }
        }
        if !declared.is_empty() {
            suite.fail("scenarios-declare-values", declared.join("; "));
        }
    }

    // 7. run-returns-values. The engine evaluates the assertion tree, so the answer must be
    // values: ok, every value the fixture's scenario declares, one answer per leaf, and no
    // verdict field.
    if !suite.decided("run-returns-values") {
        if let Err(detail) = run_scenario(manifest, client, deadline, &collect) {
            suite.fail("run-returns-values", detail);
        }
    }

    // 8. cite-schema. The store is declared in the emitted project, so the citation must
    // resolve with text; an unresolved answer is the plugin failing, not a legitimate miss.
    if !suite.decided("cite-schema") {
        let citation = Citation {
            store: manifest.citation_store.clone(),
            id: manifest.citation_id.clone(),
            turn: manifest.citation_turn,
        };
        let cite: Result<CiteResult, String> = schema("cite-result.json").and_then(|v| {
            call_ok(
                client,
                Method::Cite,
                params(&CiteReq {
                    citations: vec![citation],
                }),
                deadline,
                &v,
            )
        });
        match cite {
            Ok(result) => {
                if result.resolved.is_empty() {
                    suite.fail(
                        "cite-schema",
                        format!(
                            "cite did not resolve {}/{} declared by the emitted project",
                            manifest.citation_store, manifest.citation_id
                        ),
                    );
                } else if result.resolved.iter().any(|r| r.text.trim().is_empty()) {
                    suite.fail("cite-schema", "a resolved citation carried empty text");
                }
            }
            Err(e) => suite.fail("cite-schema", e),
        }
    }

    // 9. surface-schema. `surface` takes no params, so the request omits them rather than
    // sending an empty object: a suite that sends a shape no host sends proves nothing.
    if !suite.decided("surface-schema") {
        match schema("surface-result.json").and_then(|v| -> Result<SurfaceResult, String> {
            call_ok(client, Method::Surface, None, deadline, &v)
        }) {
            Ok(result) if result.referents.is_empty() => suite.fail(
                "surface-schema",
                "surface reported no referents for the emitted project",
            ),
            Ok(_) => {}
            Err(e) => suite.fail("surface-schema", e),
        }
    }
    // 10. init-creates-the-project. `initialization` is the capability that lets the plugin write
    //     the files the host cannot (`docs/decisions/0013`), so the suite asks for a scratch
    //     project and checks the answer is true on disk.
    if !suite.decided("init-creates-the-project") {
        if let Err(e) = init_probe(client, deadline) {
            suite.fail("init-creates-the-project", e);
        }
    }
    claims
}

/// Ask the plugin to initialise a scratch project and check three things: it reports the files it
/// wrote, those files exist, and a second request over them is refused by naming one rather than
/// replacing it silently. The scratch directory is outside the project under test, because a plugin
/// that wrote its root document into the fixture would disturb every rule after this one.
fn init_probe(client: &PluginClient, deadline: Duration) -> Result<(), String> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    // One directory per probe: the harness runs several plugins in one process, and a probe that
    // deleted another's scratch directory would report its own files as missing.
    static PROBES: AtomicUsize = AtomicUsize::new(0);
    let scratch = std::env::temp_dir().join(format!(
        "veridoc-conformance-init-{}-{}",
        std::process::id(),
        PROBES.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&scratch)
        .map_err(|e| format!("the suite could not create {}: {e}", scratch.display()))?;
    let request = || -> Value {
        serde_json::to_value(InitReq {
            root: scratch.display().to_string(),
            document: DocumentDecl {
                path: "docs/conformance.spec.md".to_string(),
                audience: "the conformance suite".to_string(),
                question: "Which files does this plugin create?".to_string(),
                not_applicable: None,
            },
            force: false,
        })
        .expect("an init request is serializable")
    };
    let outcome = (|| -> Result<(), String> {
        let created = schema("init-result.json").and_then(|v| -> Result<InitResult, String> {
            call_ok(client, Method::Init, Some(request()), deadline, &v)
        })?;
        let first = created
            .written
            .first()
            .ok_or("init reported no file it wrote")?;
        for path in &created.written {
            if !scratch.join(path).is_file() {
                return Err(format!("init reported {path}, which is not on disk"));
            }
        }
        let again = schema("init-result.json").and_then(|v| -> Result<InitResult, String> {
            call_ok(client, Method::Init, Some(request()), deadline, &v)
        });
        match again {
            Ok(_) => Err(format!("init replaced {} without force", first)),
            Err(reason) => {
                if reason.contains(first.as_str()) {
                    Ok(())
                } else {
                    Err(format!(
                        "the refusal does not name the file it refused: {reason}"
                    ))
                }
            }
        }
    })();
    let _ = std::fs::remove_dir_all(&scratch);
    outcome
}

fn method_name(method: BivalenceMethod) -> String {
    match method {
        BivalenceMethod::Structural => "structural".to_string(),
        BivalenceMethod::StaticAnalysis => "static-analysis".to_string(),
        BivalenceMethod::Witnesses => "witnesses".to_string(),
    }
}

/// Drive the manifest's scenario and check the shape of the answer: values, not verdicts. The
/// claim's id is the only address a `run` takes, and the leaf ids a plugin answers are the ones
/// the claim's own assertion tree implies, in its pre-order walk.
fn run_scenario(
    manifest: &Manifest,
    client: &PluginClient,
    deadline: Duration,
    collect: &CollectResult,
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
    let claim_id = claim.id.clone();
    let asserts = claim
        .channel
        .asserts
        .as_ref()
        .ok_or_else(|| format!("claim {claim_id} has no assertion tree to run"))?;
    let leaves: Vec<String> = asserts
        .leaves()
        .into_iter()
        .map(|(index, _)| format!("{claim_id}#l{index}"))
        .collect();
    if leaves.is_empty() {
        return Err(format!("claim {claim_id} asserts nothing"));
    }
    let request = RunRequest {
        claim: claim_id,
        // The suite owns the wait below, so the plugin's own timeout is derived from it and is
        // strictly shorter (spec §9).
        timeout_ms: Timeouts::plugin_deadline(deadline),
    };
    let raw = sentence(
        "run",
        client.request::<Value>(Method::Run, params(&request), deadline),
    )?;
    let validator = schema("run-response.json")?;
    if let Some(err) = schema_error(&validator, &raw) {
        return Err(format!("answer is not schema-valid: {err}"));
    }
    if has_outcome_key(&raw) {
        return Err(
            "the answer carries an outcome field: run answers values, the engine judges"
                .to_string(),
        );
    }
    let answer: RunResponse = serde_json::from_value(raw)
        .map_err(|e| format!("schema-valid answer failed to decode: {e}"))?;
    if !answer.ok {
        let detail = answer
            .failure
            .map(|f| format!("{:?}: {}", f.kind, f.detail))
            .unwrap_or_else(|| "no failure given".to_string());
        return Err(format!("the fixture scenario failed: {detail}"));
    }
    let observed: BTreeSet<&str> = answer
        .observed
        .iter()
        .map(|o| o.observable.as_str())
        .collect();
    let missing: Vec<&str> = manifest
        .values
        .iter()
        .map(String::as_str)
        .filter(|name| !observed.contains(name))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "the run reported none of the values the fixture scenario declares: {}",
            missing.join(", ")
        ));
    }
    let answered: BTreeSet<&str> = answer
        .satisfied
        .iter()
        .map(|s| s.leaf_id.as_str())
        .collect();
    let unanswered: Vec<&str> = leaves
        .iter()
        .map(String::as_str)
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
    let mut writer = child.stdin.take().ok_or("the plugin has no stdin")?;
    for line in lines {
        writeln!(writer, "{line}").map_err(|e| format!("cannot write to the plugin: {e}"))?;
    }
    writer.flush().map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().ok_or("the plugin has no stdout")?;
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

/// Render the conformance fixture with the plugin's emitter and drive the suite against the
/// plugin as the registry would launch it. `veridoc install-plugin`, `veridoc validate-plugin`,
/// and the suite program [`suite::run`] drive through here, so the three cannot drift.
///
/// `root` is the plugin checkout the manifest's argv was written against, and `emitter` that
/// checkout's emitter, already resolved beside it. The fixture is staged outside both, because
/// this removes the tree it stages.
pub fn drive(
    name: &str,
    root: &Path,
    argv: &[String],
    emitter: &[PathBuf],
    deadline_ms: Option<u64>,
) -> Result<Report, String> {
    let (program, args) = emitter
        .split_first()
        .ok_or(format!("plugin \"{name}\" declares an empty emitter argv"))?;
    let fixture = plugins::root().join(format!(".conformance-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&fixture);
    // The emitter prints the directory it rendered; that line is the plugin's protocol with
    // its own driver, not veridoc's output, so it is captured and discarded.
    let rendered = Command::new(program)
        .args(args)
        .args(["--out".to_string(), fixture.display().to_string()])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cannot run the conformance emitter: {e}"))?;
    if !rendered.status.success() {
        let _ = std::fs::remove_dir_all(&fixture);
        return Err(format!(
            "the conformance emitter of plugin \"{name}\" exited {}: {}",
            rendered.status,
            String::from_utf8_lossy(&rendered.stderr).trim()
        ));
    }
    let mut launch = Launch::new(argv.to_vec());
    // The plugin runs with the rendered fixture as its working directory, as a check runs it
    // with the project it collects.
    launch.cwd = Some(fixture.display().to_string());
    let report = run(&Check {
        launch,
        project: fixture.clone(),
        deadline_ms: deadline_ms.unwrap_or(60_000),
    });
    let _ = std::fs::remove_dir_all(&fixture);
    report
}
