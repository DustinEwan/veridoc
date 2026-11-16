//! The verdict cache: a key covers the section body, the claim set, the referents, the
//! model, and the prompt version, so editing one sentence misses only that section's keys
//! while every other section still hits.

use std::path::PathBuf;

use serde_json::json;
use veridoc_engine::hash;
use veridoc_engine::verdicts;

fn tmp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vd-engine-verdicts-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn key_for(body: &str, claims: &str) -> String {
    let section = hash::section_hash(body);
    let set = hash::claim_set_hash(&mut [hash::per_claim_hash(
        "c1", "ensures", "p", claims, "null",
    )]);
    let revision = hash::referent_revision(&mut ["vd:r:probe\tsig".to_string()]);
    verdicts::key("s1", &section, &set, &revision, "model-a", "vd-prompt-1")
}

#[test]
fn editing_one_sentence_misses_only_that_sections_key() {
    let root = tmp_dir("edit");
    let body_a = "The scanner walks upward from the working directory and stops at the first directory that holds a project root.\n";
    let body_b = "The plugin answers eight methods in a fixed order.\n";

    let key_a_before = key_for(body_a, "text a");
    let key_b = key_for(body_b, "text b");
    verdicts::store(&root, &key_a_before, &json!({"pairs": []}), "brief a").unwrap();
    verdicts::store(&root, &key_b, &json!({"pairs": []}), "brief b").unwrap();

    // One edited sentence in section A.
    let edited = body_a.replace("first directory", "first parent directory");
    let key_a_after = key_for(&edited, "text a");
    assert_ne!(key_a_before, key_a_after, "an edit must move the key");
    assert!(verdicts::load(&root, &key_a_after).is_none(), "the edited section must miss");
    assert!(verdicts::load(&root, &key_b).is_some(), "the untouched section must hit");
}

#[test]
fn a_stored_verdict_carries_the_brief_that_produced_it() {
    let root = tmp_dir("replay");
    let key = key_for("body\n", "text");
    let verdict = json!({"unmentionedClaims": ["c1"], "unstatedAssertions": []});
    verdicts::store(&root, &key, &verdict, "## Section docs/a.spec.md#s\nsp1: text\n").unwrap();
    let cached = verdicts::load(&root, &key).expect("hit");
    assert_eq!(cached.verdict, verdict);
    assert!(cached.brief.contains("sp1: text"));
}

#[test]
fn a_corrupt_cache_file_is_a_miss_not_a_crash() {
    let root = tmp_dir("corrupt");
    let key = key_for("body\n", "text");
    std::fs::create_dir_all(root.join(".veridoc/verdicts")).unwrap();
    std::fs::write(verdicts::path(&root, &key), b"not json").unwrap();
    assert!(verdicts::load(&root, &key).is_none());
}

#[test]
fn a_prompt_version_change_misses_every_key() {
    let section = hash::section_hash("body\n");
    let set = hash::claim_set_hash(&mut ["x".to_string()]);
    let revision = hash::referent_revision(&mut ["vd:r:probe\tsig".to_string()]);
    let old = verdicts::key("s1", &section, &set, &revision, "model-a", "vd-prompt-1");
    let new = verdicts::key("s1", &section, &set, &revision, "model-a", "vd-prompt-2");
    assert_ne!(old, new);
}
