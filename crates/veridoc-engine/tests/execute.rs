//! Execution against the scripted fake plugin: a holding tree is supported, a failing tree
//! contradicts with the observed values and both readings, `ok: false` and a scripted
//! deadline are run failures that leave the claim in no outcome list.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;
use veridoc_engine::bind::{BoundChannel, BoundClaim};
use veridoc_engine::execute::{self, Outcome, READING_CODE_WRONG, READING_SCENARIO_WRONG};
use veridoc_engine::plugin::{Launch, PluginClient};
use veridoc_protocol::{AssertLeaf, Asserts, Check, Kind, RunFailureKind};

fn fakeplugin() -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    let start = exe
        .parent()
        .expect("test binary in a directory")
        .to_path_buf();
    if let Some(found) = find_fakeplugin(&start) {
        return found;
    }
    let status = std::process::Command::new(env!("CARGO"))
        .args(["build", "-p", "veridoc-fakeplugin"])
        .status()
        .expect("build the fake plugin");
    assert!(status.success(), "cargo build -p veridoc-fakeplugin failed");
    find_fakeplugin(&start).expect("veridoc-fakeplugin still absent after building it")
}

fn find_fakeplugin(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        for name in ["veridoc-fakeplugin", "veridoc-fakeplugin.exe"] {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        dir = dir.parent()?.to_path_buf();
    }
}

fn launch(script: serde_json::Value) -> Launch {
    Launch {
        argv: vec![fakeplugin().to_string_lossy().into_owned()],
        env: std::collections::BTreeMap::from([(
            "VERIDOC_FAKE_SCRIPT".to_string(),
            script.to_string(),
        )]),
        cwd: None,
    }
}

fn scenario_claim(asserts: Asserts) -> BoundClaim {
    BoundClaim {
        id: "c8223c485f101870d".into(),
        kind: Kind::Ensures,
        concept: "project-scan".into(),
        predicate: "rejects-missing-project-root".into(),
        text: "A missing project root is refused.".into(),
        file: "src/cli.doc.ts".into(),
        line: 3,
        subject: "probe".into(),
        referent_id: "veridoc:referent:probe".into(),
        signature: "function probe(dir: string): number".into(),
        channel: BoundChannel::Scenario {
            scenario_id: "config/rejects-missing-root".into(),
            fixture: "empty-directory".into(),
            action: "run-probe".into(),
            args: vec![],
            asserts,
        },
    }
}

fn exit_code(expected: serde_json::Value) -> Asserts {
    Asserts::Leaf(AssertLeaf {
        obs: "exit.code".into(),
        r#is: Check::Equals(expected),
    })
}

/// The handshake-first rule (§6) is the mechanism's now, so a connection only exists once the
/// handshake has been answered: every fixture connects rather than spawning.
fn client(script: serde_json::Value) -> PluginClient {
    PluginClient::connect(&launch(script), Duration::from_secs(30))
        .expect("connect the fake plugin")
        .0
}

#[test]
fn a_holding_tree_is_supported() {
    let client = client(json!({})); // the default run answers exit.code = "2"
    let executed = execute::execute(
        &[scenario_claim(exit_code(json!(2)))],
        30_000,
        |req, host| client.run(req, host),
    )
    .expect("run");
    assert_eq!(executed[0].outcome, Outcome::Supported);
    assert!(executed[0].observed.is_empty());
}

#[test]
fn a_failing_tree_contradicts_with_values_and_both_readings() {
    let client = client(json!({}));
    let executed = execute::execute(
        &[scenario_claim(exit_code(json!(0)))],
        30_000,
        |req, host| client.run(req, host),
    )
    .expect("run");
    assert_eq!(executed[0].outcome, Outcome::Contradicted);
    assert_eq!(executed[0].observed, vec!["exit.code = 2".to_string()]);
    // The diagnostic that step 10 renders carries both readings verbatim.
    assert!(READING_CODE_WRONG.contains("the code is wrong and the sentence is right"));
    assert!(READING_SCENARIO_WRONG.contains("the scenario or the assertion is wrong"));
}

#[test]
fn a_string_contains_is_substring_and_an_array_contains_is_membership() {
    let client = client(json!({
        "run": {
            "ok": true,
            "observed": [
                { "observable": "stderr.text", "value": "\"no .veridoc/config.json above /tmp/x\"" },
                { "observable": "writtenFiles", "value": "[\"a.txt\", \"b.txt\"]" }
            ],
            "satisfied": []
        }
    }));
    let tree = Asserts::All {
        all: vec![
            Asserts::Leaf(AssertLeaf {
                obs: "stderr.text".into(),
                r#is: Check::Contains(json!(".veridoc/config.json")),
            }),
            Asserts::Leaf(AssertLeaf {
                obs: "writtenFiles".into(),
                r#is: Check::Contains(json!("b.txt")),
            }),
        ],
    };
    let executed = execute::execute(&[scenario_claim(tree)], 30_000, |req, host| {
        client.run(req, host)
    })
    .expect("run");
    assert_eq!(executed[0].outcome, Outcome::Supported);
}

#[test]
fn ok_false_is_a_run_failure_with_the_plugins_kind() {
    let client = client(json!({
        "run": {
            "ok": false,
            "observed": [],
            "satisfied": [],
            "failure": { "kind": "fixture", "detail": "fixture blew up" }
        }
    }));
    let failure = execute::execute(
        &[scenario_claim(exit_code(json!(2)))],
        30_000,
        |req, host| client.run(req, host),
    )
    .expect_err("ok: false must stop the run");
    assert_eq!(failure.kind, RunFailureKind::Fixture);
    assert!(failure.detail.contains("fixture blew up"));
}

#[test]
fn a_scripted_deadline_is_a_run_failure_that_leaves_the_claim_absent() {
    let client = client(json!({ "_sleepMs": { "run": 500 } }));
    let failure = execute::execute(&[scenario_claim(exit_code(json!(2)))], 50, |req, host| {
        client.run(req, host)
    })
    .expect_err("the deadline must stop the run");
    assert_eq!(failure.kind, RunFailureKind::Deadline);
    assert_eq!(
        failure.detail,
        "scenario config/rejects-missing-root outlived 50 ms"
    );
}

#[test]
fn a_satisfies_leaf_the_plugin_skipped_is_a_run_failure_not_a_false() {
    let client = client(json!({
        "run": { "ok": true, "observed": [], "satisfied": [] }
    }));
    let tree = Asserts::Leaf(AssertLeaf {
        obs: "writtenFiles".into(),
        r#is: Check::Satisfies(veridoc_protocol::SatisfiesSpec {
            source: "f => f.length > 0".into(),
            accepts: Some(json!(["a"])),
            rejects: Some(json!([])),
        }),
    });
    let failure = execute::execute(&[scenario_claim(tree)], 30_000, |req, host| {
        client.run(req, host)
    })
    .expect_err("an unanswered leaf must stop the run");
    assert_eq!(failure.kind, RunFailureKind::Plugin);
    assert_eq!(
        failure.detail,
        "plugin answered no result for leaf c8223c485f101870d#l0"
    );
}
