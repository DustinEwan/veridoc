//! The artefact channel: a `witness` claim records a file and its sha256, and the engine
//! re-hashes the file. A file that no longer resolves, or no longer matches its recorded
//! hash, contradicts the claim (`docs/spec.md:133`); the engine never writes the file or the
//! claim, so refreshing a hash is an explicit author act through `veridoc witness`.

use std::path::Path;

use crate::bind::{BoundChannel, BoundClaim};
use crate::execute::Outcome;
use crate::hash::hex64;

/// One artefact claim after its file was re-hashed.
#[derive(Debug, Clone)]
pub struct Verified {
    pub claim_id: String,
    pub outcome: Outcome,
    /// For a contradicted claim: `artefact {path} hashes to {actual}, the claim records {recorded}`.
    pub detail: String,
}

/// The lowercase hex sha256 of the file a relative path names, or the reason it could not
/// be read. `veridoc witness <claim-id>` prints this.
pub fn sha256_of(root: &Path, rel: &str) -> Result<String, String> {
    let bytes = std::fs::read(root.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
    Ok(hex64(&bytes))
}

/// Verify every artefact-channel claim. A contradicted artefact is a claim state, not a run
/// failure: the file is the evidence and it disagreed.
pub fn verify(claims: &[BoundClaim], root: &Path) -> Vec<Verified> {
    let mut verified = Vec::new();
    for claim in claims {
        let BoundChannel::Artefact { path, sha256 } = &claim.channel else {
            continue;
        };
        match sha256_of(root, path) {
            Ok(actual) if actual == sha256.to_lowercase() => verified.push(Verified {
                claim_id: claim.id.clone(),
                outcome: Outcome::Supported,
                detail: String::new(),
            }),
            Ok(actual) => verified.push(Verified {
                claim_id: claim.id.clone(),
                outcome: Outcome::Contradicted,
                detail: format!("artefact {path} hashes to {actual}, the claim records {sha256}"),
            }),
            Err(_) => verified.push(Verified {
                claim_id: claim.id.clone(),
                outcome: Outcome::Contradicted,
                detail: format!("artefact {path} hashes to missing, the claim records {sha256}"),
            }),
        }
    }
    verified
}
