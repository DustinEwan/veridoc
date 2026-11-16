//! The built `veridoc` binary against fixture projects: per blocking rule, the exit code,
//! the exact diagnostic line, and the absence of `.veridoc/result.json`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

fn tmp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vd-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn fakeplugin() -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    let start = exe.parent().expect("test binary in a directory").to_path_buf();
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

const JUDGE: &str = "#!/bin/sh\n\
    IFS= read -r req\n\
    case \"$req\" in\n\
      *'\"task\":\"s3\"'*) printf '%s\\n' '{\"pairs\": []}';;\n\
      *'\"task\":\"s2\"'*) printf '%s\\n' '{\"pairs\": []}';;\n\
      *) printf '%s\\n' '{\"unmentionedClaims\": [], \"unstatedAssertions\": []}';;\n\
    esac\n";

/// A conforming project whose plugin is the fake, optionally scripted, with the answering
/// command judge. Returns the project root.
fn scaffold(name: &str, script: Option<Value>) -> PathBuf {
    scaffold_with(name, script, json!({}))
}

fn scaffold_with(name: &str, script: Option<Value>, config_extra: Value) -> PathBuf {
    let root = tmp_dir(name);
    std::fs::create_dir_all(root.join(".veridoc")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::create_dir_all(root.join("features")).unwrap();
    std::fs::write(root.join("docs/architecture.spec.md"), SPEC).unwrap();
    std::fs::write(root.join("features/cli.feature"), FEATURE).unwrap();

    let judge = root.join("judge.sh");
    std::fs::write(&judge, JUDGE).unwrap();
    set_executable(&judge);

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
        "judge": { "transport": "command", "model": "scripted", "command": ["sh", judge.to_string_lossy()] },
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
    root
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
    let root = scaffold("green", None);
    let (code, stdout, stderr) =
        veridoc(&["check", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 0, "stdout: {stdout}  stderr: {stderr}");
    assert!(root.join(".veridoc/result.json").is_file());
    // The surface capability reports the uncovered public symbol; a warning with no
    // file is the message alone on stderr.
    assert!(
        stderr.contains("warning: symbol helper is public and no claim covers it"),
        "stderr: {stderr}"
    );
    assert!(!stdout.contains("warning:"), "stdout: {stdout}");
}

#[test]
fn an_undeclared_spec_file_is_the_exact_line_and_no_result() {
    let root = scaffold("undeclared", None);
    std::fs::write(root.join("docs/undeclared.spec.md"), "# U\n\n## S {#s}\n\nx\n").unwrap();
    let (code, stdout, _stderr) =
        veridoc(&["check", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 1);
    assert!(
        stdout
            .lines()
            .any(|l| l == "docs/undeclared.spec.md:1: no document node in index.doc.ts declares this file"),
        "stdout: {stdout}"
    );
    assert!(
        !root.join(".veridoc/result.json").is_file(),
        "a run with findings writes no document"
    );
}

#[test]
fn a_contradicted_claim_names_its_values_and_both_readings() {
    let root = scaffold(
        "contradicted",
        Some(json!({
            "run": {
                "ok": true,
                "observed": [{ "observable": "exit.code", "value": "1" }],
                "satisfied": []
            }
        })),
    );
    let (code, stdout, _stderr) =
        veridoc(&["check", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 1);
    let line = stdout
        .lines()
        .find(|l| l.contains("is contradicted"))
        .unwrap_or_else(|| panic!("stdout: {stdout}"));
    assert!(line.contains("exit.code = 1"), "{line}");
    assert!(line.contains("the code is wrong and the sentence is right"), "{line}");
    assert!(line.contains("the scenario or the assertion is wrong and the code is right"), "{line}");
    assert!(!root.join(".veridoc/result.json").is_file());
}

#[test]
fn a_fixture_failure_is_a_run_failure_with_the_plugins_kind() {
    let root = scaffold(
        "fixture",
        Some(json!({
            "run": {
                "ok": false, "observed": [], "satisfied": [],
                "failure": { "kind": "fixture", "detail": "fixture blew up" }
            }
        })),
    );
    let (code, _stdout, stderr) =
        veridoc(&["check", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert!(
        stderr.contains("run failed: fixture: scenario config/rejects-missing-root: fixture blew up"),
        "stderr: {stderr}"
    );
    assert!(!root.join(".veridoc/result.json").is_file());
}

#[test]
fn a_deadline_is_a_run_failure_of_kind_deadline() {
    let root = scaffold_with(
        "deadline",
        Some(json!({ "_sleepMs": { "run": 800 } })),
        json!({ "run": { "scenarioDeadlineMs": 50 } }),
    );
    let (code, _stdout, stderr) =
        veridoc(&["check", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert!(
        stderr.contains(
            "run failed: deadline: scenario config/rejects-missing-root outlived 50 ms"
        ),
        "stderr: {stderr}"
    );
}

/// Replaces the answering judge with a broken one, keeping the rest of the config.
fn break_judge(root: &Path, name: &str, script: &str) {
    let bad = root.join(format!("{name}.sh"));
    std::fs::write(&bad, script).unwrap();
    set_executable(&bad);
    let config_path = root.join(".veridoc/config.json");
    let mut config: Value =
        serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
    config["judge"]["command"] = json!(["sh", bad.to_string_lossy()]);
    std::fs::write(&config_path, serde_json::to_string(&config).unwrap()).unwrap();
}

#[test]
fn a_judge_that_exits_nonzero_is_a_run_failure_naming_the_command() {
    let root = scaffold("judge-exit", None);
    break_judge(&root, "bad-judge", "#!/bin/sh\necho judge offline >&2\nexit 3\n");
    let (code, _stdout, stderr) =
        veridoc(&["check", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert!(stderr.contains("run failed: judge:"), "stderr: {stderr}");
    assert!(stderr.contains("judge offline"), "stderr: {stderr}");
    assert!(!root.join(".veridoc/result.json").is_file());
}

#[test]
fn a_malformed_judge_reply_is_a_run_failure_with_no_outcomes() {
    let root = scaffold("judge-garbage", None);
    break_judge(
        &root,
        "garbage-judge",
        "#!/bin/sh\nIFS= read -r req\necho 'not json'\n",
    );
    let (code, stdout, stderr) =
        veridoc(&["check", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert!(stderr.contains("run failed: judge:"), "stderr: {stderr}");
    // A run that cannot judge gives no claim an outcome, passing or contradicted.
    assert!(!stdout.contains("contradicted"), "stdout: {stdout}");
    assert!(!root.join(".veridoc/result.json").is_file());
}

#[test]
fn an_unspawnable_plugin_is_a_run_failure() {
    let root = scaffold_with(
        "unspawnable",
        None,
        json!({ "plugin": { "command": ["/nonexistent/veridoc-plugin"] } }),
    );
    let (code, _stdout, stderr) =
        veridoc(&["check", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert!(stderr.contains("run failed: plugin:"), "stderr: {stderr}");
}

#[test]
fn a_bad_configuration_is_a_usage_error() {
    let root = tmp_dir("badconfig");
    std::fs::create_dir_all(root.join(".veridoc")).unwrap();
    std::fs::write(root.join(".veridoc/config.json"), "{ not json").unwrap();
    let (code, _stdout, stderr) =
        veridoc(&["check", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 3);
    assert!(stderr.starts_with("veridoc: "), "stderr: {stderr}");
}

/// The default claim's id: the fake resolves `src/cli.ts|probe` to `vd:r:probe`.
fn claim_id_of(_root: &Path) -> String {
    // Identity is the resolved referent joined with the predicate, hashed by the engine's
    // own rule; recomputing it here keeps the test honest about what `witness` matches.
    veridoc_engine::hash::claim_id("vd:r:probe", "rejects-missing-project-root")
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
    let root = scaffold("witness", Some(json!({ "collect": collect })));
    std::fs::write(root.join("evidence.bin"), b"transcript bytes").unwrap();

    let claim_id = claim_id_of(&root);
    let (code, stdout, stderr) =
        veridoc(&["witness", &claim_id, "--project", root.to_str().unwrap()]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout.trim(), digest);
}

#[test]
fn probe_plugin_prints_the_claim_and_referent_tables() {
    let root = scaffold("probe", None);
    let (code, stdout, stderr) =
        veridoc(&["probe-plugin", "--project", root.to_str().unwrap()]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("plugin veridoc-fakeplugin 0.1.0"), "stdout: {stdout}");
    assert!(stdout.contains("claims:"), "stdout: {stdout}");
    assert!(stdout.contains("referents:"), "stdout: {stdout}");
    assert!(stdout.contains("vd:r:probe"), "stdout: {stdout}");
}
