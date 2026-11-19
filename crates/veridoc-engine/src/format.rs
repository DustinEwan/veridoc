//! Formatters run after `result.json` is written, in config order. A built-in name renders
//! inside the engine; an object entry spawns a command with `VERIDOC_RESULT` and
//! `VERIDOC_OUT` in its environment. An unchanged content hash skips the whole dispatch
//! unless `--force` says otherwise.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::config::Config;
use crate::result::ResultDocument;

/// Run the formatters. `Err(detail)` is a run failure: the CLI exits 2.
pub fn dispatch(
    root: &Path,
    config: &Config,
    doc: &ResultDocument,
    result_json: &Path,
) -> Result<(), String> {
    for entry in &config.output.formatters {
        match entry {
            Value::String(name) => match name.as_str() {
                "markdown" => markdown(root, config, doc)?,
                "json" => json(root, config, result_json)?,
                other => return Err(format!("unknown formatter \"{other}\"")),
            },
            Value::Object(object) => {
                let name = object["name"].as_str().unwrap_or("formatter");
                let Some(argv) = object["command"].as_array() else {
                    return Err(format!("formatter \"{name}\" has no command array"));
                };
                command(root, config, name, argv, result_json)?;
            }
            other => return Err(format!("formatter entry is not a name or object: {other}")),
        }
    }
    Ok(())
}

/// The pre-write check: the stored hash is the one the last successful run recorded.
pub fn unchanged(root: &Path, hash: &str) -> bool {
    std::fs::read_to_string(root.join(".veridoc/last-result-hash"))
        .map(|text| text.trim() == hash)
        .unwrap_or(false)
}

/// One file per document node under `{output.dir}/markdown/`, mirroring the tree: the
/// authored spans, then the named claims with subject and signature in words. A claim
/// carrying the residue flag carries the marker `[unverified]` before its text.
fn markdown(root: &Path, config: &Config, doc: &ResultDocument) -> Result<(), String> {
    let base = root.join(&config.output.dir).join("markdown");
    for node in &doc.tree {
        let sections: Vec<_> = doc
            .sections
            .iter()
            .filter(|s| s.file == node.path)
            .collect();
        let mut out = String::new();
        out.push_str(&format!("# {}\n\n", node.path));
        for section in sections {
            out.push_str(&format!("## {}\n\n", section.subject));
            for span in &section.spans {
                out.push_str(&format!("{}\n\n", span.text));
            }
            if !section.claims.is_empty() {
                out.push_str("### Claims\n\n");
                for id in &section.claims {
                    let Some(claim) = doc.claims.iter().find(|c| c.id == *id) else {
                        continue;
                    };
                    let marker = if claim.residue { "[unverified] " } else { "" };
                    out.push_str(&format!(
                        "- {marker}{} ({})\n",
                        claim.text,
                        match claim.outcome {
                            crate::result::Outcome::Supported => "supported",
                            crate::result::Outcome::Contradicted => "contradicted",
                            crate::result::Outcome::RunFailed => "run-failed",
                        }
                    ));
                    out.push_str(&format!(
                        "  - subject: {} — {}\n",
                        claim.referent_id, claim.signature
                    ));
                }
                out.push('\n');
            }
            if !section.injections.is_empty() {
                out.push_str("### Injected\n\n");
                for pulled in &section.injections {
                    out.push_str(&format!(
                        "- doc-each source=\"{}\": {} {}\n",
                        pulled.source,
                        pulled.items,
                        if pulled.items == 1 { "value" } else { "values" }
                    ));
                }
                out.push('\n');
            }
        }
        let residue = doc.metrics.residue;
        let total = doc.claims.len();
        out.push_str(&format!(
            "---\nResidue: {residue} of {total} claims rest on text this project cannot check.\n"
        ));
        // The authored source is `docs/architecture.spec.md`; the rendered document
        // drops the `.spec` marker because the output is the finished prose.
        let rendered = node
            .path
            .strip_suffix(".spec.md")
            .map(|stem| format!("{stem}.md"))
            .unwrap_or_else(|| node.path.clone());
        let file = base.join(&rendered);
        std::fs::create_dir_all(file.parent().expect("mirrored dir"))
            .map_err(|e| format!("writing markdown failed: {e}"))?;
        std::fs::write(file, out).map_err(|e| format!("writing markdown failed: {e}"))?;
    }
    Ok(())
}

fn json(root: &Path, config: &Config, result_json: &Path) -> Result<(), String> {
    let dir = root.join(&config.output.dir).join("json");
    std::fs::create_dir_all(&dir).map_err(|e| format!("writing json failed: {e}"))?;
    std::fs::copy(result_json, dir.join("result.json"))
        .map_err(|e| format!("copying result.json failed: {e}"))?;
    Ok(())
}

/// Spawn the formatter with the project root as its working directory; captured output is
/// replayed only on failure.
fn command(
    root: &Path,
    config: &Config,
    name: &str,
    argv: &[Value],
    result_json: &Path,
) -> Result<(), String> {
    let Some(program) = argv.first().and_then(Value::as_str) else {
        return Err(format!("formatter \"{name}\" command is empty"));
    };
    let out_dir = root.join(&config.output.dir).join(name);
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let result_path = if result_json.is_absolute() {
        result_json.to_path_buf()
    } else {
        root.join(result_json)
    };
    let mut child = Command::new(program)
        .args(argv.iter().skip(1).filter_map(Value::as_str))
        .current_dir(root)
        .env("VERIDOC_RESULT", &result_path)
        .env("VERIDOC_OUT", &out_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("formatter \"{name}\" could not be spawned: {e}"))?;
    let mut stdout = String::new();
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_string(&mut stdout);
    }
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    let status = child
        .wait()
        .map_err(|e| format!("formatter \"{name}\": {e}"))?;
    if !status.success() {
        let code = status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "a signal".to_string());
        // Replay the captured output so the failure is diagnosable from the CLI alone.
        if !stdout.is_empty() {
            eprint!("{stdout}");
        }
        if !stderr.is_empty() {
            eprint!("{stderr}");
        }
        return Err(format!("formatter \"{name}\" exited with {code}"));
    }
    Ok(())
}
