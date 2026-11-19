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

/// One file per document node under `{output.dir}/markdown/`, mirroring the tree. The page is the
/// authored prose and nothing else: a `<doc-claim>` marker is removed once it has named its claim,
/// because the reader of a finished page has no use for the reference that bound the sentence. The
/// verdicts, subjects, signatures, and metrics a run computed belong to the result document, which
/// `.veridoc/result.json` and the `json` formatter already carry.
fn markdown(root: &Path, config: &Config, doc: &ResultDocument) -> Result<(), String> {
    let base = root.join(&config.output.dir).join("markdown");
    for node in &doc.tree {
        let mut out = String::new();
        for section in doc.sections.iter().filter(|s| s.file == node.path) {
            for span in &section.spans {
                out.push_str(&prose(&span.text));
                out.push_str("\n\n");
            }
        }
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

/// The authored span with its verification markup taken out. A `<doc-claim name="…"/>` sits inside a
/// sentence, so removing one can leave a space before the punctuation that followed it: "stops at the
/// first project root <doc-claim …/>." has to come back out as "stops at the first project root.".
/// Only the space the removal created is touched, never one the author wrote.
fn prose(text: &str) -> String {
    const PUNCTUATION: [char; 8] = ['.', ',', ':', ';', '!', '?', ')', ']'];
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("<doc-claim") {
        out.push_str(&rest[..at]);
        let Some(close) = rest[at..].find('>') else {
            out.push_str(&rest[at..]);
            return out;
        };
        let after = &rest[at + close + 1..];
        if out.ends_with(' ') && after.starts_with(PUNCTUATION) {
            out.truncate(out.trim_end().len());
        }
        rest = after;
    }
    out.push_str(rest);
    out
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
