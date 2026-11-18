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

/// `c` + `hex16(sha256(subject + "\u{0}" + predicate))`. The identity value behind a
/// claim's readable name: it proves the name still means the same thing, and it separates two
/// claims whose names slug alike (`docs/spec.md:152-158`). It is not a name, and no
/// human-facing surface prints it except as a collision suffix. The subject is the author's name
/// for the thing the claim is about, so moving the file that holds the code moves no claim
/// (`docs/decisions/0009-the-subject-is-named-not-derived-from-a-path.md`).
pub fn claim_identity(subject: &str, predicate: &str) -> String {
    let mut bytes = Vec::with_capacity(subject.len() + predicate.len() + 2);
    bytes.extend_from_slice(subject.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(predicate.as_bytes());
    format!("c{}", hex16(&bytes))
}

/// The claim's name: `<subject>#<predicate slug>`, built from the identity's own inputs so that
/// nothing else can move it. This is what a person or an agent reads in a report, types to
/// `veridoc witness`, and writes as the `name` of a `<doc-section>` (`docs/spec.md:152-158`).
pub fn claim_name(subject: &str, predicate: &str) -> String {
    format!("{}#{}", subject, slug(predicate, MAX_SLUG))
}

/// The identity shortened for use as a collision suffix, so two claims that slug alike stay
/// distinguishable without reprinting the whole digest.
pub fn suffix(identity: &str) -> String {
    format!("~{}", &identity[1..7])
}

/// The longest predicate slug a name carries before it is cut at a word boundary.
const MAX_SLUG: usize = 48;

/// Lower-case, alphanumeric runs joined by `-`, cut at the last whole word that fits in
/// `max`. A cut name is a prefix of the uncut one, so a claim keeps the prefix a reader may
/// already have copied even when a longer word pushes the tail off.
fn slug(text: &str, max: usize) -> String {
    let lowered = text.to_lowercase();
    let mut words: Vec<&str> = Vec::new();
    for word in lowered
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        let joined = if words.is_empty() {
            word.len()
        } else {
            words.iter().map(|w| w.len() + 1).sum::<usize>() + word.len()
        };
        if joined > max {
            break;
        }
        words.push(word);
    }
    words.join("-")
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
pub fn per_claim_hash(
    claim_id: &str,
    kind: &str,
    predicate: &str,
    text: &str,
    asserts_json: &str,
) -> String {
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
