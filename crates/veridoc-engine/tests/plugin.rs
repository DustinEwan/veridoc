//! The plugin client's supervision contract, driven against `veridoc-fakeplugin`: the
//! protocol-version refusal, a killed process, a response with the wrong id, a non-JSON line,
//! and the capability report. Every loss is a run failure; none is a claim state.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use veridoc_engine::plugin::{ConnectError, Launch, PluginClient};
use veridoc_protocol::{Call, Loss, UNUSABLE_RESULT};

/// `CARGO_BIN_EXE_…` cannot cross packages, so walk every ancestor of the test binary to
/// find the sibling `veridoc-fakeplugin`. `cargo test --workspace` builds it; a bare
/// `cargo test -p veridoc-engine` does not, so build it on demand.
fn fakeplugin() -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    let start = exe.parent().expect("test binary in a directory").to_path_buf();
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

fn launch(script: Option<serde_json::Value>) -> Launch {
    let mut env = BTreeMap::new();
    if let Some(script) = script {
        env.insert(
            "VERIDOC_FAKE_SCRIPT".to_string(),
            script.to_string(),
        );
    }
    Launch {
        argv: vec![fakeplugin().display().to_string()],
        env,
        cwd: None,
    }
}

#[test]
fn connect_handshakes_and_reports_the_declared_capabilities() {
    let (client, handshake) = PluginClient::connect(&launch(None), Duration::from_secs(30))
        .expect("the default script conforms");
    assert_eq!(handshake.name, "veridoc-fakeplugin");
    assert_eq!(handshake.protocol_version, "vd/1");
    assert_eq!(handshake.capabilities.len(), 7);
    let collected: veridoc_protocol::CollectResult =
        client.collect("/tmp", Duration::from_secs(30)).payload().unwrap();
    assert_eq!(collected.claims.len(), 1);
    assert_eq!(collected.observables.len(), 2);
    let resolved = client.resolve(&["src/cli.ts|probe".into()]).payload().unwrap();
    assert_eq!(resolved.resolved[0].referent_id, "vd:r:probe");
    let run = client
        .run(&veridoc_protocol::RunRequest {
            scenario_id: "config/rejects-missing-root".into(),
            fixture: "empty-directory".into(),
            action: "run-probe".into(),
            args: vec![],
            deadline_ms: 5_000,
            leaves: vec![veridoc_protocol::RunLeaf {
                leaf_id: "c000000000000001#l0".into(),
            }],
        })
        .payload()
        .unwrap();
    assert!(run.ok);
    assert_eq!(run.observed[0].value, "2");
    assert!(run.satisfied[0].ok);
}

#[test]
fn a_wrong_protocol_revision_is_refused_at_connect() {
    let mut hs = veridoc_fakeplugin::default_handshake();
    hs["protocolVersion"] = serde_json::json!("vd/2");
    let err = PluginClient::connect(
        &launch(Some(serde_json::json!({ "handshake": hs }))),
        Duration::from_secs(30),
    )
    .unwrap_err();
    assert_eq!(
        err,
        ConnectError::UnsupportedVersion {
            reported: "vd/2".to_string()
        }
    );
}

#[test]
fn a_handshake_refusal_is_refused_at_connect_and_not_called_a_loss() {
    // The plugin is up and answering; it only refuses to be a veridoc plugin. Calling that a
    // lost connection would send an operator to look for a crash that never happened.
    let frame = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "error": { "code": -32000, "message": "not a veridoc plugin" }
    });
    let err = PluginClient::connect(
        &launch(Some(serde_json::json!({ "_raw": { "handshake": frame } }))),
        Duration::from_secs(30),
    )
    .unwrap_err();
    assert_eq!(
        err,
        ConnectError::Refused {
            code: -32000,
            message: "not a veridoc plugin".to_string()
        }
    );
    assert_eq!(
        err.to_string(),
        "plugin refused the handshake (-32000): not a veridoc plugin",
        "what the operator reads names a refusal, not a crash"
    );
}

#[test]
fn a_killed_plugin_resolves_in_flight_and_later_calls_as_killed() {
    let (client, _hs) = PluginClient::connect(&launch(None), Duration::from_secs(30)).unwrap();
    client.kill();
    let call = client.collect("/tmp", Duration::from_secs(5));
    assert_eq!(call.loss(), Some(Loss::Killed));
}

#[test]
fn a_response_with_an_unknown_id_is_discarded_and_the_request_times_out() {
    // The fake answers `handshake` with `id: 0`, which matches no in-flight request.
    let raw = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 0,
        "result": veridoc_fakeplugin::default_handshake()
    });
    let client = PluginClient::spawn(
        &launch(Some(serde_json::json!({ "_raw": { "handshake": raw } }))),
    )
    .unwrap();
    let call = client.handshake(Duration::from_millis(300));
    assert_eq!(call.loss(), Some(Loss::NoAnswer));
}

#[test]
fn an_unparseable_line_is_dropped_and_the_request_times_out() {
    let client = PluginClient::spawn(
        &launch(Some(serde_json::json!({ "_malformed": ["collect"] }))),
    )
    .unwrap();
    let call = client.collect("/tmp", Duration::from_millis(300));
    assert_eq!(call.loss(), Some(Loss::NoAnswer));
}

#[test]
fn an_unreadable_answer_is_an_unusable_result_loss() {
    // A script whose `derivable` override is not the shape the answer must have is a result
    // the client cannot read: the mechanism files it as a loss with the reason
    // `unusable-result`, not as a plugin error and not as a lost process.
    let client = PluginClient::spawn(&launch(Some(
        serde_json::json!({ "derivable": "not an object" }),
    )))
    .unwrap();
    let call = client.derivable("vd:r:probe");
    match call {
        Call::Lost(Loss::Profile { reason, .. }) => {
            assert_eq!(reason, UNUSABLE_RESULT, "a malformed answer is an unusable-result loss");
        }
        other => panic!("expected an unusable-result loss, got {other:?}"),
    }
}

#[test]
fn a_plugin_error_frame_is_a_refusal() {
    // A plugin that answers a valid method with a JSON-RPC error frame has ANSWERED: the
    // process is alive, so the call is a refusal carrying the frame's own code and message,
    // never a loss. This is the case the old code misfiled as a lost process.
    let frame = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "error": { "code": -32000, "message": "store unreachable" }
    });
    let client = PluginClient::spawn(&launch(Some(
        serde_json::json!({ "_raw": { "derivable": frame } }),
    )))
    .unwrap();
    let error = client
        .derivable("vd:r:probe")
        .refusal()
        .expect("an error frame is a refusal, not a loss");
    assert_eq!(error.code, -32000);
    assert_eq!(error.message, "store unreachable");
}

#[test]
fn missing_capabilities_are_reported_never_assumed() {
    let mut hs = veridoc_fakeplugin::default_handshake();
    hs["capabilities"] = serde_json::json!(["resolution"]);
    let (_client, handshake) = PluginClient::connect(
        &launch(Some(serde_json::json!({ "handshake": hs }))),
        Duration::from_secs(30),
    )
    .unwrap();
    assert_eq!(handshake.capabilities, vec![veridoc_protocol::Capability::Resolution]);
}

#[test]
fn stderr_is_captured_for_the_run_failure_stream() {
    let client = PluginClient::spawn(&launch(Some(serde_json::json!({
        "_stderr": ["plugin said: store unreachable", "plugin said: store unreachable"]
    }))))
    .unwrap();
    // Give the drain a moment, then read what the plugin wrote.
    let mut lines = Vec::new();
    for _ in 0..100 {
        lines = veridoc_engine::plugin::plugin_said(
            &client.stderr_lines.lock().unwrap().clone(),
        );
        if !lines.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(lines, vec!["plugin said: store unreachable".to_string()], "distinct lines only");
}

#[test]
fn a_sleeping_run_outliving_its_deadline_is_a_no_answer() {
    let client = PluginClient::spawn(&launch(Some(serde_json::json!({
        "_sleepMs": { "run": 2_000 }
    }))))
    .unwrap();
    let call = client.run(&veridoc_protocol::RunRequest {
        scenario_id: "config/rejects-missing-root".into(),
        fixture: "empty-directory".into(),
        action: "run-probe".into(),
        args: vec![],
        deadline_ms: 100,
        leaves: vec![],
    });
    assert_eq!(call.loss(), Some(Loss::NoAnswer));
}

#[test]
fn an_empty_command_is_a_spawn_failure_naming_the_config_key() {
    let err = PluginClient::spawn(&Launch::new(vec![])).unwrap_err();
    assert_eq!(err, Loss::Spawn { detail: "empty plugin.command".to_string() });
}
