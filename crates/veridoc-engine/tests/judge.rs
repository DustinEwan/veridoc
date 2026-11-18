//! The model judge against a mock provider: the verdict's cited spans are re-verified before
//! the engine keeps them, and the transport faults map to `Unreachable`, `BadVerdict`, and
//! `Timeout`. The provider is an HTTP endpoint because the judge is a model the engine calls,
//! not a program it spawns (`docs/decisions/0012`).

use std::path::PathBuf;
use std::time::Duration;

use serde_json::json;
use veridoc_engine::brief::{Brief, BriefSpan, Task};
use veridoc_engine::judge::{Judge, JudgeError, ModelJudge, Provider};
use veridoc_engine::verdicts;
use veridoc_testprovider::{self as provider, Handle};

fn tmp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "veridoc-engine-judge-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// A judge pointed at `endpoint`, answering within a window short enough that the timeout
/// test finishes without waiting on a real model.
fn judge(endpoint: &Handle) -> ModelJudge {
    ModelJudge {
        provider: Provider::OpenAI,
        model: "mock-judge".into(),
        base_url: endpoint.base_url().to_string(),
        api_key: Some("test-key".into()),
        max_tokens: 4096,
        additional_params: json!({"chat_template_kwargs": {"enable_thinking": true}}),
        timeout: Duration::from_secs(10),
    }
}

fn brief_with_spans() -> Brief {
    Brief {
        task: Task::S1,
        section: "docs/architecture.spec.md#refuses".into(),
        audience: Some("an author".into()),
        question: Some("What is refused?".into()),
        spans: vec![
            BriefSpan {
                id: "sp1".into(),
                text: "The scanner walks upward.".into(),
            },
            BriefSpan {
                id: "sp2".into(),
                text: "It stops at the first root.".into(),
            },
            BriefSpan {
                id: "sp3".into(),
                text: "Nothing else is consulted.".into(),
            },
        ],
        claims: vec![],
        nodes: vec![],
    }
}

fn answering_s1(verdict: &str) -> Handle {
    provider::answering(provider::Verdicts {
        s1: verdict.to_string(),
        ..Default::default()
    })
}

#[test]
fn a_verdict_citing_an_unissued_span_is_a_run_failure() {
    let _root = tmp_dir("badid");
    let endpoint = answering_s1(
        r#"{"unmentionedClaims": [], "unstatedAssertions": [{"span": "sp9", "text": "an invented span"}]}"#,
    );
    let judge = judge(&endpoint);
    let brief = brief_with_spans();
    let verdict = judge.s1(&brief).expect("the model answers");
    let issued = brief.issued_ids();
    // The engine re-verifies every cited id before keeping or replaying the verdict.
    let failure = verdicts::verify_ids(Task::S1, &serde_json::to_value(&verdict).unwrap(), &issued)
        .expect_err("sp9 was never issued");
    assert_eq!(
        failure,
        "judge verdict for s1 cited span sp9 that this run never issued"
    );
}

#[test]
fn a_verdict_citing_only_issued_ids_is_kept() {
    let _root = tmp_dir("goodid");
    let endpoint = answering_s1(
        r#"{"unmentionedClaims": [], "unstatedAssertions": [{"span": "sp2", "text": "a stated span"}]}"#,
    );
    let brief = brief_with_spans();
    let verdict = judge(&endpoint).s1(&brief).expect("the model answers");
    verdicts::verify_ids(
        Task::S1,
        &serde_json::to_value(&verdict).unwrap(),
        &brief.issued_ids(),
    )
    .expect("every cited id was issued");
}

#[test]
fn a_provider_that_refuses_is_unreachable_with_its_status() {
    let _root = tmp_dir("refused");
    let endpoint = provider::failing(503, r#"{"error":{"message":"upstream unavailable"}}"#);
    let err = judge(&endpoint)
        .s1(&brief_with_spans())
        .expect_err("the provider refuses");
    match err {
        JudgeError::Unreachable(detail) => {
            assert!(detail.contains("503"), "{detail}");
            assert!(detail.contains("upstream unavailable"), "{detail}");
        }
        other => panic!("expected Unreachable, got {other:?}"),
    }
}

#[test]
fn a_missing_credential_names_the_variable() {
    let _root = tmp_dir("nokey");
    let endpoint = provider::answering(provider::Verdicts::default());
    let mut judge = judge(&endpoint);
    judge.api_key = None;
    let err = judge.s1(&brief_with_spans()).expect_err("no key to send");
    match err {
        JudgeError::Unreachable(detail) => assert!(
            detail.contains("OPENAI_API_KEY") && detail.contains("openai"),
            "{detail}"
        ),
        other => panic!("expected Unreachable, got {other:?}"),
    }
}

#[test]
fn an_unparseable_answer_is_a_bad_verdict() {
    let _root = tmp_dir("bad");
    let endpoint = answering_s1("this is not json");
    let err = judge(&endpoint)
        .s1(&brief_with_spans())
        .expect_err("not json");
    assert!(matches!(err, JudgeError::BadVerdict(_)), "got {err:?}");
}

#[test]
fn a_silent_judge_times_out() {
    let _root = tmp_dir("slow");
    let endpoint = provider::silent();
    let mut judge = judge(&endpoint);
    judge.timeout = Duration::from_millis(200);
    let err = judge
        .s1(&brief_with_spans())
        .expect_err("answers past the deadline");
    assert!(matches!(err, JudgeError::Timeout), "got {err:?}");
}

#[test]
fn the_request_carries_the_prompt_brief_model_and_extra_params() {
    let _root = tmp_dir("request");
    let endpoint = provider::answering(provider::Verdicts::default());
    judge(&endpoint)
        .s1(&brief_with_spans())
        .expect("the model answers");
    let sent = endpoint.requests();
    assert_eq!(sent.len(), 1, "one call per task");
    let sent_body = sent.first().expect("one request");
    let body: serde_json::Value = serde_json::from_str(sent_body).expect("a json request");
    assert_eq!(body["model"], "mock-judge");
    assert_eq!(body["max_tokens"], 4096);
    assert_eq!(body["temperature"], json!(0.0));
    assert_eq!(body["chat_template_kwargs"]["enable_thinking"], true);
    // The prompt is the system turn and the brief is the user turn, so the prompt text
    // reaches the provider ahead of the brief text however the wire encodes one turn.
    let prompt_at = sent_body
        .find("S1 coverage judge")
        .expect("the task prompt");
    let brief_at = sent_body
        .find("sp1: The scanner walks upward.")
        .expect("the brief");
    assert!(
        prompt_at < brief_at,
        "prompt at {prompt_at}, brief at {brief_at}"
    );
}

#[test]
fn each_task_sends_its_own_prompt() {
    let _root = tmp_dir("tasks");
    let endpoint = provider::answering(provider::Verdicts::default());
    let judge = judge(&endpoint);
    judge.s1(&brief_with_spans()).expect("s1 answers");
    judge.s2(&brief_with_spans()).expect("s2 answers");
    judge.s3(&brief_with_spans()).expect("s3 answers");
    let sent = endpoint.requests();
    assert_eq!(sent.len(), 3, "one call per task");
    for (index, marker) in [
        (0, "S1 coverage judge"),
        (1, "S2 contradiction judge"),
        (2, "S3 focus judge"),
    ] {
        let body = sent.get(index).expect("a request");
        assert!(
            body.contains(marker),
            "call {index} did not carry {marker}: {}",
            &body[..body.len().min(200)]
        );
        assert!(
            body.contains("sp1: The scanner walks upward."),
            "call {index}"
        );
    }
}
