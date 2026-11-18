//! `veridoc stat`: the gate for a job that cannot call a model. Report whether this checkout is the
//! one the last completed run recorded, by comparing every file the commit would carry against
//! `.veridoc/inputs.json`. No plugin is started, no scenario harness is built, and no model is
//! asked anything, so the gate costs a directory of hashes (`docs/decisions/0014`).
//!
//! Unlike `stat(1)` and `git status`, this reports through the exit code, because the reader is a
//! job that looks at nothing else: `0` the record covers this checkout, `1` it does not, `3` there
//! is no record to compare against, or no configuration to locate the project by.

use std::path::Path;

use veridoc_engine::{config, inputs};

/// How many paths of each kind are printed before the count stands in for the rest.
const PRINTED: usize = 20;

pub fn run(project: &Path) -> Result<i32, String> {
    let path = config::discover(project).map_err(|e| e.to_string())?;
    let loaded = config::load(&path).map_err(|e| e.to_string())?;
    let root = loaded.root.as_path();
    let Some(recorded) = inputs::read(root).map_err(|e| e.to_string())? else {
        return Err(format!(
            "no .veridoc/inputs.json in {}; run `veridoc check` and commit it, so a job that \
             cannot call a model has something to check against",
            root.display()
        ));
    };
    let current = inputs::collect(root).map_err(|e| e.to_string())?;
    let diff = inputs::diff(&recorded, &current);
    if diff.is_empty() {
        println!(
            "record current: {} files are the ones the last run read",
            recorded.len()
        );
        return Ok(0);
    }
    report("changed", &diff.changed);
    report("removed", &diff.removed);
    report("added", &diff.added);
    eprintln!(
        "record stale: {} of {} recorded inputs differ; run `veridoc check` and commit \
         `.veridoc/inputs.json` with the change",
        diff.count(),
        recorded.len()
    );
    Ok(1)
}

fn report(kind: &str, paths: &[String]) {
    for path in paths.iter().take(PRINTED) {
        println!("{kind}  {path}");
    }
    if paths.len() > PRINTED {
        println!("{kind}  ... and {} more", paths.len() - PRINTED);
    }
}
