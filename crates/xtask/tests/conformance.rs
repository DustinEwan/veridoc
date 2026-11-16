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
    let start = exe.parent().expect("test binary in a directory").to_path_buf();
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
    assert!(rendered.success(), "the emitter failed to render the fixture");
    let report = veridoc_conformance::run(&Check {
        launch: Launch::new(vec![exe.to_string_lossy().into_owned()]),
        project: dir.clone(),
        deadline_ms: 20_000,
    })
    .expect("the emitted fixture is readable");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(report.claims, 6, "the fixture reports its scenario claim and five probes");
    assert!(
        report.passed(),
        "the fake plugin violates its own conformance contract:\n{}",
        report.render()
    );
}
