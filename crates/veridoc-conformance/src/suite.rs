//! `veridoc validate-plugin --out <dir>`: this suite in the role the pluginrpc mechanism calls
//! its suite program (`spec/spec.md` §12 item 6). The mechanism writes `<dir>`'s `profile.json`,
//! `manifest.json`, and every schema the profile names, runs `<argv> --out <dir>`, and then
//! reads `<dir>/verdicts.json`.
//!
//! The rule set comes from the profile spec rather than from [`crate::RULES`], because the
//! mechanism requires exactly one verdict for every rule the profile declares and none it does
//! not. A rule the profile declares that this suite does not drive is therefore a suite error:
//! the alternative is to report a rule that never ran as a pass, which §12 item 3 forbids.

use std::path::{Path, PathBuf};

use plugin_rpc::conformance::{Manifest, Outcome, ProfileSpec, SuiteVerdict};
use veridoc_engine::install;
use veridoc_engine::plugins;

use crate::{drive, Report, RULES};

/// Drive the plugin the mechanism's manifest names, then write `<dir>/verdicts.json`. An `Err`
/// here is a suite error (§12 item 5): no file is written, so the mechanism charges the failure
/// to this suite and not to the plugin.
pub fn run(dir: &Path) -> Result<Vec<SuiteVerdict>, String> {
    let profile_path = dir.join("profile.json");
    let profile = ProfileSpec::load(&profile_path)
        .map_err(|e| format!("{} is unreadable: {e}", profile_path.display()))?;
    let manifest_path = dir.join("manifest.json");
    let manifest = Manifest::load(&manifest_path)
        .map_err(|e| format!("{} is unreadable: {e}", manifest_path.display()))?;
    // §12 item 6 starts this program in the mechanism's own working directory, which is the
    // checkout the author ran the runner from, and the manifest the mechanism copied holds its
    // paths exactly as that checkout wrote them. They resolve beside the checkout, not beside
    // the copy.
    let root =
        std::env::current_dir().map_err(|e| format!("the working directory is unreadable: {e}"))?;
    // The same `build` steps `veridoc validate-plugin` runs without `--out`, for the same
    // reason: a pass has to be a pass on the plugin that exists now, and validation is what
    // runs them (§11).
    for argv in &manifest.build {
        install::run_step("build", &root, argv)?;
    }
    if manifest.command.is_empty() {
        return Err(format!(
            "the manifest of plugin \"{}\" declares an empty \"command\"",
            manifest.name
        ));
    }
    let emitter = emitter_argv(&manifest)?;
    let emitter: Vec<PathBuf> = plugins::resolve_argv(&root, &emitter)
        .into_iter()
        .map(PathBuf::from)
        .collect();
    let argv = plugins::resolve_argv(&root, &manifest.command);
    // The manifest's own deadline, then the profile's, then the mechanism's default (§9).
    let deadline_ms = profile.deadline_ms(Some(&manifest));
    let report = drive(&manifest.name, &root, &argv, &emitter, Some(deadline_ms))?;
    let verdicts = verdicts(&profile, &report)?;
    let path = dir.join("verdicts.json");
    let text = serde_json::to_string_pretty(&verdicts).map_err(|e| e.to_string())?;
    std::fs::write(&path, text + "\n")
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(verdicts)
}

/// The emitter argv the manifest names. Without one there is no fixture to drive the rules
/// against, and a suite with nothing to run has failed, whatever the plugin did (§12 item 5).
fn emitter_argv(manifest: &Manifest) -> Result<Vec<String>, String> {
    manifest
        .conformance
        .as_ref()
        .and_then(|c| c.emitter.clone())
        .filter(|argv| !argv.is_empty())
        .ok_or(format!(
            "the manifest of plugin \"{}\" names no conformance emitter, so there is no fixture \
             to drive the rules against",
            manifest.name
        ))
}

/// One verdict per rule the profile declares, in the order it declares them, and none it does
/// not.
fn verdicts(profile: &ProfileSpec, report: &Report) -> Result<Vec<SuiteVerdict>, String> {
    for rule in &profile.conformance.rules {
        if !RULES.contains(&rule.id.as_str()) {
            return Err(format!(
                "the profile spec declares the rule `{}`, which this suite does not drive: it \
                 would be reported without ever having been run",
                rule.id
            ));
        }
    }
    for violation in &report.violations {
        if !profile
            .conformance
            .rules
            .iter()
            .any(|rule| rule.id == violation.rule)
        {
            // The mechanism refuses a verdict for a rule it does not declare, because a core
            // rule restated here would be charged to a rule the profile never wrote. So it is
            // said aloud on stderr instead of written into the file.
            eprintln!(
                "note  {}: {} (not a rule this profile declares)",
                violation.rule, violation.detail
            );
        }
    }
    Ok(profile
        .conformance
        .rules
        .iter()
        .map(|rule| verdict(report, &rule.id))
        .collect())
}

fn verdict(report: &Report, rule: &str) -> SuiteVerdict {
    if let Some(violation) = report.violations.iter().find(|v| v.rule == rule) {
        return SuiteVerdict {
            rule: rule.to_string(),
            outcome: Outcome::Violation,
            note: Some(violation.detail.clone()),
            reason: None,
        };
    }
    if let Some(skipped) = report.not_applicable.iter().find(|n| n.rule == rule) {
        return SuiteVerdict {
            rule: rule.to_string(),
            outcome: Outcome::NotApplicable,
            note: None,
            reason: Some(skipped.reason.clone()),
        };
    }
    SuiteVerdict {
        rule: rule.to_string(),
        outcome: Outcome::Pass,
        note: None,
        reason: None,
    }
}
