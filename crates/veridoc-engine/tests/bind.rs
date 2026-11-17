//! The bind stage, rule by rule: each row of the paths table in `docs/spec.md` produces its
//! exact message, and a conforming project binds with the selection resolved and no errors.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use veridoc_engine::bind::{self, BindFailure, CollectWire};
use veridoc_protocol::{Call, Loss, UNUSABLE_RESULT};
use veridoc_protocol::{Capability, ResolveResult};

fn tmp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vd-engine-bind-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

const SPEC: &str = "# Architecture\n\
    \n\
    ## What the scanner refuses {#refuses selects=\"concept:project-scan\"}\n\
    \n\
    The scanner walks upward and stops at the first project root.\n";

const FEATURE: &str = "Feature: project scan\n\
    \n\
    @id:config/rejects-missing-root\n\
    Scenario: A missing project root is refused\n\
    Given the empty-directory fixture\n\
    When the run-probe action runs in that directory\n";

fn scaffold(name: &str) -> PathBuf {
    let root = tmp_dir(name);
    write(&root, "docs/architecture.spec.md", SPEC);
    write(&root, "features/cli.feature", FEATURE);
    root
}

fn collect() -> CollectWire {
    serde_json::from_value(veridoc_fakeplugin::default_collect(&serde_json::Value::Null))
        .expect("default collect")
}

fn all_capabilities() -> Vec<Capability> {
    vec![
        Capability::Collect,
        Capability::Resolution,
        Capability::Derivability,
        Capability::Bivalence,
        Capability::Execution,
        Capability::Citation,
        Capability::Surface,
    ]
}

fn resolve_default(keys: &[String]) -> Call<ResolveResult> {
    let value = veridoc_fakeplugin::default_resolve(&json!({ "refs": keys }));
    serde_json::from_value(value)
        .map(Call::Result)
        .unwrap_or_else(|e| Call::Lost(Loss::Profile {
            reason: UNUSABLE_RESULT.to_string(),
            detail: e.to_string(),
        }))
}

fn run(
    root: &Path,
    collect: CollectWire,
) -> Result<veridoc_engine::bind::Bound, BindFailure> {
    bind::bind(
        root,
        "veridoc-fakeplugin",
        &all_capabilities(),
        &[],
        collect,
        resolve_default,
    )
}

fn diagnostics(result: Result<veridoc_engine::bind::Bound, BindFailure>) -> Vec<String> {
    match result {
        Ok(_) => panic!("expected bind errors, got success"),
        Err(BindFailure::Errors(errors)) => errors.iter().map(|e| e.diagnostic()).collect(),
        Err(BindFailure::RunFailure(why)) => panic!("expected bind errors, got run failure: {why}"),
    }
}

#[test]
fn a_conforming_project_binds_with_the_selection_resolved() {
    let root = scaffold("ok");
    let bound = run(&root, collect()).expect("the default project binds");
    assert_eq!(bound.claims.len(), 1);
    assert_eq!(bound.claims[0].referent_id, "vd:r:probe");
    assert_eq!(bound.claims[0].id, {
        // Identity is the resolved referent joined with the predicate, hashed.
        veridoc_engine::hash::claim_id("vd:r:probe", "rejects-missing-project-root")
    });
    let section = bound
        .sections
        .iter()
        .find(|s| s.anchor == "refuses")
        .expect("the section");
    assert_eq!(section.selects, vec![bound.claims[0].id.clone()]);
    assert!(bound.warnings.is_empty(), "unexpected warnings: {:?}", bound.warnings);
}

#[test]
fn an_undeclared_spec_file_is_reported_at_line_one() {
    let root = scaffold("undeclared");
    write(&root, "docs/undeclared.spec.md", "# U\n\n## S {#s}\n\nnothing\n");
    let lines = diagnostics(run(&root, collect()));
    assert!(
        lines.contains(&"docs/undeclared.spec.md:1: no document node in index.doc.ts declares this file".to_string()),
        "got {lines:?}"
    );
}

#[test]
fn a_claim_with_two_channels_names_the_count() {
    let root = scaffold("channels");
    let mut wire = collect();
    wire.claims[0].channel.citation = Some(veridoc_protocol::Citation {
        store: "session".into(),
        id: "01a1e512".into(),
        turn: None,
    });
    let lines = diagnostics(run(&root, wire));
    assert_eq!(
        lines[0],
        "src/cli.doc.ts:3: claim rejects-missing-project-root declares 2 channels; exactly one is required"
    );
}

#[test]
fn a_claim_with_no_channel_is_reported_too() {
    let root = scaffold("nochannel");
    let mut wire = collect();
    wire.claims[0].channel = Default::default();
    let lines = diagnostics(run(&root, wire));
    assert_eq!(
        lines[0],
        "src/cli.doc.ts:3: claim rejects-missing-project-root declares 0 channels; exactly one is required"
    );
}

#[test]
fn a_second_claim_with_the_same_identity_names_the_owner() {
    let root = scaffold("dup");
    let mut wire = collect();
    let mut second = wire.claims[0].clone();
    second.file = "src/other.doc.ts".into();
    second.line = 9;
    wire.claims.push(second);
    let lines = diagnostics(run(&root, wire));
    assert_eq!(
        lines[0],
        "src/other.doc.ts:9: claim rejects-missing-project-root duplicates \
         c{}, owned by src/cli.doc.ts:3"
            .replace("c{}", &veridoc_engine::hash::claim_id("vd:r:probe", "rejects-missing-project-root"))
    );
}

#[test]
fn an_unresolved_referent_is_reported_with_the_plugin_reason() {
    let root = scaffold("unresolved");
    let mut wire = collect();
    wire.claims[0].subject = veridoc_protocol::RefToken::Symbol {
        name: "gone".into(),
        from: "src/cli.ts".into(),
    };
    let lines = diagnostics(run(&root, wire));
    assert_eq!(
        lines[0],
        "src/cli.doc.ts:3: referent src/cli.ts|gone does not resolve: no exported member"
    );
}

#[test]
fn a_scenario_acting_on_another_referent_is_an_error() {
    let root = scaffold("wrongref");
    let mut wire = collect();
    wire.actions[0].calls = veridoc_protocol::RefToken::Symbol {
        name: "other".into(),
        from: "src/cli.ts".into(),
    };
    let lines = diagnostics(run(&root, wire));
    assert!(
        lines[0].starts_with(
            "src/cli.doc.ts:3: scenario config/rejects-missing-root acts on src/cli.ts|other, not the claim's referent vd:r:probe"
        ) || lines[0].starts_with(
            "src/cli.doc.ts:3: referent src/cli.ts|other does not resolve"
        ),
        "got {lines:?}"
    );
}

#[test]
fn an_undeclared_observable_is_reported() {
    let root = scaffold("obs");
    let mut wire = collect();
    let mut value: Value = serde_json::to_value(&wire.claims[0].channel.asserts).unwrap();
    value["obs"] = json!("exit.unknown");
    wire.claims[0].channel.asserts = Some(value);
    let lines = diagnostics(run(&root, wire));
    assert_eq!(
        lines[0],
        "src/cli.doc.ts:3: observable exit.unknown is not declared in any .obs.ts"
    );
}

#[test]
fn an_observable_the_action_cannot_produce_is_reported() {
    let root = scaffold("produces");
    let mut wire = collect();
    wire.actions[0].produces = vec!["stderr.text".into()];
    let lines = diagnostics(run(&root, wire));
    assert_eq!(
        lines[0],
        format!(
            "src/cli.doc.ts:3: claim {} reads observable exit.code, which action run-probe cannot produce",
            veridoc_engine::hash::claim_id("vd:r:probe", "rejects-missing-project-root")
        )
    );
}

#[test]
fn a_scenario_naming_an_undeclared_fixture_is_reported() {
    let root = scaffold("fixture");
    let mut wire = collect();
    wire.fixtures = vec!["some-other-fixture".into()];
    let lines = diagnostics(run(&root, wire));
    assert_eq!(
        lines[0],
        "features/cli.feature:5: scenario config/rejects-missing-root refers to undeclared fixture the empty-directory fixture"
    );
}

#[test]
fn a_scenario_without_its_tag_is_reported() {
    let root = scaffold("notag");
    write(
        &root,
        "features/cli.feature",
        "Feature: project scan\n\nScenario: A missing project root is refused\n  Given the empty-directory fixture\n  When the run-probe action runs\n",
    );
    let lines = diagnostics(run(&root, collect()));
    assert_eq!(
        lines[0],
        "features/cli.feature:3: scenario has no @id:<slug> tag"
    );
}

#[test]
fn a_section_without_an_id_is_reported() {
    let root = scaffold("noid");
    write(
        &root,
        "docs/architecture.spec.md",
        "# Architecture\n\n## No id\n\nbody\n",
    );
    let lines = diagnostics(run(&root, collect()));
    assert!(
        lines
            .iter()
            .any(|l| l == "docs/architecture.spec.md:3: section has no id; add {#id}"),
        "got {lines:?}"
    );
}

#[test]
fn a_citation_naming_an_undeclared_store_is_reported() {
    let root = scaffold("store");
    let mut wire = collect();
    wire.claims[0].channel.scenario = None;
    wire.claims[0].channel.asserts = None;
    wire.claims[0].channel.citation = Some(veridoc_protocol::Citation {
        store: "tickets".into(),
        id: "42".into(),
        turn: None,
    });
    let lines = diagnostics(run(&root, wire));
    assert_eq!(
        lines[0],
        "src/cli.doc.ts:3: citation names undeclared store tickets"
    );
}

#[test]
fn a_supports_link_to_a_missing_section_is_reported() {
    let root = scaffold("supports");
    let mut wire = collect();
    wire.claims[0].supports = vec!["docs/architecture.spec.md#nope".into()];
    let lines = diagnostics(run(&root, wire));
    assert!(
        lines[0].ends_with("supports docs/architecture.spec.md#nope, which does not exist"),
        "got {lines:?}"
    );
}

#[test]
fn a_bad_assertion_node_names_the_tree_path() {
    let root = scaffold("badnode");
    let mut wire = collect();
    wire.claims[0].channel.asserts = Some(json!({ "all": [{ "wat": true }] }));
    let lines = diagnostics(run(&root, wire));
    assert!(
        lines[0].contains("has an assertion node with neither obs nor all/any/not at all[0]"),
        "got {lines:?}"
    );
}

#[test]
fn an_empty_all_node_is_reported() {
    let root = scaffold("emptyall");
    let mut wire = collect();
    wire.claims[0].channel.asserts = Some(json!({ "all": [] }));
    let lines = diagnostics(run(&root, wire));
    assert!(lines[0].contains("has an empty all node"), "got {lines:?}");
}

#[test]
fn a_skipped_sidecar_is_a_run_failure_not_a_finding() {
    let root = scaffold("skipped");
    let mut wire = collect();
    wire.skipped = vec![veridoc_protocol::Skipped {
        file: "src/broken.doc.ts".into(),
        reason: "import threw".into(),
    }];
    match run(&root, wire) {
        Err(BindFailure::RunFailure(why)) => {
            assert_eq!(why, "sidecar src/broken.doc.ts did not import: import threw");
        }
        other => panic!("expected a run failure, got {other:?}"),
    }
}

#[test]
fn a_missing_bivalence_capability_makes_scenario_claims_errors() {
    let root = scaffold("nobival");
    let result = bind::bind(
        &root,
        "veridoc-fakeplugin",
        &[Capability::Collect, Capability::Resolution, Capability::Execution],
        &[],
        collect(),
        resolve_default,
    );
    let lines = diagnostics(result);
    assert!(
        lines[0].contains("cannot take the scenario channel: plugin \"veridoc-fakeplugin\" declares no bivalence"),
        "got {lines:?}"
    );
}

#[test]
fn an_unsatisfied_node_without_a_reason_is_reported() {
    let root = scaffold("unsat");
    let mut wire = collect();
    // The declared node names a file that is not on disk and carries no notApplicable reason.
    wire.documents[0].path = "docs/missing.spec.md".into();
    let lines = diagnostics(run(&root, wire));
    assert!(
        lines
            .iter()
            .any(|l| l == "docs/missing.spec.md:1: declared node is neither satisfied nor notApplicable with a reason")
            && lines
                .iter()
                .any(|l| l == "docs/architecture.spec.md:1: no document node in index.doc.ts declares this file"),
        "got {lines:?}"
    );
}

#[test]
fn a_not_applicable_node_with_a_reason_is_satisfied() {
    let root = scaffold("na");
    let mut wire = collect();
    wire.documents[0].path = "docs/missing.spec.md".into();
    wire.documents[0].not_applicable = Some("no plugin-facing surface in v1".into());
    // The notApplicable node no longer backs the default `supports` link; drop that link.
    wire.claims[0].supports.clear();
    std::fs::remove_file(root.join("docs/architecture.spec.md")).unwrap();
    match run(&root, wire) {
        Ok(bound) => assert!(bound.sections.is_empty()),
        Err(BindFailure::Errors(errors)) => {
            panic!("notApplicable node should satisfy the tree: {errors:?}")
        }
        Err(BindFailure::RunFailure(why)) => panic!("unexpected run failure: {why}"),
    }
}

#[test]
fn an_artefact_escaping_the_root_is_refused() {
    let root = scaffold("escape");
    let mut wire = collect();
    wire.claims[0].channel.scenario = None;
    wire.claims[0].channel.asserts = None;
    wire.claims[0].channel.artefact = Some(veridoc_protocol::Artefact {
        path: "../outside.bin".into(),
        sha256: "ab".into(),
    });
    let lines = diagnostics(run(&root, wire));
    assert_eq!(
        lines[0],
        "src/cli.doc.ts:3: artefact path ../outside.bin escapes the project root"
    );
}

#[test]
fn a_contains_over_a_number_observable_is_a_bind_error() {
    let root = scaffold("containstype");
    let mut wire = collect();
    let mut value: Value = serde_json::to_value(&wire.claims[0].channel.asserts).unwrap();
    value["obs"] = json!("exit.code");
    value["is"] = json!({ "contains": 1 });
    wire.claims[0].channel.asserts = Some(value);
    let lines = diagnostics(run(&root, wire));
    assert!(
        lines[0].contains("asserts contains over observable exit.code of type number; it must be string or string[]"),
        "got {lines:?}"
    );
}

#[test]
fn a_section_selecting_nothing_and_an_unselected_claim_are_warnings() {
    let root = scaffold("warn");
    write(
        &root,
        "docs/architecture.spec.md",
        "# Architecture\n\n## Unrelated {#unrelated selects=\"concept:other\"}\n\nNothing selects this.\n",
    );
    // The rewritten file no longer carries the anchor the default `supports` link names.
    let mut wire = collect();
    wire.claims[0].supports.clear();
    let bound = run(&root, wire).expect("binds");
    let messages: Vec<&str> = bound.warnings.iter().map(|w| w.message.as_str()).collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("section docs/architecture.spec.md#unrelated selects no claim")),
        "got {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("is selected by no section")),
        "got {messages:?}"
    );
}
