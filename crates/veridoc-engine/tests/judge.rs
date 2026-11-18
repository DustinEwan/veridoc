//! The command judge against a scripted fixture: a verdict citing a span the brief never
//! issued is a run failure that leaves no claim state, and transport faults map to
//! `Unreachable` and `BadVerdict`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use veridoc_engine::brief::{Brief, BriefSpan, Task};
use veridoc_engine::judge::{CommandJudge, Judge, JudgeError};
use veridoc_engine::verdicts;

fn tmp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "veridoc-engine-judge-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// A judge whose whole behaviour is `cat` of a fixture file.
fn scripted(root: &Path, file: &str, verdict_json: &str) -> CommandJudge {
    std::fs::write(root.join(file), verdict_json).unwrap();
    CommandJudge {
        command: vec![
            "sh".into(),
            "-c".into(),
            format!("cat {}", root.join(file).display()),
        ],
        cwd: None,
        env: Vec::new(),
        model: Some("scripted".into()),
        prompt_version: "veridoc-prompt-1".into(),
        timeout: Duration::from_secs(10),
    }
}

fn brief_with_spans(root: &Path) -> Brief {
    let _ = root;
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

#[test]
fn a_verdict_citing_an_unissued_span_is_a_run_failure() {
    let root = tmp_dir("badid");
    let judge = scripted(
        &root,
        "verdict.json",
        r#"{"unmentionedClaims": [], "unstatedAssertions": [{"span": "sp9", "text": "an invented span"}]}"#,
    );
    let brief = brief_with_spans(&root);
    let verdict = judge.s1(&brief).expect("the judge answers");
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
    let root = tmp_dir("goodid");
    let judge = scripted(
        &root,
        "verdict.json",
        r#"{"unmentionedClaims": [], "unstatedAssertions": [{"span": "sp2", "text": "a stated span"}]}"#,
    );
    let brief = brief_with_spans(&root);
    let verdict = judge.s1(&brief).expect("the judge answers");
    verdicts::verify_ids(
        Task::S1,
        &serde_json::to_value(&verdict).unwrap(),
        &brief.issued_ids(),
    )
    .expect("every cited id was issued");
}

#[test]
fn a_nonzero_exit_is_unreachable_with_the_child_stderr() {
    let root = tmp_dir("exit");
    let judge = CommandJudge {
        command: vec!["sh".into(), "-c".into(), "echo boom >&2; exit 3".into()],
        cwd: None,
        env: Vec::new(),
        model: None,
        prompt_version: "veridoc-prompt-1".into(),
        timeout: Duration::from_secs(10),
    };
    let err = judge.s1(&brief_with_spans(&root)).expect_err("exit 3");
    match err {
        JudgeError::Unreachable(detail) => {
            assert!(detail.contains("exited with 3"), "{detail}");
            assert!(detail.contains("boom"), "{detail}");
        }
        other => panic!("expected Unreachable, got {other:?}"),
    }
}

#[test]
fn an_unparseable_answer_is_a_bad_verdict() {
    let root = tmp_dir("bad");
    let judge = scripted(&root, "verdict.txt", "this is not json");
    let err = judge.s1(&brief_with_spans(&root)).expect_err("not json");
    assert!(matches!(err, JudgeError::BadVerdict(_)), "got {err:?}");
}

#[test]
fn a_silent_judge_times_out() {
    let root = tmp_dir("slow");
    let judge = CommandJudge {
        command: vec!["sh".into(), "-c".into(), "sleep 5".into()],
        cwd: None,
        env: Vec::new(),
        model: None,
        prompt_version: "veridoc-prompt-1".into(),
        timeout: Duration::from_millis(200),
    };
    let err = judge
        .s1(&brief_with_spans(&root))
        .expect_err("sleeps past the deadline");
    assert!(matches!(err, JudgeError::Timeout), "got {err:?}");
}

#[test]
fn the_request_carries_task_prompt_brief_version_and_model() {
    let root = tmp_dir("request");
    // The scripted judge echoes its stdin back, so the fixture is the request itself.
    let request_path = root.join("request.json").to_string_lossy().into_owned();
    std::fs::write(
        root.join("echo.sh"),
        format!("IFS= read -r line; printf '%s\\n' \"$line\" > {request_path}; echo '{{\"unmentionedClaims\": [], \"unstatedAssertions\": []}}'\n"),
    )
    .unwrap();
    let judge = CommandJudge {
        command: vec![
            "sh".into(),
            root.join("echo.sh").to_string_lossy().into_owned(),
        ],
        cwd: None,
        env: Vec::new(),
        prompt_version: "veridoc-prompt-9".into(),
        model: Some("scripted".into()),
        timeout: Duration::from_secs(10),
    };
    judge.s1(&brief_with_spans(&root)).expect("answers");
    let request: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("request.json")).unwrap()).unwrap();
    assert_eq!(request["task"], "s1");
    assert!(request["prompt"]
        .as_str()
        .unwrap()
        .contains("S1 coverage judge"));
    assert!(request["brief"]
        .as_str()
        .unwrap()
        .contains("sp1: The scanner walks upward."));
    assert_eq!(request["promptVersion"], "veridoc-prompt-9");
    assert_eq!(request["model"], "scripted");
}
