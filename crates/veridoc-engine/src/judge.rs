//! The semantic stage: three bounded tasks over a brief, each answered by a model that the
//! engine calls itself. Every `JudgeError` becomes a run failure in the caller — the semantic
//! stage either answers or the build is red, and there is no path from a judge error to a
//! claim state (`docs/decisions/0005`).
//!
//! The model is chosen by configuration, not by the project: `provider` selects the wire and
//! the endpoint, `model` names the model on it, and the credential comes from the environment.
//! `rig-core` speaks every provider's chat wire, so the engine holds no provider code and no
//! project ships a judge program.

use std::time::Duration;

use rig_core::completion::CompletionRequest;
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

/// The wire a provider speaks, and the endpoint and credential its name implies. Every
/// provider reachable here is one the author can pick in `.veridoc/config.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    /// OpenAI's Chat Completions API, or any server that speaks it: the `baseUrl` field
    /// points at a local or self-hosted one (radiance, vLLM, LM Studio, an Ingress in front
    /// of a fleet) without changing anything else about the run.
    OpenAI,
    Anthropic,
    Gemini,
    /// A local Ollama daemon, which needs no credential.
    Ollama,
    Groq,
    Xai,
    DeepSeek,
    OpenRouter,
    Mistral,
}

/// Which chat wire a [`Provider`] speaks. The four dialects cover every provider above: the
/// OpenAI-compatible ones differ only by endpoint and credential variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dialect {
    OpenAiChat,
    AnthropicMessages,
    GeminiGenerate,
    OllamaChat,
}

impl Provider {
    /// The provider named in the config, or `None` when the name is not one this build
    /// speaks. The error text lists the accepted names because the author is the reader.
    pub fn parse(name: &str) -> Result<Self, String> {
        let found = match name {
            "openai" => Provider::OpenAI,
            "anthropic" => Provider::Anthropic,
            "gemini" => Provider::Gemini,
            "ollama" => Provider::Ollama,
            "groq" => Provider::Groq,
            "xai" => Provider::Xai,
            "deepseek" => Provider::DeepSeek,
            "openrouter" => Provider::OpenRouter,
            "mistral" => Provider::Mistral,
            other => {
                return Err(format!(
                    "judge.provider {other} is not one of {}",
                    Provider::names()
                ))
            }
        };
        Ok(found)
    }

    /// The accepted `judge.provider` values, in the rejection `parse` returns.
    pub fn names() -> &'static str {
        "openai, anthropic, gemini, ollama, groq, xai, deepseek, openrouter, mistral"
    }

    fn dialect(self) -> Dialect {
        match self {
            Provider::OpenAI | Provider::Groq | Provider::Xai | Provider::DeepSeek => {
                Dialect::OpenAiChat
            }
            Provider::OpenRouter | Provider::Mistral => Dialect::OpenAiChat,
            Provider::Anthropic => Dialect::AnthropicMessages,
            Provider::Gemini => Dialect::GeminiGenerate,
            Provider::Ollama => Dialect::OllamaChat,
        }
    }

    /// The endpoint used when `judge.baseUrl` is absent.
    pub fn default_base_url(self) -> &'static str {
        match self {
            Provider::OpenAI => "https://api.openai.com/v1",
            Provider::Anthropic => "https://api.anthropic.com/v1",
            Provider::Gemini => "https://generativelanguage.googleapis.com/v1beta",
            Provider::Ollama => "http://localhost:11434",
            Provider::Groq => "https://api.groq.com/openai/v1",
            Provider::Xai => "https://api.x.ai/v1",
            Provider::DeepSeek => "https://api.deepseek.com/v1",
            Provider::OpenRouter => "https://openrouter.ai/api/v1",
            Provider::Mistral => "https://api.mistral.ai/v1",
        }
    }

    /// The environment variable holding the credential. `None` is a provider that needs
    /// none, which is why `apiKeyEnv` exists: a deployment that keeps the credential under
    /// another name sets it there.
    pub fn default_key_env(self) -> Option<&'static str> {
        match self {
            Provider::OpenAI => Some("OPENAI_API_KEY"),
            Provider::Anthropic => Some("ANTHROPIC_API_KEY"),
            Provider::Gemini => Some("GEMINI_API_KEY"),
            Provider::Ollama => None,
            Provider::Groq => Some("GROQ_API_KEY"),
            Provider::Xai => Some("XAI_API_KEY"),
            Provider::DeepSeek => Some("DEEPSEEK_API_KEY"),
            Provider::OpenRouter => Some("OPENROUTER_API_KEY"),
            Provider::Mistral => Some("MISTRAL_API_KEY"),
        }
    }
}

/// The judge: one model, reached over its provider's chat wire. The three tasks differ only
/// in the prompt they send, so the transport, the timeout, and the reply reading are shared.
pub struct ModelJudge {
    pub provider: Provider,
    pub model: String,
    pub base_url: String,
    /// The credential, already read from the environment by the caller so that a missing
    /// key is reported with the variable's name rather than at the fourth model call.
    pub api_key: Option<String>,
    pub max_tokens: u64,
    /// Merged into the request body as-is: the provider-specific fields a deployment needs,
    /// such as a chat template's `{"enable_thinking": true}`.
    pub additional_params: Value,
    pub timeout: Duration,
}

impl ModelJudge {
    fn ask(&self, task: Task, brief: &Brief) -> Result<Value, JudgeError> {
        let mut request = CompletionRequest::new(brief.render())
            .preamble(prompt(task))
            .temperature(0.0)
            .max_tokens(self.max_tokens);
        if !self.additional_params.is_null() {
            request = request.additional_params(self.additional_params.clone());
        }
        let text = self.send(request)?;
        // The prompt asks for JSON only; a model that wraps the object in prose or a fence
        // is still read, because the object itself is delimited.
        let start = text
            .find('{')
            .ok_or_else(|| JudgeError::BadVerdict(format!("no json object in {text}")))?;
        let end = text
            .rfind('}')
            .ok_or_else(|| JudgeError::BadVerdict(format!("no json object in {text}")))?;
        serde_json::from_str(&text[start..=end])
            .map_err(|e| JudgeError::BadVerdict(format!("{e}: {text}")))
    }

    /// One model call, driven to completion on a current-thread runtime so the engine's
    /// synchronous pipeline needs no async context of its own. The timeout is the engine's,
    /// not the provider's, so it applies to a server that accepts the connection and never
    /// answers — the case a reasoning model makes routine.
    fn send(&self, request: CompletionRequest) -> Result<String, JudgeError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| JudgeError::Unreachable(format!("judge runtime: {e}")))?;
        let dialect = self.provider.dialect();
        let key = self.credential()?;
        let base = self.base_url.clone();
        let model = self.model.clone();
        let timeout = self.timeout;
        // Each arm awaits inside the arm: the four wires are different request types, so the
        // futures cannot share one binding, but their answers can. The timeout covers the
        // whole call, including a server that accepts the connection and never replies.
        let answered = runtime.block_on(async move {
            match dialect {
                Dialect::OpenAiChat => {
                    let client = rig_core::providers::openai::OpenAIConfig::new(key)
                        .with_base_url(base)
                        .client();
                    tokio::time::timeout(timeout, client.chat(model).call(request)).await
                }
                Dialect::AnthropicMessages => {
                    let client = rig_core::providers::anthropic::AnthropicConfig::new(key)
                        .with_base_url(base)
                        .client();
                    tokio::time::timeout(timeout, client.completion(model).call(request)).await
                }
                Dialect::GeminiGenerate => {
                    let client = rig_core::providers::gemini::GeminiConfig::new(key).client();
                    tokio::time::timeout(timeout, client.completion(model).call(request)).await
                }
                Dialect::OllamaChat => {
                    let client = rig_core::providers::ollama::OllamaConfig::new()
                        .with_base_url(base)
                        .client();
                    tokio::time::timeout(timeout, client.completion(model).call(request)).await
                }
            }
        });
        match answered {
            Err(_) => Err(JudgeError::Timeout),
            Ok(Ok(reply)) => Ok(reply.text()),
            Ok(Err(e)) => Err(JudgeError::Unreachable(e.to_string())),
        }
    }

    /// The credential to authenticate with. A provider that needs one and cannot find it is
    /// an unreachable judge naming the variable, because a run that silently sends no
    /// credential produces an authentication error the author misreads as a bad key.
    fn credential(&self) -> Result<String, JudgeError> {
        if let Some(key) = &self.api_key {
            return Ok(key.clone());
        }
        match self.provider.default_key_env() {
            Some(variable) => Err(JudgeError::Unreachable(format!(
                "{variable} is not set, and judge.provider \"{}\" needs it",
                self.provider_name()
            ))),
            None => Ok(String::new()),
        }
    }

    /// The provider's config name, for error text.
    pub fn provider_name(&self) -> &'static str {
        match self.provider {
            Provider::OpenAI => "openai",
            Provider::Anthropic => "anthropic",
            Provider::Gemini => "gemini",
            Provider::Ollama => "ollama",
            Provider::Groq => "groq",
            Provider::Xai => "xai",
            Provider::DeepSeek => "deepseek",
            Provider::OpenRouter => "openrouter",
            Provider::Mistral => "mistral",
        }
    }
}

impl Judge for ModelJudge {
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
