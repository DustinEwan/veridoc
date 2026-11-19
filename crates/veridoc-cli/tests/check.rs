//! The built `veridoc` binary against fixture projects: per blocking rule, the exit code,
//! the exact diagnostic line, and the absence of `.veridoc/result.json`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};
use veridoc_testprovider::{self as provider, Handle};

fn tmp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("veridoc-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn fakeplugin() -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    let start = exe
        .parent()
        .expect("test binary in a directory")
        .to_path_buf();
    if let Some(found) = find_fakeplugin(&start) {
        return found;
    }
    let status = Command::new(env!("CARGO"))
        .args(["build", "-p", "veridoc-fakeplugin"])
        .status()
        .expect("build the fake plugin");
    assert!(status.success());
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

const SPEC: &str = "<doc-section name=\"probe\">\n\
    \n\
    ## What the scanner refuses\n\
    \n\
    The scanner walks upward and stops at the first project root <doc-claim name=\"rejects-missing-project-root\"/>.\n";

/// A conforming project whose plugin is the fake, optionally scripted, with the answering
/// model judge. Returns the project root and the mock provider its `judge.baseUrl` points at.
fn scaffold(name: &str, script: Option<Value>) -> (PathBuf, Handle) {
    scaffold_with(name, script, json!({}))
}

fn scaffold_with(name: &str, script: Option<Value>, config_extra: Value) -> (PathBuf, Handle) {
    // `openai` reads a credential from the environment and this provider needs no real one;
    // the binary every test spawns inherits this process's environment.
    static KEY: std::sync::Once = std::sync::Once::new();
    KEY.call_once(|| std::env::set_var("VERIDOC_TEST_JUDGE_KEY", "test"));
    let judge = provider::answering(provider::Verdicts::default());
    let root = tmp_dir(name);
    std::fs::create_dir_all(root.join(".veridoc")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/architecture.spec.md"), SPEC).unwrap();

    let plugin = match script {
        None => fakeplugin().to_string_lossy().into_owned(),
        Some(script) => {
            let wrapper = root.join("plugin.sh");
            std::fs::write(
                &wrapper,
                format!(
                    "#!/bin/sh\nexport VERIDOC_FAKE_SCRIPT='{}'\nexec {}\n",
                    script.to_string().replace('\'', r"'\''"),
                    fakeplugin().display()
                ),
            )
            .unwrap();
            set_executable(&wrapper);
            wrapper.to_string_lossy().into_owned()
        }
    };

    let mut config = json!({
        "plugin": { "command": [plugin] },
        "judge": {
            "provider": "openai",
            "model": "scripted",
            "baseUrl": judge.base_url(),
            "apiKeyEnv": "VERIDOC_TEST_JUDGE_KEY",
            "timeoutMs": 10000,
            "maxTokens": 1000,
        },
        "output": { "dir": "out", "formatters": [] },
    });
    for (key, value) in config_extra.as_object().into_iter().flatten() {
        config[key] = value.clone();
    }
    std::fs::write(
        root.join(".veridoc/config.json"),
        serde_json::to_string_pretty(&config).unwrap(),
    )
    .unwrap();
    (root, judge)
}

fn veridoc(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_veridoc"))
        .args(args)
        .output()
        .expect("run veridoc");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn a_conforming_project_exits_zero_and_writes_the_result() {
    let (root, judge) = scaffold("green", None);
    let (code, stdout, stderr) = veridoc(&["check", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 0, "stdout: {stdout}  stderr: {stderr}");
    assert!(root.join(".veridoc/result.json").is_file());
    // The surface capability reports the uncovered public symbol; a warning with no
    // file is the message alone on stderr.
    assert!(
        stderr.contains("warning: symbol helper is public and no claim covers it"),
        "stderr: {stderr}"
    );
    assert!(!stdout.contains("warning:"), "stdout: {stdout}");
    // The run asked the model. A verdict replayed from `.veridoc/verdicts` would also exit
    // zero, so the exit code alone does not prove the judge was consulted.
    assert!(judge.calls() > 0, "the provider received no request");
}

#[test]
fn an_undeclared_spec_file_is_the_exact_line_and_no_result() {
    let (root, _judge) = scaffold("undeclared", None);
    std::fs::write(
        root.join("docs/undeclared.spec.md"),
        "<doc-section name=\"other\">\n\nx\n",
    )
    .unwrap();
    let (code, stdout, _stderr) = veridoc(&["check", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 1);
    assert!(
        stdout.lines().any(|l| l
            == "docs/undeclared.spec.md:1: no document node in the project's document tree declares this file"),
        "stdout: {stdout}"
    );
    assert!(
        !root.join(".veridoc/result.json").is_file(),
        "a run with findings writes no document"
    );
}

#[test]
fn a_contradicted_claim_names_its_values_and_both_readings() {
    let (root, _judge) = scaffold(
        "contradicted",
        Some(json!({
            "run": {
                "ok": true,
                "observed": [{ "observable": "exit.code", "value": "1" }],
                "satisfied": []
            }
        })),
    );
    let (code, stdout, _stderr) = veridoc(&["check", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 1);
    let line = stdout
        .lines()
        .find(|l| l.contains("is contradicted"))
        .unwrap_or_else(|| panic!("stdout: {stdout}"));
    assert!(line.contains("exit.code = 1"), "{line}");
    assert!(
        line.contains("the code is wrong and the sentence is right"),
        "{line}"
    );
    assert!(
        line.contains("the scenario or the assertion is wrong and the code is right"),
        "{line}"
    );
    assert!(!root.join(".veridoc/result.json").is_file());
}

#[test]
fn a_fixture_failure_is_that_claims_result_and_the_build_finishes() {
    let (root, _judge) = scaffold(
        "fixture",
        Some(json!({
            "run": {
                "ok": false, "observed": [], "satisfied": [],
                "failure": { "kind": "fixture", "detail": "fixture blew up" }
            }
        })),
    );
    let (code, stdout, _stderr) = veridoc(&["check", "--project", root.to_str().unwrap()]);
    // Decision 0006: a scenario that cannot run is one claim's `run-failed` result, not a run
    // failure. The build goes on, writes its document, and exits on its findings alone.
    assert_eq!(code, 0);
    assert!(
        stdout.contains(
            "claim probe#rejects-missing-project-root did not run: fixture: fixture blew up"
        ),
        "stdout: {stdout}"
    );
    let result =
        std::fs::read_to_string(root.join(".veridoc/result.json")).expect("the result document");
    assert!(result.contains("\"run-failed\""), "result: {result}");
}

#[test]
fn a_deadline_is_that_claims_result_and_the_build_finishes() {
    let (root, _judge) = scaffold_with(
        "deadline",
        Some(json!({ "_sleepMs": { "run": 800 } })),
        json!({ "run": { "scenarioDeadlineMs": 50 } }),
    );
    let (code, stdout, _stderr) = veridoc(&["check", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains(
            "claim probe#rejects-missing-project-root did not run: deadline: \
             claim probe#rejects-missing-project-root outlived 50 ms"
        ),
        "stdout: {stdout}"
    );
}

/// Repoints the judge at `endpoint`, keeping the rest of the config: the way a transport
/// fault is simulated now that the judge is a model the engine calls.
fn break_judge(root: &Path, endpoint: &str) {
    let config_path = root.join(".veridoc/config.json");
    let mut config: Value =
        serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
    config["judge"]["baseUrl"] = json!(endpoint);
    std::fs::write(&config_path, serde_json::to_string(&config).unwrap()).unwrap();
}

#[test]
fn a_judge_that_cannot_be_reached_is_a_run_failure_naming_the_endpoint() {
    let (root, _judge) = scaffold("judge-unreachable", None);
    break_judge(&root, "http://127.0.0.1:1");
    let (code, _stdout, stderr) = veridoc(&["check", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert!(
        stderr.contains("run failed: judge: judge unreachable:"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("127.0.0.1:1"), "stderr: {stderr}");
    assert!(!root.join(".veridoc/result.json").is_file());
}

#[test]
fn a_provider_that_answers_badly_is_a_run_failure_with_no_outcomes() {
    let (root, _judge) = scaffold("judge-refusal", None);
    // The handle is held until the child has finished: the listener answers its requests.
    let refusing = provider::failing(500, r#"{"error":{"message":"boom"}}"#);
    break_judge(&root, refusing.base_url());
    let (code, stdout, stderr) = veridoc(&["check", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert!(
        stderr.contains("run failed: judge: judge unreachable:"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("boom"), "stderr: {stderr}");
    // A run that cannot judge gives no claim an outcome, passing or contradicted.
    assert!(!stdout.contains("contradicted"), "stdout: {stdout}");
    assert!(!root.join(".veridoc/result.json").is_file());
}

#[test]
fn an_unspawnable_plugin_is_a_run_failure() {
    let (root, _judge) = scaffold_with(
        "unspawnable",
        None,
        json!({ "plugin": { "command": ["/nonexistent/veridoc-plugin"] } }),
    );
    let (code, _stdout, stderr) = veridoc(&["check", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert!(stderr.contains("run failed: plugin:"), "stderr: {stderr}");
}

#[test]
fn a_bad_configuration_is_a_usage_error() {
    let root = tmp_dir("badconfig");
    std::fs::create_dir_all(root.join(".veridoc")).unwrap();
    std::fs::write(root.join(".veridoc/config.json"), "{ not json").unwrap();
    let (code, _stdout, stderr) = veridoc(&["check", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 3);
    assert!(stderr.starts_with("veridoc: "), "stderr: {stderr}");
}

/// The default claim's name: the author's subject `probe` joined with the predicate slug.
fn claim_id_of(_root: &Path) -> String {
    // The name is the author's subject joined with the predicate slug by the engine's own
    // rule; recomputing it here keeps the test honest about what `witness` matches.
    veridoc_engine::hash::claim_name("probe", "rejects-missing-project-root")
}

fn veridoc_engine_hex64(bytes: &[u8]) -> String {
    veridoc_engine::hash::hex64(bytes)
}

#[cfg(unix)]
fn set_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) {}

#[test]
fn witness_prints_the_current_hash_and_nothing_else() {
    let mut collect = veridoc_fakeplugin::default_collect(&serde_json::Value::Null);
    let digest = veridoc_engine_hex64(b"transcript bytes");
    collect["claims"][0]["kind"] = json!("witness");
    collect["claims"][0]["channel"] = json!({
        "artefact": { "path": "evidence.bin", "sha256": digest }
    });
    let (root, _judge) = scaffold("witness", Some(json!({ "collect": collect })));
    std::fs::write(root.join("evidence.bin"), b"transcript bytes").unwrap();

    let claim_id = claim_id_of(&root);
    let (code, stdout, stderr) =
        veridoc(&["witness", &claim_id, "--project", root.to_str().unwrap()]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout.trim(), digest);
}

#[test]
fn probe_plugin_prints_the_claim_and_referent_tables() {
    let (root, _judge) = scaffold("probe", None);
    let (code, stdout, stderr) = veridoc(&["probe-plugin", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(
        stdout.contains("plugin veridoc-fakeplugin 0.1.0"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("claims:"), "stdout: {stdout}");
    assert!(stdout.contains("referents:"), "stdout: {stdout}");
    assert!(
        stdout.contains("veridoc:referent:probe"),
        "stdout: {stdout}"
    );
}

#[test]
fn validate_plugin_validates_a_checkout_with_an_empty_registry() {
    let dir = tmp_dir("validate-checkout");
    let exe = fakeplugin();
    let exe_name = exe.file_name().unwrap().to_string_lossy().into_owned();
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    std::fs::copy(&exe, dir.join("bin").join(&exe_name)).unwrap();
    std::fs::write(
        dir.join("veridoc-plugin.json"),
        format!(
            "{{\"name\": \"veridoc-fakeplugin\", \"install\": [], \"build\": [], \
             \"command\": [\"bin/{exe_name}\"], \
             \"conformance\": {{\"emitter\": [\"bin/{exe_name}\", \"--emit-conformance\"]}}}}"
        ),
    )
    .unwrap();
    // An empty registry: the checkout is everything the command is given.
    let output = Command::new(env!("CARGO_BIN_EXE_veridoc"))
        .args(["validate-plugin", &dir.to_string_lossy()])
        .env("VERIDOC_HOME", dir.join("empty-registry"))
        .output()
        .expect("run veridoc");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        output.status.success(),
        "exit {:?}\nstdout: {stdout}\nstderr: {stderr}",
        output.status.code()
    );
    assert!(stdout.contains("12 rules passed"), "stdout: {stdout}");
}

#[test]
fn validate_plugin_names_both_alternatives_when_the_argument_is_neither() {
    let dir = tmp_dir("validate-nothing");
    let (code, _stdout, stderr) = veridoc(&["validate-plugin", dir.to_str().unwrap()]);
    assert_eq!(code, 3);
    assert!(
        stderr.contains("no veridoc-plugin.json in")
            && stderr.contains("or pass an installed plugin name"),
        "stderr: {stderr}"
    );
}

/// The opt-out a project takes when it names no `judge`: the deterministic stage still gates the
/// build, and the semantic stage is reported as not run rather than counted as a residue of zero.
/// This is the case a scripted judge used to hide, and it needs no model to test.
#[test]
fn a_project_with_no_judge_reports_the_semantic_stage_as_not_run() {
    let (root, _judge) = scaffold("no-judge", None);
    let config = root.join(".veridoc/config.json");
    let mut value: Value =
        serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("judge");
    std::fs::write(&config, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    let (code, stdout, stderr) = veridoc(&["check", "--project", root.to_str().unwrap()]);
    let printed = format!("{stdout}{stderr}");
    assert_eq!(code, 0, "exit {code}\n{printed}");
    assert!(
        printed.contains("semantic stage not run: no judge is configured in .veridoc/config.json"),
        "printed: {printed}"
    );
    assert!(
        printed.contains("semantic stage not run: no judge is configured, so prose is unchecked"),
        "printed: {printed}"
    );
}

/// `veridoc stat` is the gate for a job that cannot call a model: it compares the checkout
/// against the fingerprint the last run recorded, starting no plugin and asking no model.
#[test]
fn stat_after_a_run_reports_the_record_current() {
    let (root, _judge) = scaffold("stat-current", None);
    let (code, _stdout, stderr) = veridoc(&["check", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 0, "stderr: {stderr}");
    let (code, stdout, stderr) = veridoc(&["stat", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(
        stdout.contains("record current"),
        "stdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn stat_after_an_edit_names_the_changed_file() {
    let (root, _judge) = scaffold("stat-stale", None);
    let (code, _stdout, stderr) = veridoc(&["check", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 0, "stderr: {stderr}");
    std::fs::write(
        root.join("docs/architecture.spec.md"),
        format!("{}\nA sentence the record has never seen.\n", SPEC),
    )
    .unwrap();
    let (code, stdout, stderr) = veridoc(&["stat", "--project", root.to_str().unwrap()]);
    let printed = format!("{stdout}{stderr}");
    assert_eq!(code, 1, "exit {code}\n{printed}");
    assert!(
        printed.contains("changed  docs/architecture.spec.md"),
        "printed: {printed}"
    );
    assert!(printed.contains("record stale"), "printed: {printed}");
}

#[test]
fn stat_without_a_record_names_the_run_it_needs() {
    let (root, _judge) = scaffold("stat-none", None);
    let (code, _stdout, stderr) = veridoc(&["stat", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 3, "stderr: {stderr}");
    assert!(
        stderr.contains("no .veridoc/inputs.json in") && stderr.contains("veridoc check"),
        "stderr: {stderr}"
    );
}

/// The case a CI job is in: the checkout and the committed record are there, the plugin is not.
/// `stat` compares hashes and launches nothing, so a `plugin.name` the registry does not hold is not
/// its failure to report.
#[test]
fn stat_needs_the_record_and_not_an_installed_plugin() {
    let (root, _judge) = scaffold("stat-no-plugin", None);
    let (code, _stdout, stderr) = veridoc(&["check", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 0, "stderr: {stderr}");
    let config = root.join(".veridoc/config.json");
    let mut value: Value =
        serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
    value["plugin"] = json!({ "name": "veridoc-plugin-not-installed" });
    std::fs::write(&config, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    let (code, stdout, stderr) = veridoc(&["stat", "--project", root.to_str().unwrap()]);
    let printed = format!("{stdout}{stderr}");
    assert_ne!(
        code, 3,
        "stat refused to read a configuration whose plugin is not installed: {printed}"
    );
    assert!(
        !printed.contains("install-plugin"),
        "stat asked for a plugin it never starts: {printed}"
    );
    assert!(
        printed.contains("changed  .veridoc/config.json") && printed.contains("record stale"),
        "the edit to the configuration is a changed input the record has to name: {printed}"
    );
}

/// A project that renders its documentation into a tracked directory is still gated by the record:
/// the pages the run wrote are its own product, not inputs it never read. Without that exclusion the
/// next `stat` would find them added and report the record stale forever.
#[test]
fn stat_ignores_the_documentation_the_run_rendered() {
    let (root, _judge) = scaffold_with(
        "stat-rendered",
        None,
        json!({ "output": { "dir": "./website", "formatters": ["markdown"] } }),
    );
    let (code, _stdout, stderr) = veridoc(&["check", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(
        root.join("website/markdown/docs/architecture.md").is_file(),
        "the formatter writes into the directory the config names"
    );
    let (code, stdout, stderr) = veridoc(&["stat", "--project", root.to_str().unwrap()]);
    let printed = format!("{stdout}{stderr}");
    assert_eq!(code, 0, "exit {code}\n{printed}");
    assert!(
        printed.contains("record current"),
        "the rendered pages must not be inputs: {printed}"
    );
}

/// `veridoc init` names the plugin, writes the configuration, and lets the plugin create the files
/// only it can write. Flags answer every question, so the test needs no terminal.
#[test]
fn init_with_flags_writes_the_configuration_and_the_plugins_files() {
    let root = tmp_dir("init-flags");
    let (code, stdout, stderr) = veridoc(&[
        "init",
        "--project",
        root.to_str().unwrap(),
        "--plugin-command",
        fakeplugin().to_str().unwrap(),
        "--document",
        "docs/architecture.spec.md",
        "--audience",
        "an author wiring a plugin",
        "--question",
        "What must my plugin answer?",
    ]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(root.join(".veridoc/config.json").is_file());
    assert!(root.join(".veridoc/.gitignore").is_file());
    assert!(root.join("index.doc.ts").is_file());
    assert!(root.join("docs/architecture.spec.md").is_file());
    let written: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join(".veridoc/config.json")).unwrap())
            .unwrap();
    assert_eq!(
        written["plugin"]["command"],
        json!([fakeplugin().display().to_string()]),
        "the configuration names the plugin the answers came from"
    );
    for line in stdout.lines() {
        assert!(
            line.starts_with("wrote "),
            "stdout carries paths only: {line}"
        );
    }
    assert!(stderr.contains("next:"), "stderr: {stderr}");
}

#[test]
fn init_without_a_terminal_names_the_flag_it_cannot_ask() {
    let root = tmp_dir("init-asked");
    let (code, stdout, stderr) = veridoc(&[
        "init",
        "--project",
        root.to_str().unwrap(),
        "--plugin-command",
        fakeplugin().to_str().unwrap(),
    ]);
    assert_eq!(code, 3, "stderr: {stderr}");
    assert!(
        stderr.contains("`--audience` is required"),
        "stderr: {stderr}"
    );
    assert!(stdout.is_empty(), "stdout: {stdout}");
    assert!(
        !root.join(".veridoc/config.json").exists(),
        "a run that cannot answer its questions writes nothing"
    );
}

#[test]
fn init_writes_nothing_when_the_plugin_refuses() {
    let root = tmp_dir("init-refused");
    std::fs::write(root.join("index.doc.ts"), "the author's own tree\n").unwrap();
    let (code, _stdout, stderr) = veridoc(&[
        "init",
        "--project",
        root.to_str().unwrap(),
        "--plugin-command",
        fakeplugin().to_str().unwrap(),
        "--document",
        "docs/architecture.spec.md",
        "--audience",
        "an author wiring a plugin",
        "--question",
        "What must my plugin answer?",
    ]);
    assert_eq!(code, 3, "stderr: {stderr}");
    assert!(
        stderr.contains("index.doc.ts exists"),
        "the plugin's own reason is what the author reads: {stderr}"
    );
    assert!(
        !root.join(".veridoc/config.json").exists(),
        "a configuration with no root document is a half-written scaffold"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("index.doc.ts")).unwrap(),
        "the author's own tree\n"
    );
}
