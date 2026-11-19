//! The whole check pipeline in one function: spawn, handshake, collect, bind, gates,
//! execution, channels, briefs, judge with the verdict cache, metrics, the coverage gate,
//! the result document, and the formatters. Exit codes are fixed here because they are
//! policy, not presentation: 0 documentation built, 1 blocking findings, 2 run failure.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::artefact;
use crate::bind::{self, BindFailure, Bound, BoundChannel, BoundClaim, CollectWire};
use crate::bivalence;
use crate::brief::{Brief, BriefClaim, BriefNode, BriefSpan, Task};
use crate::cite;
use crate::config::{Config, LoadedConfig};
use crate::derivability;
use crate::diagnostic::Diagnostic;
use crate::execute::{self, Outcome, READING_CODE_WRONG, READING_SCENARIO_WRONG};
use crate::format;
use crate::hash;
use crate::inputs;
use crate::judge::{Judge, JudgeError, ModelJudge, Provider};
use crate::plugin::PluginClient;
use crate::result::{self, ClaimResult, Metrics, Outcome as DocOutcome, ResultDocument};
use crate::verdicts;
use serde::de::DeserializeOwned;
use serde_json::Value;
use veridoc_protocol::{Call, Capability, Kind, Method};

/// What the CLI prints and exits with.
pub struct Report {
    pub exit: i32,
    pub diagnostics: Vec<Diagnostic>,
    pub result_written: bool,
}

impl Report {
    fn run_failure(detail: String) -> Self {
        Self {
            exit: 2,
            diagnostics: vec![Diagnostic {
                file: None,
                line: None,
                message: detail,
                severity: crate::diagnostic::Severity::Error,
            }],
            result_written: false,
        }
    }
    fn findings(diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            exit: 1,
            diagnostics,
            result_written: false,
        }
    }
}

/// Run `veridoc check`. `stop` is the Ctrl-C flag: the first press turns the run into a run
/// failure between stages; the second exits 130 in the signal handler.
pub fn check(loaded: &LoadedConfig, force: bool, stop: &AtomicBool) -> Report {
    let root = loaded.root.as_path();
    let config = &loaded.config;
    REJUDGED.with(|set| set.borrow_mut().clear());
    let interrupted = |stage: &str| -> Option<Report> {
        if stop.load(Ordering::Relaxed) {
            Some(Report::run_failure(format!(
                "run failed: plugin: interrupted during {stage}"
            )))
        } else {
            None
        }
    };

    // 1. Spawn and handshake. Version mismatch and spawn faults are run failures.
    // The launch resolves `plugin.command` against the project root and injects
    // `VERIDOC_ROOT` and `VERIDOC_CONFIG`.
    let launch = match loaded.launch() {
        Ok(launch) => launch,
        Err(e) => return Report::run_failure(format!("run failed: plugin: {e}")),
    };
    let (client, handshake) = match PluginClient::connect(
        &launch,
        Duration::from_millis(config.plugin.handshake_timeout_ms),
    ) {
        Ok(pair) => pair,
        Err(err) => return Report::run_failure(format!("run failed: plugin: {err}")),
    };
    let caps = handshake.capabilities.clone();
    for required in [Capability::Collect, Capability::Resolution] {
        if !caps.contains(&required) {
            return Report::run_failure(format!(
                "run failed: plugin: plugin \"{}\" declares no {}",
                handshake.name,
                required.name()
            ));
        }
    }
    if let Some(report) = interrupted("collect") {
        return report;
    }

    // 2. Collect, as raw JSON so bind can validate each assertion tree itself.
    let raw = match client.request::<Value>(
        Method::Collect,
        Some(serde_json::json!({ "root": root.to_string_lossy() })),
        Duration::from_millis(config.plugin.collect_timeout_ms),
    ) {
        Call::Result(raw) => raw,
        Call::Refusal(error) => {
            return Report::run_failure(format!(
                "run failed: plugin: the plugin refused collect ({}): {}",
                error.code, error.message
            ))
        }
        // A loss is a connection problem, so it carries the plugin's own uncorrelated refusal
        // beside it when the connection holds one (spec §4).
        Call::Lost(lost) => {
            return Report::run_failure(client.with_fault(&format!("run failed: plugin: {lost}")))
        }
    };
    let collect: CollectWire = match serde_json::from_value(raw) {
        Ok(collect) => collect,
        Err(e) => {
            return Report::run_failure(format!("run failed: plugin: replied malformed json: {e}"))
        }
    };

    // 3. Resolve once, bind. Bind errors are findings; a skipped sidecar is a run failure.
    let resolve = |keys: &[String]| client.resolve(keys);
    let bound = match bind::bind(
        root,
        &handshake.name,
        &caps,
        &config.citation_stores,
        collect,
        resolve,
    ) {
        Ok(bound) => bound,
        Err(BindFailure::RunFailure(why)) => {
            return Report::run_failure(client.with_fault(&format!("run failed: plugin: {why}")));
        }
        Err(BindFailure::Errors(errors)) => {
            return Report::findings(errors.iter().map(bind_error_diagnostic).collect::<Vec<_>>());
        }
    };
    if let Some(report) = interrupted("gates") {
        return report;
    }

    // 4. The derivability ban. Rejections are warnings, not findings.
    let mut warnings: Vec<crate::result::Warning> = bound.warnings.clone();
    let derived =
        match derivability::gate(bound.claims.clone(), &handshake.name, &caps, |referent| {
            client.derivable(referent)
        }) {
            Ok(outcome) => outcome,
            Err(detail) => {
                return Report::run_failure(
                    client.with_fault(&format!("run failed: plugin: {detail}")),
                )
            }
        };
    for rejected in &derived.rejected {
        warnings.push(crate::result::Warning {
            file: Some(rejected.file.clone()),
            line: Some(rejected.line),
            message: format!(
                "claim {} restates a fact the reader can already see: {}",
                rejected.claim_id, rejected.fact
            ),
        });
    }
    let claims = derived.kept;

    // 5. The bivalence gate routes non-bivalent claims to the judge channel.
    let classified = match bivalence::gate(claims, &handshake.name, &caps, |claim| {
        client.bivalence(claim)
    }) {
        Ok(outcome) => outcome,
        Err(detail) => {
            return Report::run_failure(client.with_fault(&format!("run failed: plugin: {detail}")))
        }
    };
    let mut errors: Vec<Diagnostic> = classified
        .errors
        .iter()
        .map(bind_error_diagnostic)
        .collect();
    let claims = classified.claims;
    if let Some(report) = interrupted("execution") {
        return report;
    }

    // 6. Execution: each scenario-channel claim runs its own scenario, and the engine evaluates
    //    the tree. A scenario that cannot run is recorded as that claim's `run-failed` result and
    //    every other claim still gets its verdict; only a contradiction is a finding.
    let executed = execute::execute(&claims, config.run.scenario_deadline_ms, |req, host| {
        client.run(req, host)
    });
    let mut outcomes: Vec<OutcomeOf> = Vec::new();
    for done in executed {
        match done {
            execute::Executed::Evaluated {
                claim_id,
                outcome,
                observed,
            } => {
                let claim = claims
                    .iter()
                    .find(|c| c.id == claim_id)
                    .expect("executed claim");
                match outcome {
                    Outcome::Supported => outcomes.push(OutcomeOf {
                        claim,
                        outcome: DocOutcome::Supported,
                        residue: false,
                        channel: "scenario",
                        evidence: scenario_when_of(claim),
                    }),
                    Outcome::Contradicted => errors.push(Diagnostic::error(
                        &claim.file,
                        claim.line,
                        format!(
                            "claim {} is contradicted: {}; either {READING_CODE_WRONG}, or {READING_SCENARIO_WRONG}",
                            claim.id,
                            observed.join(", ")
                        ),
                    )),
                }
            }
            // A run that did not happen is never a claim called supported or contradicted. It is
            // the claim's result, said out loud, and the build keeps going.
            execute::Executed::RunFailed {
                claim_id,
                kind,
                detail,
            } => {
                let claim = claims
                    .iter()
                    .find(|c| c.id == claim_id)
                    .expect("executed claim");
                let said = format!("{}: {detail}", execute::kind_word(kind));
                warnings.push(crate::result::Warning {
                    file: Some(claim.file.clone()),
                    line: Some(claim.line),
                    message: format!("claim {} did not run: {said}", claim.id),
                });
                outcomes.push(OutcomeOf {
                    claim,
                    outcome: DocOutcome::RunFailed,
                    residue: false,
                    channel: "scenario",
                    evidence: said,
                });
            }
        }
    }
    if let Some(report) = interrupted("channels") {
        return report;
    }

    // 7. The artefact channel.
    for verified in artefact::verify(&claims, root) {
        let claim = claims
            .iter()
            .find(|c| c.id == verified.claim_id)
            .expect("verified claim");
        match verified.outcome {
            Outcome::Supported => outcomes.push(OutcomeOf {
                claim,
                outcome: DocOutcome::Supported,
                residue: false,
                channel: "artefact",
                evidence: artefact_path_of(claim),
            }),
            Outcome::Contradicted => {
                errors.push(Diagnostic::error(&claim.file, claim.line, verified.detail))
            }
        }
    }

    // 8. The citation channel, judged by S2 over a synthesized single-claim brief.
    let judge_client = match make_judge(loaded) {
        Ok(judge_client) => judge_client,
        Err(detail) => return Report::run_failure(format!("run failed: judge: {detail}")),
    };
    if judge_client.is_none() {
        // The absence is the author's configuration, so it is not a failure; it is also not
        // a pass, so it is said out loud and carried into the result document.
        eprintln!("semantic stage not run: no judge is configured in .veridoc/config.json");
        warnings.push(crate::result::Warning {
            file: None,
            line: None,
            message: "semantic stage not run: no judge is configured, so prose is unchecked"
                .to_string(),
        });
    }
    let cited = match cite::cite_all(
        &claims,
        |citations| client.cite(citations),
        |claim, text| {
            judge_citation(judge_client.as_deref(), root, config, claim, text).unwrap_or_else(
                |detail| {
                    RUN_FAILURE.with(|slot| {
                        *slot.borrow_mut() = Some(detail.clone());
                    });
                    // The value is ignored once the slot is set; the run stops below.
                    Outcome::Supported
                },
            )
        },
    ) {
        Ok(outcome) => outcome,
        Err(detail) => {
            return Report::run_failure(client.with_fault(&format!("run failed: plugin: {detail}")))
        }
    };
    if let Some(detail) = RUN_FAILURE.with(|slot| slot.borrow_mut().take()) {
        return Report::run_failure(format!("run failed: judge: {detail}"));
    }
    for gap in &cited.errors {
        errors.push(bind_error_diagnostic(gap));
    }
    warnings.extend(cited.warnings.iter().cloned());
    for done in &cited.cited {
        let claim = claims
            .iter()
            .find(|c| c.id == done.claim_id)
            .expect("cited claim");
        outcomes.push(OutcomeOf {
            claim,
            outcome: match done.outcome {
                Outcome::Supported => DocOutcome::Supported,
                Outcome::Contradicted => DocOutcome::Contradicted,
            },
            residue: done.residue,
            channel: "citation",
            evidence: citation_ref_of(claim),
        });
    }
    if let Some(report) = interrupted("judging") {
        return report;
    }

    // 9. Briefs and the judge, per section and for the index, with the verdict cache.
    let mut judged_sections: Vec<String> = Vec::new();
    for section in &bound.sections {
        if section.claims.is_empty() {
            continue;
        }
        let named: Vec<&BoundClaim> = claims
            .iter()
            .filter(|c| section.claims.contains(&c.id))
            .collect();
        let brief = section_brief(Task::S1, &bound, section, &named, &outcomes);
        let ids = brief_cache_ids(&named, &section.body);
        match judge_task(
            Task::S1,
            &JudgeRun {
                judge: judge_client.as_deref(),
                root,
                config,
                brief: &brief,
                section_path: section.file.clone(),
                ids: ids.clone(),
            },
            |j, b| j.s1(b),
        ) {
            Ok(Some(verdict)) => {
                for unstated in &verdict.unstated_assertions {
                    warnings.push(crate::result::Warning {
                        file: Some(section.file.clone()),
                        line: Some(section.line),
                        message: format!(
                            "span {} states an assertion no named claim states: {}",
                            unstated.span, unstated.text
                        ),
                    });
                }
            }
            Ok(None) => {}
            Err(detail) => return Report::run_failure(format!("run failed: judge: {detail}")),
        }
        let brief = section_brief(Task::S2, &bound, section, &named, &outcomes);
        match judge_task(
            Task::S2,
            &JudgeRun {
                judge: judge_client.as_deref(),
                root,
                config,
                brief: &brief,
                section_path: section.file.clone(),
                ids,
            },
            |j, b| j.s2(b),
        ) {
            Ok(Some(verdict)) => {
                judged_sections.extend(section.claims.iter().cloned());
                for pair in &verdict.pairs {
                    errors.push(Diagnostic::error(
                        &section.file,
                        section.line,
                        format!(
                            "spans {} and {} disagree about claim {}",
                            pair.span_a, pair.span_b, pair.claim_id
                        ),
                    ));
                }
            }
            Ok(None) => {}
            Err(detail) => return Report::run_failure(format!("run failed: judge: {detail}")),
        }
    }

    // 10. A judge-channel claim is evaluated only by the S1 and S2 of the sections that
    //     name it; without a section naming it, it has no verdict.
    for claim in &claims {
        if matches!(claim.channel, BoundChannel::Judge) && !judged_sections.contains(&claim.id) {
            errors.push(Diagnostic::error(
                &claim.file,
                claim.line,
                format!(
                    "claim {} is on the judge channel with no verdict: {}",
                    claim.id,
                    if judge_client.is_some() {
                        "no section names it"
                    } else {
                        "no judge is configured"
                    }
                ),
            ));
        }
    }

    // 11. S3 over the whole index.
    let index_brief = index_brief(&bound);
    let index_ids = {
        let all: Vec<&BoundClaim> = claims.iter().collect();
        judge_ids_for(&all, &index_brief.render())
    };
    match judge_task(
        Task::S3,
        &JudgeRun {
            judge: judge_client.as_deref(),
            root,
            config,
            brief: &index_brief,
            section_path: "document tree".to_string(),
            ids: index_ids,
        },
        |j, b| j.s3(b),
    ) {
        Ok(Some(verdict)) => {
            for pair in &verdict.pairs {
                warnings.push(crate::result::Warning {
                    file: None,
                    line: None,
                    message: format!(
                        "nodes {} and {} ask the same thing",
                        pair.node_a, pair.node_b
                    ),
                });
            }
        }
        Ok(None) => {}
        Err(detail) => return Report::run_failure(format!("run failed: judge: {detail}")),
    }

    // 12. Judge-channel claims that ran: supported unless S2 contradicted them above.
    for claim in &claims {
        if matches!(claim.channel, BoundChannel::Judge)
            && judged_sections.contains(&claim.id)
            && !outcomes.iter().any(|o| o.claim.id == claim.id)
        {
            outcomes.push(OutcomeOf {
                claim,
                outcome: DocOutcome::Supported,
                residue: false,
                channel: "judge",
                evidence: "judge".to_string(),
            });
        }
    }

    // 13. The uncovered-symbol warning, or the inert line.
    if caps.contains(&Capability::Surface) {
        match client.surface() {
            Call::Result(surface) => {
                for referent in surface.referents.iter().filter(|r| r.public) {
                    if !claims.iter().any(|c| c.referent_id == referent.referent_id) {
                        warnings.push(crate::result::Warning {
                            file: None,
                            line: None,
                            message: format!(
                                "symbol {} is public and no claim covers it",
                                referent.name
                            ),
                        });
                    }
                }
            }
            Call::Refusal(error) => {
                return Report::run_failure(format!(
                    "run failed: plugin: the plugin refused surface ({}): {}",
                    error.code, error.message
                ))
            }
            Call::Lost(lost) => {
                return Report::run_failure(
                    client.with_fault(&format!("run failed: plugin: {lost}")),
                )
            }
        }
    } else {
        eprintln!(
            "surface not reported by plugin \"{}\"; uncovered symbols unknown",
            handshake.name
        );
    }

    // 14. Metrics and the coverage gate.
    let metrics = compute_metrics(
        &claims,
        &outcomes,
        &bound,
        &warnings,
        derived.rejected.len(),
    );
    if metrics.coverage < config.coverage.required {
        errors.push(Diagnostic {
            file: None,
            line: None,
            message: format!(
                "coverage {:.3} is below the required {:.3}",
                metrics.coverage, config.coverage.required
            ),
            severity: crate::diagnostic::Severity::Error,
        });
    }
    for node in &bound.documents {
        if node.not_applicable.is_none()
            && !bound
                .sections
                .iter()
                .any(|s| s.file == node.path && !s.claims.is_empty())
        {
            errors.push(Diagnostic {
                file: Some(node.path.clone()),
                line: Some(1),
                message: format!("node {} carries no claim", node.path),
                severity: crate::diagnostic::Severity::Error,
            });
        }
    }

    if !errors.is_empty() {
        errors.sort_by_key(|d| (d.file.clone().unwrap_or_default(), d.line.unwrap_or(0)));
        return Report::findings(errors);
    }

    // 15. The result document, then the formatters.
    let document = build_document(&bound, &claims, &outcomes, &warnings, metrics);
    let content_hash = result::result_hash(&document);
    let skipped = format::unchanged(root, &content_hash) && !force;
    let written = match result::write_result(root, &document) {
        Ok(_) => true,
        Err(e) => return Report::run_failure(format!("run failed: plugin: {e}")),
    };

    // 16. The input fingerprint `veridoc stat` compares a later checkout against. Failing to
    //     write it is a run failure: a run that records what it read is the only run a job that
    //     cannot call a model can trust later.
    if let Err(e) = inputs::collect(root).and_then(|files| inputs::write(root, &files)) {
        return Report::run_failure(format!("run failed: inputs: {e}"));
    }
    if skipped {
        eprintln!("documentation up to date; formatters skipped");
        return Report {
            exit: 0,
            diagnostics: warning_diagnostics(&warnings),
            result_written: written,
        };
    }
    if let Err(detail) =
        format::dispatch(root, config, &document, &root.join(".veridoc/result.json"))
    {
        return Report::run_failure(format!("run failed: formatter: {detail}"));
    }
    Report {
        exit: 0,
        diagnostics: warning_diagnostics(&warnings),
        result_written: written,
    }
}

fn bind_error_diagnostic(error: &bind::BindError) -> Diagnostic {
    let (file, line) = error.location();
    Diagnostic::error(file, line, error.to_string())
}

fn warning_diagnostics(warnings: &[crate::result::Warning]) -> Vec<Diagnostic> {
    warnings
        .iter()
        .map(|w| Diagnostic::warning(w.file.clone(), w.line, w.message.clone()))
        .collect()
}

struct OutcomeOf<'a> {
    claim: &'a BoundClaim,
    outcome: DocOutcome,
    residue: bool,
    channel: &'static str,
    evidence: String,
}

/// The evidence a scenario-channel claim carries: the `When` label of the scenario that ran,
/// which names the world and the call the verdict rests on.
fn scenario_when_of(claim: &BoundClaim) -> String {
    match &claim.channel {
        BoundChannel::Scenario { when, .. } => when.clone(),
        _ => String::new(),
    }
}

fn artefact_path_of(claim: &BoundClaim) -> String {
    match &claim.channel {
        BoundChannel::Artefact { path, .. } => path.clone(),
        _ => String::new(),
    }
}

fn citation_ref_of(claim: &BoundClaim) -> String {
    match &claim.channel {
        BoundChannel::Citation { store, id, .. } => format!("{store}:{id}"),
        _ => String::new(),
    }
}

/// The judge the project's config names. `None` is a project that runs only the
/// deterministic checks; a provider name this build does not speak is an error, because a
/// judge silently dropped by a typo would read as a clean run.
fn make_judge(loaded: &LoadedConfig) -> Result<Option<Box<dyn Judge>>, String> {
    let Some(judge) = &loaded.config.judge else {
        return Ok(None);
    };
    let provider = Provider::parse(&judge.provider)?;
    let key_env = judge
        .api_key_env
        .clone()
        .or_else(|| provider.default_key_env().map(str::to_string));
    let base = judge
        .base_url
        .clone()
        .unwrap_or_else(|| provider.default_base_url().to_string());
    Ok(Some(Box::new(ModelJudge {
        provider,
        model: judge.model.clone(),
        base_url: base.trim_end_matches('/').to_string(),
        api_key: key_env
            .as_deref()
            .and_then(|variable| std::env::var(variable).ok()),
        max_tokens: judge.max_tokens,
        additional_params: judge.additional_params.clone().unwrap_or(Value::Null),
        timeout: Duration::from_millis(judge.timeout_ms),
    })))
}

// A judge transport failure raised inside the citation `judge` callback, which must return
// an `Outcome` and cannot return `Err`.
thread_local! {
    static RUN_FAILURE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

// Sections whose re-judge has already been announced this run.
thread_local! {
    static REJUDGED: std::cell::RefCell<std::collections::HashSet<String>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

fn judge_citation(
    judge: Option<&dyn Judge>,
    root: &Path,
    config: &Config,
    claim: &BoundClaim,
    cited_text: &str,
) -> Result<Outcome, String> {
    let brief = Brief {
        task: Task::S2,
        section: citation_ref_of(claim),
        audience: None,
        question: None,
        spans: vec![
            BriefSpan {
                id: "sp1".into(),
                text: claim.text.clone(),
            },
            BriefSpan {
                id: "sp2".into(),
                text: cited_text.to_string(),
            },
        ],
        claims: vec![brief_claim(claim, "citation")],
        nodes: vec![],
    };
    let ids = judge_ids_for(&[claim], &format!("{}\u{0}{cited_text}", claim.text));
    let verdict = match judge_task(
        Task::S2,
        &JudgeRun {
            judge,
            root,
            config,
            brief: &brief,
            section_path: citation_ref_of(claim),
            ids,
        },
        |j, b| j.s2(b),
    )? {
        Some(verdict) => verdict,
        None => {
            return Err(format!(
                "claim {} cites {}, which no deterministic check can settle, and no judge is \
                 configured to judge it",
                claim.id,
                citation_ref_of(claim)
            ))
        }
    };
    Ok(if verdict.pairs.is_empty() {
        Outcome::Supported
    } else {
        Outcome::Contradicted
    })
}

/// The three cache inputs for a set of claims over a section body.
fn brief_cache_ids(claims: &[&BoundClaim], body: &str) -> (String, String, String) {
    judge_ids_for(claims, body)
}

fn judge_ids_for(claims: &[&BoundClaim], body: &str) -> (String, String, String) {
    let section = hash::section_hash(body);
    let mut per_claim: Vec<String> = claims
        .iter()
        .map(|c| {
            let asserts = match &c.channel {
                BoundChannel::Scenario { asserts, .. } => {
                    serde_json::to_string(asserts).unwrap_or_default()
                }
                _ => "null".to_string(),
            };
            hash::per_claim_hash(&c.id, kind_text(c.kind), &c.predicate, &c.text, &asserts)
        })
        .collect();
    let set = hash::claim_set_hash(&mut per_claim);
    let mut entries: Vec<String> = claims
        .iter()
        .map(|c| format!("{}\t{}", c.referent_id, c.signature))
        .collect();
    let revision = hash::referent_revision(&mut entries);
    (section, set, revision)
}

fn kind_text(kind: Kind) -> &'static str {
    match kind {
        Kind::Requires => "requires",
        Kind::Ensures => "ensures",
        Kind::Invariant => "invariant",
        Kind::Witness => "witness",
        Kind::Because => "because",
    }
}

/// Everything one judged task needs: the transport, the cache inputs, and the brief.
struct JudgeRun<'a> {
    judge: Option<&'a dyn Judge>,
    root: &'a Path,
    config: &'a Config,
    brief: &'a Brief,
    section_path: String,
    ids: (String, String, String),
}

/// One judged task with the verdict cache: a hit replays after re-verification, a miss
/// prints `re-judged`, and a verdict citing an unissued id is a run failure.
fn judge_task<T>(
    task: Task,
    run: &JudgeRun<'_>,
    call: impl FnOnce(&dyn Judge, &Brief) -> Result<T, JudgeError>,
) -> Result<Option<T>, String>
where
    T: serde::Serialize + DeserializeOwned,
{
    let (Some(judge), Some(settings)) = (run.judge, run.config.judge.as_ref()) else {
        return Ok(None);
    };
    let key = verdicts::key(
        task.name(),
        &run.ids.0,
        &run.ids.1,
        &run.ids.2,
        &settings.model,
        &settings.prompt_version,
    );
    let issued = run.brief.issued_ids();
    if let Some(cached) = verdicts::load(run.root, &key) {
        verdicts::verify_ids(task, &cached.verdict, &issued)?;
        return serde_json::from_value(cached.verdict)
            .map(Some)
            .map_err(|e| format!("judge verdict malformed: {e}"));
    }
    // One line per section per run: S1 and S2 of the same section share the section,
    // and the author edits the section, not the rule.
    if REJUDGED.with(|set| set.borrow_mut().insert(run.section_path.clone())) {
        eprintln!("re-judged {}", run.section_path);
    }
    let verdict = call(judge, run.brief).map_err(|e| e.to_string())?;
    let value = serde_json::to_value(&verdict).expect("a verdict is serializable");
    verdicts::verify_ids(task, &value, &issued)?;
    let _ = verdicts::store(run.root, &key, &value, &run.brief.render());
    Ok(Some(verdict))
}

fn brief_claim(claim: &BoundClaim, channel: &str) -> BriefClaim {
    let outcome = match channel {
        "scenario" | "artefact" | "citation" => "supported",
        _ => "judge channel",
    };
    BriefClaim {
        id: claim.id.clone(),
        kind: kind_text(claim.kind).to_string(),
        outcome: outcome.to_string(),
        text: claim.text.clone(),
        subject: claim.subject.clone(),
        signature: claim.signature.clone(),
        asserts: match &claim.channel {
            BoundChannel::Scenario { asserts, .. } => crate::brief::render_asserts(asserts),
            _ => vec![],
        },
    }
}

fn section_brief(
    task: Task,
    bound: &Bound,
    section: &bind::BoundSection,
    named: &[&BoundClaim],
    outcomes: &[OutcomeOf],
) -> Brief {
    let document = bound.documents.iter().find(|d| d.path == section.file);
    Brief {
        task,
        section: format!("{}#{}", section.file, section.subject),
        audience: document.map(|d| d.audience.clone()),
        question: document.map(|d| d.question.clone()),
        spans: section
            .spans
            .iter()
            .map(|s| BriefSpan {
                id: s.id.clone(),
                text: s.text.clone(),
            })
            .collect(),
        claims: named
            .iter()
            .map(|c| {
                let mut brief = brief_claim(
                    c,
                    outcomes
                        .iter()
                        .find(|o| o.claim.id == c.id)
                        .map(|o| o.channel)
                        .unwrap_or("judge"),
                );
                if let Some(o) = outcomes.iter().find(|o| o.claim.id == c.id) {
                    brief.outcome = format!("{} ({})", outcome_word(o.outcome), o.evidence);
                }
                brief
            })
            .collect(),
        nodes: vec![],
    }
}

fn outcome_word(outcome: DocOutcome) -> &'static str {
    match outcome {
        DocOutcome::Supported => "supported",
        DocOutcome::Contradicted => "contradicted",
        DocOutcome::RunFailed => "run-failed",
    }
}

fn index_brief(bound: &Bound) -> Brief {
    Brief {
        task: Task::S3,
        section: "document tree".into(),
        audience: None,
        question: None,
        spans: vec![],
        claims: vec![],
        nodes: bound
            .documents
            .iter()
            .map(|d| BriefNode {
                path: d.path.clone(),
                audience: d.audience.clone(),
                question: d.question.clone(),
            })
            .collect(),
    }
}

fn compute_metrics(
    claims: &[BoundClaim],
    outcomes: &[OutcomeOf],
    bound: &Bound,
    warnings: &[crate::result::Warning],
    redundancy: usize,
) -> Metrics {
    let bucket = |kind: &str| -> (usize, usize) {
        let in_bucket: Vec<&OutcomeOf> = outcomes
            .iter()
            .filter(|o| bucket_of(o.claim) == kind)
            .collect();
        let supported = in_bucket
            .iter()
            .filter(|o| o.outcome == DocOutcome::Supported)
            .count();
        (supported, in_bucket.len())
    };
    let (s_scn, t_scn) = bucket("scenario");
    let (s_art, t_art) = bucket("artefact");
    let (s_jud, t_jud) = bucket("judge");
    let supported = s_scn + s_art + s_jud;
    let total = t_scn + t_art + t_jud;
    let coverage = if total == 0 {
        1.0
    } else {
        supported as f64 / total as f64
    };
    let coverage_detail = format!(
        "{:.3} ({}/{} supported; scenario {}/{}, artefact {}/{}, judge {}/{})",
        coverage, supported, total, s_scn, t_scn, s_art, t_art, s_jud, t_jud
    );

    let mut leaves = 0usize;
    let mut custom = 0usize;
    for claim in claims {
        if let BoundChannel::Scenario { asserts, .. } = &claim.channel {
            for (_, leaf) in asserts.leaves() {
                leaves += 1;
                if matches!(leaf.r#is, veridoc_protocol::Check::Satisfies(_)) {
                    custom += 1;
                }
            }
        }
    }
    let custom_logic = if leaves == 0 {
        0.0
    } else {
        custom as f64 / leaves as f64
    };

    let residue = outcomes.iter().filter(|o| o.residue).count();
    let obligation = bound
        .documents
        .iter()
        .filter(|d| {
            d.not_applicable.is_none()
                && !bound
                    .sections
                    .iter()
                    .any(|s| s.file == d.path && !s.claims.is_empty())
        })
        .count();
    let unmatched_prose = warnings
        .iter()
        .filter(|w| w.message.contains("assertion no named claim states"))
        .count();

    Metrics {
        coverage,
        coverage_detail,
        custom_logic,
        unmatched_prose: unmatched_prose as u32,
        residue: residue as u32,
        residue_share: if claims.is_empty() {
            0.0
        } else {
            residue as f64 / claims.len() as f64
        },
        redundancy: redundancy as u32,
        obligation: obligation as u32,
    }
}

/// The coverage bucket of a claim: scenario and artefact keep their channels; citation and
/// judge-channel claims are judged, so they share the `judge` bucket.
fn bucket_of(claim: &BoundClaim) -> &'static str {
    match claim.channel {
        BoundChannel::Scenario { .. } => "scenario",
        BoundChannel::Artefact { .. } => "artefact",
        _ => "judge",
    }
}

fn build_document(
    bound: &Bound,
    claims: &[BoundClaim],
    outcomes: &[OutcomeOf],
    warnings: &[crate::result::Warning],
    metrics: Metrics,
) -> ResultDocument {
    ResultDocument {
        protocol_version: veridoc_protocol::PROTOCOL_VERSION.to_string(),
        generated_at: String::new(),
        tree: bound
            .documents
            .iter()
            .enumerate()
            .map(|(i, d)| result::TreeNode {
                path: d.path.clone(),
                audience: d.audience.clone(),
                question: d.question.clone(),
                order: i as u32,
                not_applicable: d.not_applicable.clone(),
            })
            .collect(),
        sections: bound
            .sections
            .iter()
            .map(|s| result::SectionResult {
                file: s.file.clone(),
                subject: s.subject.clone(),
                spans: s
                    .spans
                    .iter()
                    .map(|sp| result::Span {
                        id: sp.id.clone(),
                        text: sp.text.clone(),
                    })
                    .collect(),
                claims: s.claims.clone(),
                injections: s
                    .injections
                    .iter()
                    .map(|pulled| result::Injected {
                        source: pulled.source.clone(),
                        items: pulled.items,
                    })
                    .collect(),
            })
            .collect(),
        claims: claims
            .iter()
            .filter_map(|c| {
                let outcome = outcomes.iter().find(|o| o.claim.id == c.id)?;
                Some(ClaimResult {
                    id: c.id.clone(),
                    kind: c.kind,
                    text: c.text.clone(),
                    predicate: c.predicate.clone(),
                    concept: c.concept.clone(),
                    referent_id: c.referent_id.clone(),
                    signature: c.signature.clone(),
                    channel: outcome.channel.to_string(),
                    outcome: outcome.outcome,
                    residue: outcome.residue,
                    evidence: outcome.evidence.clone(),
                })
            })
            .collect(),
        concepts: bound.concepts.clone(),
        warnings: warnings.to_vec(),
        metrics,
    }
}
