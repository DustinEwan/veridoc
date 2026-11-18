//! The result document writer: the hash excludes `generated_at` so an unchanged run can skip
//! the formatters, the write is atomic, and `.veridoc/last-result-hash` is rewritten after
//! every run.

use std::path::{Path, PathBuf};

use veridoc_engine::result::{self, ClaimResult, Metrics, ResultDocument, Span, TreeNode};
use veridoc_protocol::Kind;

fn tmp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "veridoc-engine-result-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn sample_doc(text: &str) -> ResultDocument {
    ResultDocument {
        protocol_version: "veridoc/1".into(),
        generated_at: String::new(),
        tree: vec![TreeNode {
            path: "docs/architecture.spec.md".into(),
            audience: "an author".into(),
            question: "What must my plugin answer?".into(),
            order: 0,
            not_applicable: None,
        }],
        sections: vec![result::SectionResult {
            file: "docs/architecture.spec.md".into(),
            subject: "scanner".into(),
            spans: vec![Span {
                id: "sp1".into(),
                text: text.into(),
            }],
            claims: vec!["c000000000000001".into()],
            injections: vec![],
        }],
        claims: vec![ClaimResult {
            id: "c000000000000001".into(),
            kind: Kind::Ensures,
            text: "A missing project root is refused.".into(),
            predicate: "rejects-missing-project-root".into(),
            concept: "project-scan".into(),
            referent_id: "veridoc:referent:abc".into(),
            signature: "function parseConfig(root: string): ProjectConfig".into(),
            channel: "scenario".into(),
            outcome: result::Outcome::Supported,
            residue: false,
            evidence: "config/rejects-missing-root".into(),
        }],
        concepts: vec!["project-scan".into()],
        warnings: vec![],
        metrics: Metrics {
            coverage: 1.0,
            coverage_detail: "1.000 (1/1 supported; scenario 1/1, artefact 0/0, judge 0/0)".into(),
            custom_logic: 0.0,
            unmatched_prose: 0,
            residue: 0,
            residue_share: 0.0,
            redundancy: 0,
            obligation: 0,
        },
    }
}

#[test]
fn two_identical_runs_share_a_hash_and_differ_in_generated_at() {
    let root = tmp_dir("stable-hash");
    let first = result::write_result(&root, &sample_doc("The scanner walks upward.")).unwrap();
    let written_at_1 = read_generated_at(&root);
    std::thread::sleep(std::time::Duration::from_millis(1_100));
    let second = result::write_result(&root, &sample_doc("The scanner walks upward.")).unwrap();
    let written_at_2 = read_generated_at(&root);
    assert_eq!(
        first, second,
        "an identical document must produce the same resultHash"
    );
    assert!(!written_at_1.is_empty() && !written_at_2.is_empty());
    assert_ne!(
        written_at_1, written_at_2,
        "the timestamp must move between runs a second apart"
    );
}

#[test]
fn an_edited_sentence_moves_the_hash_and_the_recorded_hash_follows() {
    let root = tmp_dir("moved-hash");
    let first = result::write_result(&root, &sample_doc("The scanner walks upward.")).unwrap();
    let recorded = std::fs::read_to_string(root.join(".veridoc/last-result-hash")).unwrap();
    assert_eq!(recorded.trim(), first);
    let second =
        result::write_result(&root, &sample_doc("The scanner walks upward first.")).unwrap();
    assert_ne!(first, second);
    let recorded = std::fs::read_to_string(root.join(".veridoc/last-result-hash")).unwrap();
    assert_eq!(
        recorded.trim(),
        second,
        "last-result-hash is rewritten after each run"
    );
}

#[test]
fn the_write_is_atomic_and_leaves_no_temporary_file() {
    let root = tmp_dir("atomic");
    result::write_result(&root, &sample_doc("The scanner walks upward.")).unwrap();
    let out = root.join(".veridoc");
    assert!(out.join("result.json").is_file());
    assert!(
        !out.join("result.json.tmp").exists(),
        "the temporary file must not survive the rename"
    );
    let text = std::fs::read_to_string(out.join("result.json")).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(
        value["claims"][0]["outcome"] == "supported",
        "the finished document carries the outcome"
    );
}

fn read_generated_at(root: &Path) -> String {
    let text = std::fs::read_to_string(root.join(".veridoc/result.json")).unwrap();
    serde_json::from_str::<serde_json::Value>(&text).unwrap()["generatedAt"]
        .as_str()
        .unwrap()
        .to_string()
}
