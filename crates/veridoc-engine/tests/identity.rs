//! Claim identity is the author's subject joined with the predicate, so moving the file that
//! holds the code moves no claim (`docs/decisions/0009-the-subject-is-named-not-derived-from-a-path.md`).

use veridoc_engine::{
    bind::{self, BindFailure},
    hash,
};
use veridoc_protocol::RefToken;

#[test]
fn a_claim_name_moves_with_the_subject_and_not_the_file() {
    let written = RefToken::Symbol {
        name: "parse_config".into(),
        from: "src/config.rs".into(),
    };
    let moved = RefToken::Symbol {
        name: "parse_config".into(),
        from: "src/v2/configuration.rs".into(),
    };
    assert_eq!(
        hash::claim_name(written.subject_name(), "rejects-a-missing-file"),
        hash::claim_name(moved.subject_name(), "rejects-a-missing-file"),
        "the name a reader writes must survive a file rename"
    );
    assert_ne!(
        written.key(),
        moved.key(),
        "the resolve key still names the file, so coverage keeps pointing at real code"
    );
    assert_eq!(
        RefToken::Concept {
            name: "architecture".into()
        }
        .subject_name(),
        "architecture",
        "a concept subject names itself"
    );
}

#[test]
fn two_claims_over_one_subject_and_predicate_are_a_duplicate() {
    let root = std::env::temp_dir().join(format!("veridoc-engine-identity-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(
        root.join("docs/architecture.spec.md"),
        "<doc-section name=\"probe\">\n\nThe root is refused <doc-claim name=\"rejects-missing-project-root\"/>.\n",
    )
    .unwrap();
    let root = root.canonicalize().unwrap();

    let mut wire: bind::CollectWire = serde_json::from_value(veridoc_fakeplugin::default_collect(
        &serde_json::Value::Null,
    ))
    .expect("default collect");
    let mut second = wire.claims[0].clone();
    second.file = "src/other.doc.ts".into();
    second.line = 9;
    wire.claims.push(second);

    let result = bind::bind(
        &root,
        "veridoc-fakeplugin",
        &[
            veridoc_protocol::Capability::Collect,
            veridoc_protocol::Capability::Resolution,
            veridoc_protocol::Capability::Derivability,
            veridoc_protocol::Capability::Bivalence,
            veridoc_protocol::Capability::Execution,
            veridoc_protocol::Capability::Citation,
            veridoc_protocol::Capability::Surface,
        ],
        &[],
        wire,
        |keys| {
            let value = veridoc_fakeplugin::default_resolve(&serde_json::json!({ "refs": keys }));
            serde_json::from_value(value)
                .map(veridoc_protocol::Call::Result)
                .unwrap_or_else(|e| {
                    veridoc_protocol::Call::Lost(veridoc_protocol::Loss::Profile {
                        reason: veridoc_protocol::UNUSABLE_RESULT.to_string(),
                        detail: e.to_string(),
                    })
                })
        },
    );
    let errors = match result {
        Ok(bound) => panic!("one subject and predicate must not bind twice: {bound:?}"),
        Err(BindFailure::Errors(errors)) => errors,
        Err(BindFailure::RunFailure(why)) => panic!("unexpected run failure: {why}"),
    };
    assert!(
        errors.iter().any(|e| e
            .diagnostic()
            .contains("duplicates probe#rejects-missing-project-root")),
        "expected the duplicate to name the owner: {errors:?}"
    );
}
