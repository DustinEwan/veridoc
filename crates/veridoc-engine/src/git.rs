//! The only git access veridoc needs: run a git command in a directory and capture its stdout.
//! `veridoc install-plugin` clones, pins, and records through this; nothing else shells out.

use std::fmt;
use std::path::Path;
use std::process::Command;

#[derive(Debug)]
pub struct GitError(pub String);

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for GitError {}

/// `git -C <dir> <args…>` with `env` applied; stdout trimmed. A non-zero exit becomes a
/// `GitError` carrying stderr, so the caller's message names the reason git gave.
pub fn git(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Result<String, GitError> {
    let mut command = Command::new("git");
    command.arg("-C").arg(dir);
    for (key, value) in env {
        command.env(key, value);
    }
    let output = command
        .args(args)
        .output()
        .map_err(|e| GitError(format!("cannot run git: {e}")))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if detail.is_empty() {
            format!("git {} exited {}", args.join(" "), output.status)
        } else {
            detail
        };
        return Err(GitError(detail));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
