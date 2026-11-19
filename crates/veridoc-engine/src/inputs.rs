//! The input fingerprint at `.veridoc/inputs.json`: every file the recorded run was made
//! against, and its content hash. `veridoc check` writes it next to the result document, and
//! `veridoc stat` compares the working tree against it. That comparison is the gate a
//! continuous-integration job runs when it cannot call a model: it answers "are these the files
//! the last judged run read?" without starting a plugin, building a harness, or asking a model
//! anything (`docs/decisions/0014`).
//!
//! The fingerprint is the whole project, not the slice a verdict key covers. A key covers one
//! section's body, its claims, and its referent signatures; the fingerprint covers every tracked
//! file, so a change to a source file the record never hashed cannot slip through. The cost is
//! that an unrelated commit fails the gate, and the repair is a local `veridoc check`, which
//! replays every unchanged section from the verdict cache and calls no model.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::CONFIG_DIR;
use crate::hash;

/// Directories never part of a project's inputs: build output and dependency trees.
const EXCLUDED_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "target",
    "node_modules",
    "dist",
    "build",
    ".venv",
    "venv",
    "__pycache__",
];

/// Files that hold a machine's own values, read when the configuration loads. A git checkout
/// already keeps them out of the record through the ignore rules; this list is what keeps a
/// directory that is not a checkout from hashing them, because a record that differs between a
/// developer's machine and a continuous-integration runner answers the wrong question.
const EXCLUDED_NAMES: &[&str] = &[".env"];

/// The files a run produces inside `.veridoc/`, which the fingerprint must not cover: covering
/// them would make every run's own output invalidate it.
const RECORD_OUTPUTS: &[&str] = &[
    "verdicts",
    "out",
    "result.json",
    "last-result-hash",
    "inputs.json",
    "engine.log",
    // The scenario harness build a plugin writes during a run; a build directory is never an
    // input the run read, and a project whose ignore rules are anchored at its own root would
    // otherwise carry it into the record and into the commit.
    "target",
];

/// One recorded input: the project-relative path and the sha256 of its content.
pub type Files = BTreeMap<String, String>;

/// What changed between the recorded fingerprint and the working tree.
#[derive(Debug, Default)]
pub struct Diff {
    pub changed: Vec<String>,
    pub removed: Vec<String>,
    pub added: Vec<String>,
}

impl Diff {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.removed.is_empty() && self.added.is_empty()
    }

    pub fn count(&self) -> usize {
        self.changed.len() + self.removed.len() + self.added.len()
    }
}

/// The project's input files, hashed. A git checkout is fingerprinted by what git tracks, which
/// is what a commit contains and already respects every ignore rule; any other directory is
/// walked with the exclusion list above.
///
/// `output_dir` is the project's `output.dir`: the documentation a run renders is that run's own
/// product, so it is excluded wherever the project chooses to put it, including a tracked
/// directory such as `docs/`. Covering it would make every run invalidate the record it just wrote.
pub fn collect(root: &Path, output_dir: &str) -> io::Result<Files> {
    let paths = match tracked(root) {
        Some(paths) => paths,
        None => walk(root)?,
    };
    let mut files = Files::new();
    for relative in paths {
        if !is_input(&relative) || is_output(&relative, output_dir) {
            continue;
        }
        let path = root.join(&relative);
        let Ok(bytes) = std::fs::read(&path) else {
            // A tracked path that is not on disk (a staged deletion, a broken symlink) is not an
            // input this run read, and `stat` reports it as removed.
            continue;
        };
        files.insert(relative, hash::hex64(&bytes));
    }
    Ok(files)
}

/// Whether `relative` is inside the directory the run renders its documentation into. The config
/// spells that directory with an optional leading `./` and an optional trailing `/`, and the paths
/// compared here never carry either, so both are trimmed before the comparison.
fn is_output(relative: &str, output_dir: &str) -> bool {
    let dir = output_dir
        .trim()
        .trim_start_matches("./")
        .trim_end_matches('/');
    !dir.is_empty() && relative.starts_with(&format!("{dir}/"))
}

fn is_input(relative: &str) -> bool {
    let mut parts = relative.split('/');
    let Some(first) = parts.next() else {
        return false;
    };
    if EXCLUDED_DIRS.contains(&first) {
        return false;
    }
    if EXCLUDED_NAMES.contains(&first) {
        return false;
    }
    if first != CONFIG_DIR {
        return true;
    }
    // Inside `.veridoc/`, the config, the citation stores, and anything an author adds are
    // inputs; the run's own outputs are not.
    match parts.next() {
        Some(name) => !RECORD_OUTPUTS.contains(&name),
        None => false,
    }
}

/// The files a commit would carry: tracked plus not-yet-tracked, minus whatever the ignore rules
/// hide. A file the author has just added and not yet staged is in that set, which is the point —
/// the record must cover it before the commit that carries it exists.
fn tracked(root: &Path) -> Option<Vec<String>> {
    let output = Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&output.stdout)
            .split('\0')
            .filter(|p| !p.is_empty())
            .map(|p| p.replace('\\', "/"))
            .collect(),
    )
}

fn walk(root: &Path) -> io::Result<Vec<String>> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut children: Vec<PathBuf> = match std::fs::read_dir(&dir) {
            Ok(entries) => entries.filter_map(Result::ok).map(|e| e.path()).collect(),
            Err(_) => continue,
        };
        children.sort();
        for child in children {
            let name = child.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if child.is_dir() {
                if !EXCLUDED_DIRS.contains(&name) {
                    stack.push(child);
                }
                continue;
            }
            if let Ok(relative) = child.strip_prefix(root) {
                found.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    Ok(found)
}

/// Write `.veridoc/inputs.json` atomically, beside the result document.
pub fn write(root: &Path, files: &Files) -> io::Result<()> {
    let dir = root.join(CONFIG_DIR);
    std::fs::create_dir_all(&dir)?;
    let text = serde_json::to_string_pretty(&serde_json::json!({
        "files": files.iter().map(|(path, sha)| serde_json::json!({"path": path, "sha256": sha})).collect::<Vec<_>>(),
    }))? + "\n";
    let tmp = dir.join("inputs.json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, dir.join("inputs.json"))
}

/// The recorded fingerprint, or `None` when the project has never completed a run.
pub fn read(root: &Path) -> io::Result<Option<Files>> {
    let path = root.join(CONFIG_DIR).join("inputs.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{path:?}: {e}")))?;
    let mut files = Files::new();
    for entry in value["files"].as_array().into_iter().flatten() {
        let path = entry["path"].as_str().unwrap_or_default().to_string();
        let sha = entry["sha256"].as_str().unwrap_or_default().to_string();
        files.insert(path, sha);
    }
    Ok(Some(files))
}

/// Compare the working tree against the record. A path in the record that is no longer on disk
/// is removed; a path on disk that the record does not name is added.
pub fn diff(recorded: &Files, current: &Files) -> Diff {
    let mut diff = Diff::default();
    for (path, sha) in recorded {
        match current.get(path) {
            None => diff.removed.push(path.clone()),
            Some(now) if now != sha => diff.changed.push(path.clone()),
            Some(_) => {}
        }
    }
    for path in current.keys() {
        if !recorded.contains_key(path) {
            diff.added.push(path.clone());
        }
    }
    diff
}
