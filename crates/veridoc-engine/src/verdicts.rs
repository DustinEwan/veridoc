//! The verdict cache at `.veridoc/verdicts/{key}.json`: the verdict plus the brief that
//! produced it, so a stored verdict can be replayed (`docs/spec.md:460-465`,
//! `docs/decisions/0002`). Before a verdict is kept or replayed, every id it cites is
//! re-verified against the ids the engine issued; an id the run never issued is a run
//! failure, not a discard.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::brief::Task;
use crate::hash;

/// The key: rule, section, claims, referents, model, and prompt version. One edited
/// sentence moves `section_hash` and misses only that section's keys.
pub fn key(
    rule_id: &str,
    section_hash: &str,
    claim_set_hash: &str,
    referent_revision: &str,
    model: &str,
    prompt_version: &str,
) -> String {
    hash::verdict_key(
        rule_id,
        section_hash,
        claim_set_hash,
        referent_revision,
        model,
        prompt_version,
    )
}

/// What a cache file holds: the verdict and the brief that produced it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cached {
    pub verdict: Value,
    pub brief: String,
}

pub fn path(root: &Path, key: &str) -> PathBuf {
    root.join(".veridoc")
        .join("verdicts")
        .join(format!("{key}.json"))
}

/// A cache read that fails for any reason is a miss; the cache may not break a run.
pub fn load(root: &Path, key: &str) -> Option<Cached> {
    let text = std::fs::read_to_string(path(root, key)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn store(root: &Path, key: &str, verdict: &Value, brief: &str) -> std::io::Result<()> {
    let file = path(root, key);
    std::fs::create_dir_all(file.parent().expect("cache dir"))?;
    let text = serde_json::to_string_pretty(&Cached {
        verdict: verdict.clone(),
        brief: brief.to_string(),
    })
    .expect("cached is valid json");
    std::fs::write(file, text)
}

/// Every id the verdict cites: S1 spans and claim ids, S2 spans and claim ids, S3 node
/// paths.
pub fn cited_ids(task: Task, verdict: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    match task {
        Task::S1 => {
            for entry in verdict["unstatedAssertions"]
                .as_array()
                .into_iter()
                .flatten()
            {
                if let Some(span) = entry["span"].as_str() {
                    ids.push(span.to_string());
                }
            }
            for claim in verdict["unmentionedClaims"]
                .as_array()
                .into_iter()
                .flatten()
            {
                if let Some(id) = claim.as_str() {
                    ids.push(id.to_string());
                }
            }
        }
        Task::S2 => {
            for pair in verdict["pairs"].as_array().into_iter().flatten() {
                for field in ["spanA", "spanB", "claimId"] {
                    if let Some(id) = pair[field].as_str() {
                        ids.push(id.to_string());
                    }
                }
            }
        }
        Task::S3 => {
            for pair in verdict["pairs"].as_array().into_iter().flatten() {
                for field in ["nodeA", "nodeB"] {
                    if let Some(id) = pair[field].as_str() {
                        ids.push(id.to_string());
                    }
                }
            }
        }
    }
    ids
}

/// Re-verify every cited id against the ids the brief issued. The message is the run
/// failure the plan fixes; the caller stops the run with exit 2.
pub fn verify_ids(task: Task, verdict: &Value, issued: &[String]) -> Result<(), String> {
    for id in cited_ids(task, verdict) {
        if !issued.contains(&id) {
            return Err(format!(
                "judge verdict for {} cited span {id} that this run never issued",
                task.name()
            ));
        }
    }
    Ok(())
}
