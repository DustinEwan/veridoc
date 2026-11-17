//! The citation channel: one batched `cite` call, an unresolved citation blocks with the
//! plugin's reason, unreadable text supports with the residue flag and a warning, and a
//! transport error is a run failure and never a soft pass.

use veridoc_engine::bind::{BindError, BoundChannel, BoundClaim};
use veridoc_engine::cite::{self, CiteOutcome};
use veridoc_engine::execute::Outcome;
use veridoc_protocol::{Call, Loss};
use veridoc_protocol::{CiteResult, Citation, Kind};

fn citation_claim(store: &str, id: &str) -> BoundClaim {
    BoundClaim {
        id: "c111111111111111b".into(),
        kind: Kind::Because,
        concept: "project-scan".into(),
        predicate: "walks-upward-before-failing".into(),
        text: "The scan walks upward because a nested invocation must reuse the root.".into(),
        file: "src/cli.doc.ts".into(),
        line: 20,
        supports: vec![],
        referent_id: "vd:r:probe".into(),
        signature: "function probe(dir: string): number".into(),
        channel: BoundChannel::Citation {
            store: store.into(),
            id: id.into(),
            turn: Some(12),
        },
    }
}

fn cite_ok(resolved: Vec<(String, String, Option<String>)>, unresolved: Vec<(String, String)>) -> impl Fn(&[Citation]) -> Call<CiteResult> {
    move |_| {
        Call::Result(CiteResult {
            resolved: resolved
                .iter()
                .map(|(id, text, state)| veridoc_protocol::ResolvedCitation {
                    id: id.clone(),
                    text: text.clone(),
                    state: state.clone(),
                })
                .collect(),
            unresolved: unresolved
                .iter()
                .map(|(id, reason)| veridoc_protocol::UnresolvedCitation {
                    id: id.clone(),
                    reason: reason.clone(),
                })
                .collect(),
        })
    }
}

fn supported(_: &BoundClaim, _: &str) -> Outcome {
    Outcome::Supported
}

#[test]
fn a_readable_citation_is_judged_and_supports() {
    let outcome = cite::cite_all(
        &[citation_claim("session", "01a1e512")],
        cite_ok(vec![("01a1e512".into(), "we reuse the first root".into(), None)], vec![]),
        supported,
    )
    .expect("cite");
    assert_eq!(outcome.cited.len(), 1);
    assert_eq!(outcome.cited[0].outcome, Outcome::Supported);
    assert!(!outcome.cited[0].residue);
    assert!(outcome.errors.is_empty());
    assert!(outcome.warnings.is_empty());
}

#[test]
fn an_unresolved_citation_blocks_with_the_plugins_reason() {
    let outcome = cite::cite_all(
        &[citation_claim("tickets", "42")],
        cite_ok(vec![], vec![("42".into(), "ticket does not exist".into())]),
        supported,
    )
    .expect("cite");
    assert!(outcome.cited.is_empty());
    assert_eq!(
        outcome.errors[0].diagnostic(),
        "src/cli.doc.ts:20: citation tickets:42 does not resolve: ticket does not exist"
    );
    assert!(matches!(outcome.errors[0], BindError::CitationUnresolved { .. }));
}

#[test]
fn unreadable_text_supports_with_the_residue_flag_and_warns() {
    let outcome = cite::cite_all(
        &[citation_claim("session", "01a1e512")],
        cite_ok(vec![("01a1e512".into(), "   ".into(), None)], vec![]),
        |_, _| panic!("unreadable text must not reach the judge"),
    )
    .expect("cite");
    assert_eq!(outcome.cited[0].outcome, Outcome::Supported);
    assert!(outcome.cited[0].residue, "unreadable text is the residue metric");
    assert_eq!(
        outcome.warnings[0].message,
        "claim c111111111111111b cites session:01a1e512, whose text could not be read"
    );
}

#[test]
fn a_non_terminal_state_warns_without_failing_the_run() {
    let outcome = cite::cite_all(
        &[citation_claim("tickets", "42")],
        cite_ok(
            vec![("42".into(), "reuse the first root".into(), Some("open".into()))],
            vec![],
        ),
        supported,
    )
    .expect("cite");
    assert_eq!(outcome.cited[0].outcome, Outcome::Supported);
    assert_eq!(
        outcome.warnings[0].message,
        "citation tickets:42 is in state open; the decision it justifies has shipped"
    );
}

#[test]
fn a_transport_error_is_a_run_failure_not_a_soft_pass() {
    let result = cite::cite_all(
        &[citation_claim("session", "01a1e512")],
        |_| Call::Lost(Loss::Eof),
        supported,
    );
    match result {
        Err(detail) => assert!(detail.starts_with("citation store")),
        Ok(CiteOutcome { .. }) => panic!("a lost plugin must stop the run"),
    }
}

#[test]
fn a_citation_the_plugin_never_answered_is_a_run_failure() {
    let result = cite::cite_all(
        &[citation_claim("session", "01a1e512")],
        cite_ok(vec![], vec![]),
        supported,
    );
    match result {
        Err(detail) => assert!(
            detail.contains("01a1e512 was not answered"),
            "an unanswered citation must not pass softly: {detail}"
        ),
        Ok(_) => panic!("an unanswered citation must not pass softly"),
    }
}

#[test]
fn every_citation_shares_one_call() {
    let calls = std::cell::RefCell::new(0usize);
    let outcome = cite::cite_all(
        &[
            citation_claim("session", "a1"),
            citation_claim("session", "b2"),
        ],
        |requests| {
            *calls.borrow_mut() += 1;
            assert_eq!(requests.len(), 2, "both citations travel in one request");
            Call::Result(CiteResult {
                resolved: requests
                    .iter()
                    .map(|r| veridoc_protocol::ResolvedCitation {
                        id: r.id.clone(),
                        text: "text".into(),
                        state: None,
                    })
                    .collect(),
                unresolved: vec![],
            })
        },
        supported,
    )
    .expect("cite");
    assert_eq!(calls.into_inner(), 1);
    assert_eq!(outcome.cited.len(), 2);
}
