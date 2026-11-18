//! The artefact channel: hash match supports, mismatch and a missing file contradict with
//! the recorded-versus-actual detail, and an escaping path is refused at bind.

use std::path::{Path, PathBuf};

use veridoc_engine::artefact::{self, Verified};
use veridoc_engine::bind::{BoundChannel, BoundClaim};
use veridoc_engine::execute::Outcome;
use veridoc_engine::hash::hex64;
use veridoc_protocol::Kind;

fn tmp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "veridoc-engine-artefact-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn artefact_claim(rel: &str, recorded: &str) -> BoundClaim {
    BoundClaim {
        id: "c000000000000000a".into(),
        kind: Kind::Witness,
        concept: "project-scan".into(),
        predicate: "refused-at-this-path".into(),
        text: "The refusal was captured in this transcript.".into(),
        file: "src/cli.doc.ts".into(),
        line: 12,
        subject: "probe".into(),
        referent_id: "veridoc:referent:probe".into(),
        signature: "function probe(dir: string): number".into(),
        channel: BoundChannel::Artefact {
            path: rel.into(),
            sha256: recorded.into(),
        },
    }
}

fn one(claims: &[BoundClaim], root: &Path) -> Verified {
    let verified = artefact::verify(claims, root);
    assert_eq!(verified.len(), 1);
    verified.into_iter().next().unwrap()
}

#[test]
fn a_matching_hash_supports_the_claim() {
    let root = tmp_dir("match");
    std::fs::write(root.join("evidence.txt"), b"transcript bytes").unwrap();
    let recorded = hex64(b"transcript bytes");
    let claim = artefact_claim("evidence.txt", &recorded);
    let result = one(&[claim], &root);
    assert_eq!(result.outcome, Outcome::Supported);
    assert!(result.detail.is_empty());
}

#[test]
fn a_mismatching_hash_contradicts_with_both_hashes() {
    let root = tmp_dir("mismatch");
    std::fs::write(root.join("evidence.txt"), b"edited bytes").unwrap();
    let recorded = hex64(b"original bytes");
    let claim = artefact_claim("evidence.txt", &recorded);
    let result = one(&[claim], &root);
    assert_eq!(result.outcome, Outcome::Contradicted);
    assert_eq!(
        result.detail,
        format!(
            "artefact evidence.txt hashes to {}, the claim records {recorded}",
            hex64(b"edited bytes")
        )
    );
}

#[test]
fn a_missing_file_contradicts() {
    let root = tmp_dir("missing");
    let recorded = hex64(b"whatever");
    let claim = artefact_claim("gone.bin", &recorded);
    let result = one(&[claim], &root);
    assert_eq!(result.outcome, Outcome::Contradicted);
    assert_eq!(
        result.detail,
        format!("artefact gone.bin hashes to missing, the claim records {recorded}")
    );
}

#[test]
fn an_uppercase_recorded_hash_still_matches() {
    let root = tmp_dir("case");
    std::fs::write(root.join("evidence.txt"), b"bytes").unwrap();
    let recorded = hex64(b"bytes").to_uppercase();
    let claim = artefact_claim("evidence.txt", &recorded);
    assert_eq!(one(&[claim], &root).outcome, Outcome::Supported);
}
