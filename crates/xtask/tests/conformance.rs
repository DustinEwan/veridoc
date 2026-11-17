//! The repository's own fake plugin must pass the conformance suite its own contract defines:
//! the emitter renders the fixture, the suite drives the binary. This is the gate CI runs
//! after `cargo build`, and it is what makes "the suite is not vacuous" checkable — a suite
//! that passes nothing proves nothing.

use std::path::{Path, PathBuf};
use std::process::Command;

use veridoc_conformance::Check;
use veridoc_engine::plugin::Launch;

/// `CARGO_BIN_EXE_…` cannot cross packages, so walk every ancestor of the test binary to
/// find the sibling `veridoc-fakeplugin`; build it on demand when absent.
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
    assert!(status.success(), "cargo build -p veridoc-fakeplugin failed");
    find_fakeplugin(&start).expect("veridoc-fakeplugin still absent after building it")
}

fn find_fakeplugin(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let candidate = dir.join("veridoc-fakeplugin");
        if candidate.is_file() {
            return Some(candidate);
        }
        if dir.join("Cargo.toml").is_file() && dir.join("crates").is_dir() {
            let candidate = dir.join("target").join("debug").join("veridoc-fakeplugin");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[test]
fn fakeplugin_passes_the_conformance_suite() {
    let exe = fakeplugin();
    let dir = std::env::temp_dir().join(format!("vd-conformance-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let rendered = Command::new(&exe)
        .args(["--emit-conformance"])
        .arg(&dir)
        .status()
        .expect("run the emitter");
    assert!(
        rendered.success(),
        "the emitter failed to render the fixture"
    );
    let report = veridoc_conformance::run(&Check {
        launch: Launch::new(vec![exe.to_string_lossy().into_owned()]),
        project: dir.clone(),
        deadline_ms: 20_000,
    })
    .expect("the emitted fixture is readable");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        report.claims, 6,
        "the fixture reports its scenario claim and five probes"
    );
    assert!(
        report.passed(),
        "the fake plugin violates its own conformance contract:\n{}",
        report.render()
    );
    assert!(
        report.not_applicable.is_empty(),
        "every rule applies to a plugin that declares all seven capabilities: {:?}",
        report.not_applicable
    );
}

/// Render the fixture and drive the suite once, with `VERIDOC_FAKE_SCRIPT` set for the plugin.
fn driven(exe: &Path, name: &str, script: &str) -> veridoc_conformance::Report {
    let dir = std::env::temp_dir().join(format!("vd-defect-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let rendered = Command::new(exe)
        .args(["--emit-conformance"])
        .arg(&dir)
        .status()
        .expect("run the emitter");
    assert!(
        rendered.success(),
        "the emitter failed to render the fixture"
    );
    let mut launch = Launch::new(vec![exe.to_string_lossy().into_owned()]);
    launch
        .env
        .insert("VERIDOC_FAKE_SCRIPT".to_string(), script.to_string());
    let report = veridoc_conformance::run(&Check {
        launch,
        project: dir.clone(),
        deadline_ms: 20_000,
    })
    .expect("the emitted fixture is readable");
    let _ = std::fs::remove_dir_all(&dir);
    report
}

/// The rules that reported a violation for one scripted defect, in the order the suite drives.
fn violated(exe: &Path, name: &str, script: &str) -> Vec<String> {
    driven(exe, name, script)
        .rules_failed()
        .into_iter()
        .map(String::from)
        .collect()
}

/// Each rule of `vd-1.profile.json` names a scripted defect and the exact set of rules that
/// defect must make report a violation. Running every one of them is what makes the spec's
/// `defect` fields checked rather than asserted (§12 item 4).
#[test]
fn every_scripted_defect_fires_exactly_the_rules_it_names() {
    let exe = fakeplugin();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vd-1.profile.json");
    let text = std::fs::read_to_string(&path).expect("the profile spec is readable");
    let spec: serde_json::Value = serde_json::from_str(&text).expect("the profile spec is json");
    let rules = spec["conformance"]["rules"]
        .as_array()
        .expect("the profile declares its own rules");
    assert_eq!(rules.len(), 7, "the profile declares seven rules");
    for rule in rules {
        let id = rule["id"].as_str().expect("every rule has an id");
        let scripted = rule["defect"]["env"]
            .as_array()
            .unwrap_or_else(|| panic!("`{id}` declares no scripted defect"));
        assert_eq!(scripted.len(), 1, "`{id}` scripts exactly one variable");
        let (_, script) = scripted[0]
            .as_str()
            .expect("a scripted variable is text")
            .split_once('=')
            .expect("a scripted variable is NAME=value");
        let expected: Vec<&str> = rule["defect"]["expectViolation"]
            .as_array()
            .expect("the defect names the rules it fires")
            .iter()
            .map(|target| target.as_str().expect("a rule id"))
            .collect();
        assert_eq!(
            violated(&exe, id, script),
            expected,
            "the scripted defect of `{id}` fires a different set of rules than the spec names"
        );
    }
}

/// A rule gated on a capability the plugin did not declare is not applicable with that gate
/// named, and is never counted as a pass (§12 item 3).
#[test]
fn a_capability_the_plugin_did_not_declare_gates_its_rule() {
    let exe = fakeplugin();
    let script = r#"{"handshake":{"protocolVersion":"vd/1","name":"veridoc-fakeplugin","version":"0.1.0","languages":["typescript"],"capabilities":["collect","resolution","derivability","bivalence","execution","citation"]}}"#;
    let report = driven(&exe, "no-surface", script);
    assert_eq!(
        report.rules_skipped(),
        vec!["surface-schema"],
        "only the surface rule is gated: {}",
        report.render()
    );
    assert!(
        report.not_applicable[0]
            .reason
            .contains("no `surface` capability"),
        "the reason must name the gate: {}",
        report.not_applicable[0].reason
    );
    assert!(
        report.violations.is_empty(),
        "a rule the plugin was never asked to answer is not a violation: {}",
        report.render()
    );
    assert_eq!(report.rules_failed(), Vec::<&str>::new());
}
