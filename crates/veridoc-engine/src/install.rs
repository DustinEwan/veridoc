//! The install machinery's engine half: clone a plugin repository, pin it to a commit, read
//! its manifest, and run its install and build steps. The conformance drive and the registry
//! swap live in the CLI, because the suite depends on this crate and the engine must not
//! depend back.

use std::path::PathBuf;
use std::process::Command;

use crate::git::git;
use crate::plugins::{self, PluginManifest};

/// A plugin repository cloned into the registry's staging directory, pinned, and read.
pub struct Staged {
    /// The staging directory; it becomes `<registry>/<name>/checkout` when conformance passes.
    pub path: PathBuf,
    /// `git rev-parse HEAD` after the optional reference checkout.
    pub commit: String,
    pub manifest: PluginManifest,
}

/// Clone `url` into a fresh staging directory under the registry and pin it. A `reference`
/// (branch, tag, or commit) is checked out before the pin, so the recorded commit is the
/// commit that will run.
pub fn stage(url: &str, reference: Option<&str>) -> Result<Staged, String> {
    let registry = plugins::root();
    std::fs::create_dir_all(&registry)
        .map_err(|e| format!("cannot create {}: {e}", registry.display()))?;
    let staging = registry.join(format!(".staging-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    git(
        registry.parent().unwrap_or(&registry),
        &["clone", url, &staging.display().to_string()],
        &[],
    )
    .map_err(|e| format!("cannot clone {url}: {}", e.0))?;
    if let Some(reference) = reference {
        git(&staging, &["checkout", reference], &[])
            .map_err(|e| format!("cannot check out \"{reference}\": {}", e.0))?;
    }
    let commit = git(&staging, &["rev-parse", "HEAD"], &[])
        .map_err(|e| format!("cannot pin the checkout: {}", e.0))?;
    let manifest = plugins::read_manifest(&staging)?;
    Ok(Staged {
        path: staging,
        commit,
        manifest,
    })
}

/// Run the manifest's `install` steps and then its `build` steps, with the checkout as the
/// working directory. A step that exits non-zero stops the install; nothing is recorded.
pub fn run_steps(staged: &Staged) -> Result<(), String> {
    for (label, steps) in [("install", &staged.manifest.install), ("build", &staged.manifest.build)] {
        for argv in steps {
            let Some((program, args)) = argv.split_first() else {
                return Err(format!("a {label} step is an empty argv"));
            };
            let status = Command::new(program)
                .args(args)
                .current_dir(&staged.path)
                .status()
                .map_err(|e| format!("cannot run {label} step `{}`: {e}", argv.join(" ")))?;
            if !status.success() {
                return Err(format!(
                    "{label} step `{}` exited {status}",
                    argv.join(" ")
                ));
            }
        }
    }
    Ok(())
}

/// Remove a staging directory after a failed or finished install. Failure is silent: a stale
/// staging directory is swept by the next install of the same name.
pub fn discard(path: &PathBuf) {
    let _ = std::fs::remove_dir_all(path);
}
