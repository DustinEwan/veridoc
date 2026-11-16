//! The derivability ban: exact-equality after normalization rejects a verbatim restatement,
//! keeps a claim that merely names a parameter, and prints the inert line when the plugin
//! declares no `derivability`.

use veridoc_engine::bind::BoundClaim;
use veridoc_engine::derivability::{self, normalize};
use veridoc_protocol::{Capability, DerivableResult};

fn claim(text: &str) -> BoundClaim {
    BoundClaim {
        id: "c8223c485f101870d".into(),
        kind: veridoc_protocol::Kind::Ensures,
        concept: "project-scan".into(),
        predicate: "rejects-missing-project-root".into(),
        text: text.into(),
        file: "src/cli.doc.ts".into(),
        line: 3,
        supports: vec![],
        referent_id: "vd:r:probe".into(),
        signature: "function probe(dir: string): number".into(),
        channel: veridoc_engine::bind::BoundChannel::Judge,
    }
}

const ALL_CAPS: &[Capability] = &[
    Capability::Collect,
    Capability::Resolution,
    Capability::Derivability,
    Capability::Bivalence,
    Capability::Execution,
    Capability::Citation,
    Capability::Surface,
];

fn facts() -> DerivableResult {
    DerivableResult {
        facts: vec![
            "probe is exported from src/cli.ts.".into(),
            "probe takes one parameter named dir of type string and returns number.".into(),
        ],
    }
}

#[test]
fn a_verbatim_restatement_is_rejected_and_naming_the_fact() {
    let outcome = derivability::gate(
        vec![claim("`probe` is exported from `src/cli.ts`")],
        "veridoc-fakeplugin",
        ALL_CAPS,
        |_| Ok(facts()),
    )
    .expect("gate");
    assert!(outcome.kept.is_empty());
    assert_eq!(outcome.rejected.len(), 1);
    assert_eq!(
        outcome.rejected[0].fact,
        "probe is exported from src/cli.ts."
    );
    assert!(!outcome.inert);
}

#[test]
fn a_claim_that_merely_names_a_parameter_is_kept() {
    let outcome = derivability::gate(
        vec![claim("probe rejects a missing project root with a diagnostic.")],
        "veridoc-fakeplugin",
        ALL_CAPS,
        |_| Ok(facts()),
    )
    .expect("gate");
    assert_eq!(outcome.kept.len(), 1);
    assert!(outcome.rejected.is_empty());
}

#[test]
fn the_ban_is_inert_without_the_capability() {
    let outcome = derivability::gate(vec![claim("probe is exported from src/cli.ts.")], "p", &[], |_| Ok(facts()))
        .expect("gate");
    assert_eq!(outcome.kept.len(), 1, "the inert ban rejects nothing");
    assert!(outcome.rejected.is_empty());
    assert!(outcome.inert);
}

#[test]
fn a_plugin_error_is_a_run_failure_not_a_decision() {
    let result = derivability::gate(
        vec![claim("whatever")],
        "p",
        ALL_CAPS,
        |_| Err(veridoc_engine::plugin::LostReason::Eof),
    );
    assert!(result.is_err(), "a lost plugin must stop the run");
}

#[test]
fn normalization_lowercases_drops_punctuation_and_collapses_whitespace() {
    assert_eq!(
        normalize("  Probe   takes one `dir`: a STRING!!  "),
        "probe takes one dir a string"
    );
}
