//! The semantic stage: three bounded tasks over a brief, and the two transports that answer
//! them. Every `JudgeError` becomes a run failure in the caller — the semantic stage either
//! answers or the build is red, and there is no path from a judge error to a claim state
//! (`docs/decisions/0005`).

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde_json::Value;
use veridoc_protocol::{S1Verdict, S2Verdict, S3Verdict};

use crate::brief::{Brief, Task};

pub trait Judge {
    fn s1(&self, brief: &Brief) -> Result<S1Verdict, JudgeError>;
    fn s2(&self, brief: &Brief) -> Result<S2Verdict, JudgeError>;
    fn s3(&self, brief: &Brief) -> Result<S3Verdict, JudgeError>;
}

#[derive(Debug, thiserror::Error)]
pub enum JudgeError {
    #[error("judge unreachable: {0}")]
    Unreachable(String),
    #[error("judge verdict malformed: {0}")]
    BadVerdict(String),
    #[error("judge timed out")]
    Timeout,
}

/// The prompt for one task, shipped inside the binary so a run needs no install tree.
pub fn prompt(task: Task) -> &'static str {
    match task {
        Task::S1 => include_str!("../prompts/s1.md"),
        Task::S2 => include_str!("../prompts/s2.md"),
        Task::S3 => include_str!("../prompts/s3.md"),
    }
}

/// A judge that runs an external command: the request JSON goes to its stdin as one line,
/// one JSON line comes back on stdout. A non-zero exit or an unparseable line is
/// `Unreachable` or `BadVerdict` with the child's stderr appended.
pub struct CommandJudge {
    pub command: Vec<String>,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
    pub model: Option<String>,
    pub prompt_version: String,
    pub timeout: Duration,
}

impl CommandJudge {
    fn ask(&self, task: Task, brief: &Brief) -> Result<Value, JudgeError> {
        if self.command.is_empty() {
            return Err(JudgeError::Unreachable(
                "judge.command is empty".to_string(),
            ));
        }
        let request = serde_json::json!({
            "task": task.name(),
            "prompt": prompt(task),
            "brief": brief.render(),
            "promptVersion": self.prompt_version,
            "model": self.model,
        });

        let mut spawn = Command::new(&self.command[0]);
        spawn
            .args(&self.command[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .envs(self.env.iter().map(|(key, value)| (key, value)));
        if let Some(cwd) = &self.cwd {
            spawn.current_dir(cwd);
        }
        let mut child = spawn.spawn().map_err(|e| {
            JudgeError::Unreachable(format!("{} could not be spawned: {e}", self.command[0]))
        })?;

        let mut stdin = child.stdin.take().expect("piped stdin");
        let line = serde_json::to_string(&request).expect("request is valid json");
        if let Err(e) = writeln!(stdin, "{line}").and_then(|()| stdin.flush()) {
            // The judge died before reading its request; the exit status says so below.
            if e.kind() != std::io::ErrorKind::BrokenPipe {
                return Err(JudgeError::Unreachable(format!(
                    "writing the request failed: {e}"
                )));
            }
        }
        drop(stdin);

        // Both pipes are drained on their own threads so a chatty judge cannot deadlock on
        // a full pipe while the engine waits for the other one.
        let stdout = child.stdout.take().expect("piped stdout");
        let (tx, rx) = mpsc::channel::<Option<String>>();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut answer = String::new();
            let first = match reader.read_line(&mut answer) {
                Ok(0) | Err(_) => None,
                Ok(_) => Some(answer.trim_end().to_string()),
            };
            let _ = tx.send(first);
        });
        let stderr = child.stderr.take().expect("piped stderr");
        let (etx, erx) = mpsc::channel::<String>();
        thread::spawn(move || {
            let mut text = String::new();
            let mut reader = BufReader::new(stderr);
            let _ = reader.read_to_string(&mut text);
            let _ = etx.send(text);
        });

        let answer = match rx.recv_timeout(self.timeout) {
            Ok(answer) => answer,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(JudgeError::Timeout);
            }
        };
        let status = child
            .wait()
            .map_err(|e| JudgeError::Unreachable(format!("waiting failed: {e}")))?;
        let stderr = erx
            .recv_timeout(Duration::from_millis(500))
            .unwrap_or_default();

        if !status.success() {
            return Err(JudgeError::Unreachable(format!(
                "judge command exited with {}: {}",
                status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "a signal".to_string()),
                stderr.trim()
            )));
        }
        let Some(answer) = answer else {
            return Err(JudgeError::Unreachable(format!(
                "judge command closed stdout without answering: {}",
                stderr.trim()
            )));
        };
        serde_json::from_str(&answer).map_err(|e| {
            JudgeError::BadVerdict(format!("{e}: {}{}", answer.trim(), formatted(&stderr)))
        })
    }
}

fn formatted(stderr: &str) -> String {
    let stderr = stderr.trim();
    if stderr.is_empty() {
        String::new()
    } else {
        format!("; stderr: {stderr}")
    }
}

impl Judge for CommandJudge {
    fn s1(&self, brief: &Brief) -> Result<S1Verdict, JudgeError> {
        let value = self.ask(Task::S1, brief)?;
        serde_json::from_value(value).map_err(|e| JudgeError::BadVerdict(e.to_string()))
    }
    fn s2(&self, brief: &Brief) -> Result<S2Verdict, JudgeError> {
        let value = self.ask(Task::S2, brief)?;
        serde_json::from_value(value).map_err(|e| JudgeError::BadVerdict(e.to_string()))
    }
    fn s3(&self, brief: &Brief) -> Result<S3Verdict, JudgeError> {
        let value = self.ask(Task::S3, brief)?;
        serde_json::from_value(value).map_err(|e| JudgeError::BadVerdict(e.to_string()))
    }
}

/// The Anthropic Messages transport: the prompt is the system text and the brief is the
/// user text; the verdict is the single JSON object in the reply's text block.
pub struct AnthropicJudge {
    pub model: String,
    pub prompt_version: String,
    pub timeout: Duration,
}

impl AnthropicJudge {
    fn ask(&self, task: Task, brief: &Brief) -> Result<Value, JudgeError> {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| JudgeError::Unreachable("ANTHROPIC_API_KEY is not set".to_string()))?;
        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 4096,
            "system": prompt(task),
            "messages": [{ "role": "user", "content": brief.render() }],
        });
        let client = reqwest::blocking::Client::builder()
            .timeout(self.timeout)
            .build()
            .map_err(|e| JudgeError::Unreachable(e.to_string()))?;
        let response = client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .map_err(|e| JudgeError::Unreachable(e.to_string()))?;
        let status = response.status();
        let body: Value = response
            .json()
            .map_err(|e| JudgeError::BadVerdict(format!("response was not json: {e}")))?;
        if !status.is_success() {
            return Err(JudgeError::Unreachable(format!(
                "anthropic returned {status}: {body}"
            )));
        }
        let text = body["content"][0]["text"]
            .as_str()
            .ok_or_else(|| JudgeError::BadVerdict(format!("no text block in {body}")))?;
        // The model may wrap the object in prose or fences; the object itself is delimited.
        let start = text
            .find('{')
            .ok_or_else(|| JudgeError::BadVerdict(format!("no json object in {text}")))?;
        let end = text
            .rfind('}')
            .ok_or_else(|| JudgeError::BadVerdict(format!("no json object in {text}")))?;
        serde_json::from_str(&text[start..=end])
            .map_err(|e| JudgeError::BadVerdict(format!("{e}: {text}")))
    }
}

impl Judge for AnthropicJudge {
    fn s1(&self, brief: &Brief) -> Result<S1Verdict, JudgeError> {
        let value = self.ask(Task::S1, brief)?;
        serde_json::from_value(value).map_err(|e| JudgeError::BadVerdict(e.to_string()))
    }
    fn s2(&self, brief: &Brief) -> Result<S2Verdict, JudgeError> {
        let value = self.ask(Task::S2, brief)?;
        serde_json::from_value(value).map_err(|e| JudgeError::BadVerdict(e.to_string()))
    }
    fn s3(&self, brief: &Brief) -> Result<S3Verdict, JudgeError> {
        let value = self.ask(Task::S3, brief)?;
        serde_json::from_value(value).map_err(|e| JudgeError::BadVerdict(e.to_string()))
    }
}
