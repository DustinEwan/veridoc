//! The typed payload every formatter consumes, and the engine-side schema root. A claim whose
//! own scenario could not run is in it, as `run-failed` with the plugin's words beside it: the
//! unit of work is the claim, so a scenario that failed to run is one claim's result and the run
//! still writes the document. A bind error, a judge transport error, and a formatter failure are
//! still diagnostics on the failure stream, and the run writes no document at all in those cases.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use std::path::Path;
use veridoc_protocol::Kind;

/// The result document at `.veridoc/result.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResultDocument {
    pub protocol_version: String,
    /// ISO 8601. Excluded from `resultHash` by hashing the document before this field is set.
    pub generated_at: String,
    pub tree: Vec<TreeNode>,
    pub sections: Vec<SectionResult>,
    pub claims: Vec<ClaimResult>,
    pub concepts: Vec<String>,
    pub warnings: Vec<Warning>,
    pub metrics: Metrics,
}

/// One declared node of the document tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TreeNode {
    pub path: String,
    pub audience: String,
    pub question: String,
    pub order: u32,
    pub not_applicable: Option<String>,
}

/// One parsed `.spec.md` section with the spans the engine issued and the claims it names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SectionResult {
    pub file: String,
    /// The subject from the section's opening element.
    pub subject: String,
    pub spans: Vec<Span>,
    /// The resolved claim ids the section names.
    pub claims: Vec<String>,
    /// What each `<doc-each>` in the section pulled in, in document order: the element's source
    /// and the number of values it resolved to at assembly.
    pub injections: Vec<Injected>,
}

/// One `<doc-each>` element with the number of values it resolved to. The injected text is already
/// in the spans above; this names the element that put it there, which is what a reader of a re-run
/// with no prose diff needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Injected {
    pub source: String,
    pub items: usize,
}

/// One authored span. Span ids are assigned by the engine at assembly time; a verdict citing
/// an id the engine never issued is a run failure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Span {
    pub id: String,
    pub text: String,
}

/// One claim with its outcome and the evidence that produced it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaimResult {
    pub id: String,
    pub kind: Kind,
    pub text: String,
    pub predicate: String,
    pub concept: String,
    pub referent_id: String,
    pub signature: String,
    /// The channel that produced the outcome: `scenario`, `artefact`, `citation`, or `judge`.
    pub channel: String,
    pub outcome: Outcome,
    /// Set on a `because` claim whose citation resolved but whose cited text could not be
    /// read. The single occupant of the residue metric.
    pub residue: bool,
    /// The `When` label of the scenario that ran, the artefact path, the citation, the verdict
    /// key, or — for a `run-failed` claim — the reason the scenario did not run.
    pub evidence: String,
}

/// Three members, and the third is the whole of what a scenario that never ran may say about the
/// claim. `Contradicted` is kept even though a contradicted claim ends the run before the document
/// exists, so a formatter's `match` on the outcome is exhaustive and cannot default a
/// non-`supported` claim to plain prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Supported,
    Contradicted,
    /// The scenario did not run. The claim is neither supported nor contradicted: no run that did
    /// not happen is evidence either way.
    #[serde(rename = "run-failed")]
    RunFailed,
}

/// A warning carried for formatters that want them, beside the same line on stderr.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Warning {
    pub file: Option<String>,
    pub line: Option<u32>,
    pub message: String,
}

/// The six figures of a green run. Names a formatter may render, not prose the engine writes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Metrics {
    pub coverage: f64,
    pub coverage_detail: String,
    pub custom_logic: f64,
    pub unmatched_prose: u32,
    pub residue: u32,
    pub residue_share: f64,
    pub redundancy: u32,
    pub obligation: u32,
}

/// The content hash of a document: `hex64(sha256(serde_json::to_string_pretty(doc)))` with
/// `generated_at` set to the empty string, so two identical runs produce the same hash with
/// different timestamps. This is what lets an unchanged run skip the formatters.
pub fn result_hash(doc: &ResultDocument) -> String {
    let mut hashed = doc.clone();
    hashed.generated_at = String::new();
    let text = serde_json::to_string_pretty(&hashed).expect("the result document is serializable");
    crate::hash::hex64(text.as_bytes())
}

/// Write `.veridoc/result.json` by writing `.veridoc/result.json.tmp` and renaming, so a
/// reader never sees a half-written document, and record the hash in
/// `.veridoc/last-result-hash`. Returns the hash.
pub fn write_result(root: &Path, doc: &ResultDocument) -> Result<String, std::io::Error> {
    let hash = result_hash(doc);
    let mut finished = doc.clone();
    finished.generated_at = iso8601_now();
    let bytes = serde_json::to_string_pretty(&finished)
        .expect("the result document is serializable")
        + "\n";
    let dir = root.join(crate::config::CONFIG_DIR);
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join("result.json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, dir.join("result.json"))?;
    std::fs::write(dir.join("last-result-hash"), format!("{hash}\n"))?;
    Ok(hash)
}

/// UTC timestamp, second resolution; the engine carries no date library.
fn iso8601_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

// Howard Hinnant's `civil_from_days`.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
