//! `veridoc-1.profile.json` is the promise the profile makes; this file is what stops it from being
//! an unkept one. It reads the published spec and pins it against the two tables the suite
//! actually runs on — [`RULES`] and [`GATES`] — so a spec that declares a rule the driver does
//! not drive, or a dependency the driver does not honour, fails here instead of reporting a rule
//! that never ran as a pass.

use std::collections::BTreeSet;
use std::path::PathBuf;

use plugin_rpc::conformance::{Defect, ProfileSpec};
use veridoc_conformance::{GATES, RULES};

fn spec() -> ProfileSpec {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../veridoc-1.profile.json");
    ProfileSpec::load(&path)
        .unwrap_or_else(|e| panic!("{} is not a readable profile spec: {e}", path.display()))
}

/// `ProfileSpec::load` refuses a spec whose schema paths do not resolve beside it, so this
/// failing means a committed schema went missing or moved.
#[test]
fn every_schema_the_spec_names_is_committed_beside_it() {
    let spec = spec();
    assert_eq!(
        spec.schema_files().len(),
        15,
        "eight result schemas and seven params schemas, less the surface params the profile does not have"
    );
}

#[test]
fn every_rule_the_spec_declares_is_one_the_suite_drives() {
    let spec = spec();
    for rule in &spec.conformance.rules {
        assert!(
            RULES.contains(&rule.id.as_str()),
            "`{}` is declared by the spec but the suite does not drive it, so it would be \
             reported without ever having been run",
            rule.id
        );
    }
}

#[test]
fn the_suite_restates_no_core_rule_of_its_own() {
    let spec = spec();
    let declared: BTreeSet<&str> = spec
        .conformance
        .rules
        .iter()
        .map(|r| r.id.as_str())
        .collect();
    let driven: BTreeSet<&str> = RULES.iter().copied().collect();
    assert!(
        declared.is_subset(&driven),
        "the spec declares a rule the suite does not know: {:?}",
        declared.difference(&driven).collect::<Vec<_>>()
    );
    // The mechanism runs these three itself, from this spec, before the suite program starts;
    // the suite keeps them in its own human-readable report and leaves them out of verdicts.
    assert_eq!(
        &driven - &declared,
        BTreeSet::from(["framing-intact", "handshake-schema", "unknown-method-frame"])
    );
}

#[test]
fn the_declared_dependencies_are_the_ones_the_suite_skips_on() {
    let spec = spec();
    for rule in &spec.conformance.rules {
        let gate = GATES
            .iter()
            .find(|(id, _, _)| *id == rule.id)
            .unwrap_or_else(|| panic!("`{}` has no gate", rule.id));
        assert_eq!(
            rule.depends_on, gate.2,
            "`{}` states a different dependency than the suite enforces",
            rule.id
        );
    }
}

#[test]
fn every_gated_capability_is_declared_and_explained() {
    let spec = spec();
    let required: BTreeSet<&str> = spec
        .methods
        .values()
        .filter_map(|method| method.requires_capability.as_deref())
        .collect();
    let gated: BTreeSet<&str> = spec
        .conformance
        .rules
        .iter()
        .filter_map(|rule| {
            GATES
                .iter()
                .find(|(id, _, _)| *id == rule.id)
                .and_then(|(_, capability, _)| capability.map(|c| c.name()))
        })
        .collect();
    assert_eq!(
        required, gated,
        "a method is gated on a capability no rule checks, or a rule on one no method names"
    );
    for capability in &required {
        assert!(
            spec.capabilities.contains_key(*capability),
            "`{capability}` gates a method but has no ifAbsent sentence"
        );
    }
}

#[test]
fn every_rule_carries_a_defect_that_names_only_it() {
    let spec = spec();
    for rule in &spec.conformance.rules {
        match &rule.defect {
            Defect::Scripted {
                run,
                env,
                expect_violation,
            } => {
                assert!(
                    run.len() == 1
                        && run[0].starts_with("target/")
                        && std::path::Path::new(&run[0])
                            .file_name()
                            .and_then(|n| n.to_str())
                            == Some("veridoc-fakeplugin"),
                    "`{}`'s defect must start the binary this repository builds, as one path \
                     under `target/`, so the runner finds it whichever profile CI builds: {run:?}",
                    rule.id
                );
                assert_eq!(env.len(), 1, "`{}` scripts exactly one variable", rule.id);
                assert_eq!(
                    expect_violation,
                    &vec![rule.id.clone()],
                    "`{}`'s defect must be shown to fire its own rule and no other",
                    rule.id
                );
            }
            Defect::Unverified { unverified } => {
                panic!("rule `{}` is published unverified: {unverified}", rule.id)
            }
        }
    }
}

#[test]
fn the_spec_describes_this_host() {
    let spec = spec();
    assert_eq!(spec.protocol_version, veridoc_protocol::PROTOCOL_VERSION);
    assert_eq!(spec.manifest, "veridoc-plugin.json");
    assert_eq!(spec.conformance.suite, ["veridoc", "validate-plugin"]);
    assert_eq!(spec.conformance.deadline_ms, Some(20_000));
    let methods: BTreeSet<&str> = spec.methods.keys().map(String::as_str).collect();
    assert_eq!(
        methods,
        BTreeSet::from([
            "bivalence",
            "cite",
            "collect",
            "derivable",
            "init",
            "resolve",
            "run",
            "surface",
        ])
    );
    assert!(
        spec.methods["surface"].params.is_none(),
        "surface takes no parameters, so the spec must not name a params schema for it"
    );
    assert_eq!(
        spec.methods["run"].deadline_field.as_deref(),
        Some("deadlineMs")
    );
}
