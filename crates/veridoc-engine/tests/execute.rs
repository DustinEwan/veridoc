//! Execution against the scripted fake plugin: a holding tree is supported, a failing tree
//! contradicts with the observed values and both readings, and a scenario that cannot run is
//! recorded as that claim's `run-failed` result while the rest of the project keeps judging.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;
use veridoc_engine::bind::{BoundChannel, BoundClaim};
use veridoc_engine::execute::{
    self, Executed, Outcome, READING_CODE_WRONG, READING_SCENARIO_WRONG,
};
use veridoc_engine::plugin::{Launch, PluginClient};
use veridoc_protocol::{AssertLeaf, Asserts, Check, Kind, RunFailureKind, ValueDecl, ValueKind};

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

/// The claim as `bind` leaves it: the scenario's two labels and the types of the values it
/// produces arrived beside the claim, so they are already on the channel.
fn scenario_claim(id: &str, asserts: Asserts) -> BoundClaim {
    BoundClaim {
        id: id.into(),
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
            given: "an empty directory".into(),
            when: "run probe-plugin in an empty directory".into(),
            values: vec![
                ValueDecl {
                    name: "exit.code".into(),
                    kind: ValueKind::Number,
                },
                ValueDecl {
                    name: "stderr.text".into(),
                    kind: ValueKind::String,
                },
                ValueDecl {
                    name: "writtenFiles".into(),
                    kind: ValueKind::Array,
                },
            ],
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

/// The verdict the engine reached, or `None` when the claim's scenario never ran. A claim is
/// never in both: a run that did not happen supports nothing and contradicts nothing.
fn outcome(executed: &Executed) -> Option<Outcome> {
    match executed {
        Executed::Evaluated { outcome, .. } => Some(*outcome),
        Executed::RunFailed { .. } => None,
    }
}

/// The kind and the plugin's own words for a claim whose scenario did not run.
fn run_failed(executed: &Executed) -> (RunFailureKind, String) {
    match executed {
        Executed::RunFailed { kind, detail, .. } => (*kind, detail.clone()),
        Executed::Evaluated { claim_id, .. } => {
            panic!("{claim_id} reached a verdict, and the test expected its scenario to fail")
        }
    }
}

fn observed(executed: &Executed) -> Vec<String> {
    match executed {
        Executed::Evaluated { observed, .. } => observed.clone(),
        Executed::RunFailed { .. } => Vec::new(),
    }
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
        &[scenario_claim("c8223c485f101870d", exit_code(json!(2)))],
        30_000,
        |req, host| client.run(req, host),
    );
    assert_eq!(outcome(&executed[0]), Some(Outcome::Supported));
    assert!(observed(&executed[0]).is_empty());
}

#[test]
fn a_failing_tree_contradicts_with_values_and_both_readings() {
    let client = client(json!({}));
    let executed = execute::execute(
        &[scenario_claim("c8223c485f101870d", exit_code(json!(0)))],
        30_000,
        |req, host| client.run(req, host),
    );
    assert_eq!(outcome(&executed[0]), Some(Outcome::Contradicted));
    assert_eq!(observed(&executed[0]), vec!["exit.code = 2".to_string()]);
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
    let executed = execute::execute(
        &[scenario_claim("c8223c485f101870d", tree)],
        30_000,
        |req, host| client.run(req, host),
    );
    assert_eq!(outcome(&executed[0]), Some(Outcome::Supported));
}

#[test]
fn a_refused_scenario_is_that_claims_result_not_the_builds() {
    let client = client(json!({
        "run": {
            "ok": false,
            "observed": [],
            "satisfied": [],
            "failure": { "kind": "fixture", "detail": "fixture blew up" }
        }
    }));
    let executed = execute::execute(
        &[scenario_claim("c8223c485f101870d", exit_code(json!(2)))],
        30_000,
        |req, host| client.run(req, host),
    );
    // A scenario that cannot run is this claim's result, and nothing stronger: the claim is in
    // no outcome list, because a run that did not happen proves nothing either way.
    assert_eq!(outcome(&executed[0]), None);
    assert_eq!(
        run_failed(&executed[0]),
        (RunFailureKind::Fixture, "fixture blew up".into())
    );
}

#[test]
fn a_scripted_deadline_is_that_claims_result_not_the_builds() {
    let client = client(json!({ "_sleepMs": { "run": 500 } }));
    let executed = execute::execute(
        &[scenario_claim("c8223c485f101870d", exit_code(json!(2)))],
        50,
        |req, host| client.run(req, host),
    );
    assert_eq!(
        run_failed(&executed[0]),
        (
            RunFailureKind::Deadline,
            "claim c8223c485f101870d outlived 50 ms".into()
        )
    );
}

#[test]
fn a_satisfies_leaf_the_plugin_skipped_is_a_run_failed_result_not_a_false() {
    let client = client(json!({})); // the default run answers the claim's first leaf, no other
    let tree = Asserts::All {
        all: vec![
            exit_code(json!(2)),
            Asserts::Leaf(AssertLeaf {
                obs: "writtenFiles".into(),
                r#is: Check::Satisfies(veridoc_protocol::SatisfiesSpec {
                    source: "f => f.length > 0".into(),
                    accepts: Some(json!(["a"])),
                    rejects: Some(json!([])),
                }),
            }),
        ],
    };
    let executed = execute::execute(
        &[scenario_claim("c8223c485f101870d", tree)],
        30_000,
        |req, host| client.run(req, host),
    );
    assert_eq!(
        run_failed(&executed[0]),
        (
            RunFailureKind::Plugin,
            "the plugin answered no result for leaf c8223c485f101870d#l1".into()
        )
    );
}

/// Decision 0006: the unit of work is the claim. A scenario that cannot run is one claim's
/// result, and the claims beside it in the same project keep their verdicts.
#[test]
fn a_claim_whose_scenario_cannot_run_does_not_stop_its_siblings() {
    let client = client(json!({})); // answers the first leaf of whatever claim it is asked for, no other
    let unrunnable = scenario_claim(
        "a0e50f8d7c1b4932",
        Asserts::All {
            all: vec![
                exit_code(json!(2)),
                Asserts::Leaf(AssertLeaf {
                    obs: "writtenFiles".into(),
                    r#is: Check::Satisfies(veridoc_protocol::SatisfiesSpec {
                        source: "f => f.length > 0".into(),
                        accepts: Some(json!(["a"])),
                        rejects: Some(json!([])),
                    }),
                }),
            ],
        },
    );
    let judged = scenario_claim("c8223c485f101870d", exit_code(json!(2)));
    let executed = execute::execute(&[unrunnable, judged], 30_000, |req, host| {
        client.run(req, host)
    });
    assert_eq!(executed.len(), 2);
    assert_eq!(
        run_failed(&executed[0]),
        (
            RunFailureKind::Plugin,
            "the plugin answered no result for leaf a0e50f8d7c1b4932#l1".into()
        )
    );
    assert_eq!(outcome(&executed[1]), Some(Outcome::Supported));
}
