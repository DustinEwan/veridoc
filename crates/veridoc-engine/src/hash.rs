//! The hashes the engine computes, in one place so the identity rules cannot drift.
//! `hex16` is the first 16 hex characters of the digest; `hex64` is the full digest.

use sha2::{Digest, Sha256};

pub fn hex64(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn hex16(bytes: &[u8]) -> String {
    hex64(bytes)[..16].to_string()
}

/// `c` + `hex16(sha256(referent_id + "\u{0}" + predicate))`. A claim's identity is its
/// resolved subject and its predicate; nothing else may move it.
pub fn claim_id(referent_id: &str, predicate: &str) -> String {
    let mut bytes = Vec::with_capacity(referent_id.len() + predicate.len() + 2);
    bytes.extend_from_slice(referent_id.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(predicate.as_bytes());
    format!("c{}", hex16(&bytes))
}

/// `hex64(sha256(normalized section body))`. Normalized: CRLF to LF, trailing whitespace per
/// line stripped, one trailing newline.
pub fn section_hash(body: &str) -> String {
    let mut normalized = String::with_capacity(body.len());
    for line in body.replace("\r\n", "\n").lines() {
        normalized.push_str(line.trim_end());
        normalized.push('\n');
    }
    hex64(normalized.as_bytes())
}

/// `hex64(sha256(join("\n", sorted per-claim hashes)))`, where a per-claim hash covers the
/// claim id, kind, predicate, text, and the canonical JSON of the assertion tree as the
/// engine parsed it.
pub fn claim_set_hash(per_claim: &mut [String]) -> String {
    per_claim.sort();
    hex64(per_claim.join("\n").as_bytes())
}

/// One claim's contribution to `claim_set_hash`.
pub fn per_claim_hash(claim_id: &str, kind: &str, predicate: &str, text: &str, asserts_json: &str) -> String {
    let joined = format!("{claim_id}\u{0}{kind}\u{0}{predicate}\u{0}{text}\u{0}{asserts_json}");
    hex64(joined.as_bytes())
}

/// `hex64(sha256(join("\n", sorted "referent_id\tsignature")))` over the claims in the
/// section. Computable engine-side from the plugin's `resolve` signatures, which is why it is
/// not a plugin call.
pub fn referent_revision(entries: &mut [String]) -> String {
    entries.sort();
    hex64(entries.join("\n").as_bytes())
}

/// The verdict cache key: rule, section, claims, referents, model, and prompt version.
pub fn verdict_key(
    rule_id: &str,
    section_hash: &str,
    claim_set_hash: &str,
    referent_revision: &str,
    model: &str,
    prompt_version: &str,
) -> String {
    let joined = format!(
        "{rule_id}\u{0}{section_hash}\u{0}{claim_set_hash}\u{0}{referent_revision}\u{0}{model}\u{0}{prompt_version}"
    );
    hex16(joined.as_bytes())
}
