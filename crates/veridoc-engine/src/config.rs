//! The project configuration: `.veridoc/config.json`, found by walking up from the project
//! directory. `#[serde(deny_unknown_fields)]` makes an unknown key a configuration error
//! rather than a silently disabled check, so a project carrying a `semanticRules` block from
//! the draft spec fails with `unknown field semanticRules` instead of ignoring it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::hash::hex64;
use crate::plugin::Launch;

pub const CONFIG_DIR: &str = ".veridoc";
pub const CONFIG_FILE_NAME: &str = "config.json";

/// The environment file a project keeps at its root: `<root>/.env`. It holds the values the
/// machine supplies and the repository must not carry, such as the address of a local model
/// server. Git ignores it by convention, and the input record never covers it, so a run on a
/// developer's machine and a gate in continuous integration read the same configuration file.
pub const DOTENV_FILE: &str = ".env";

/// Validation and user-facing failure messages share these strings, so a test can assert the
/// exact wording.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{0}")]
    Message(String),
    #[error("cannot read {}: {source}", path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot parse {}: {source}", path.display())]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
}

impl ConfigError {
    pub(crate) fn msg(m: impl Into<String>) -> ConfigError {
        ConfigError::Message(m.into())
    }
}

/// The plugin the run drives, and the two fixed deadlines that bound its start-up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct PluginConfig {
    /// The installed-plugin name, resolved through the registry written by
    /// `veridoc install-plugin`. `command` wins when both are present.
    pub name: Option<String>,
    pub command: Option<Vec<String>>,
    pub handshake_timeout_ms: u64,
    pub collect_timeout_ms: u64,
}

impl Default for PluginConfig {
    fn default() -> Self {
        PluginConfig {
            name: None,
            command: None,
            handshake_timeout_ms: 30_000,
            collect_timeout_ms: 120_000,
        }
    }
}

/// The coverage figure the run compares the achieved figure against.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct CoverageConfig {
    pub required: f64,
}

impl Default for CoverageConfig {
    fn default() -> Self {
        CoverageConfig { required: 0.0 }
    }
}

/// The one timer of a scenario: a slow machine is not blamed on the prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct RunConfig {
    pub scenario_deadline_ms: u64,
}

impl Default for RunConfig {
    fn default() -> Self {
        RunConfig {
            scenario_deadline_ms: 30_000,
        }
    }
}

/// The judge: one model the engine calls over its provider's chat wire. `provider` selects
/// the wire, the endpoint, and the credential variable; `baseUrl` redirects an
/// OpenAI-compatible server (radiance, vLLM, LM Studio, a gateway) without changing anything
/// else about the run; `additionalParams` merges provider-specific fields into the request
/// body. There is no judge program anywhere in this design: a judge is a model, and the
/// engine reaches it (`docs/decisions/0012`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct JudgeConfig {
    pub provider: String,
    pub model: String,
    pub base_url: Option<String>,
    /// The variable holding the credential, for a deployment that keeps it under a name
    /// other than the provider's usual one.
    pub api_key_env: Option<String>,
    pub prompt_version: String,
    /// The window for one model call. A reasoning model spends most of it thinking, so the
    /// default is generous, and a call that exceeds it is a run failure rather than a check
    /// that did not run.
    pub timeout_ms: u64,
    pub max_tokens: u64,
    pub additional_params: Option<Value>,
}

impl Default for JudgeConfig {
    fn default() -> Self {
        JudgeConfig {
            provider: String::new(),
            model: String::new(),
            base_url: None,
            api_key_env: None,
            prompt_version: "veridoc-prompt-1".to_string(),
            timeout_ms: 900_000,
            max_tokens: 32_000,
            additional_params: None,
        }
    }
}

/// Where the formatters write and which run. A bare string is a built-in renderer; an object
/// `{ "name": ..., "command": [...] }` runs an external program with `VERIDOC_RESULT` and
/// `VERIDOC_OUT` in its environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct OutputConfig {
    pub dir: String,
    pub formatters: Vec<Value>,
}

impl Default for OutputConfig {
    fn default() -> Self {
        OutputConfig {
            dir: ".veridoc/out".to_string(),
            formatters: vec![Value::String("markdown".to_string())],
        }
    }
}

/// One resolved formatter entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Formatter {
    Builtin(String),
    External { name: String, command: Vec<String> },
}

/// A citation store a `because` claim may name. `kind` selects how the plugin reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CitationStoreConfig {
    pub name: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// The project configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct Config {
    pub protocol: String,
    pub plugin: PluginConfig,
    pub coverage: CoverageConfig,
    pub run: RunConfig,
    /// Absent means the semantic stage does not run. The deterministic checks still decide
    /// every claim they can, and a claim that needs a model's judgement is reported as
    /// unevaluated rather than passed.
    pub judge: Option<JudgeConfig>,
    pub output: OutputConfig,
    pub citation_stores: Vec<CitationStoreConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            protocol: veridoc_protocol::PROTOCOL_VERSION.to_string(),
            plugin: PluginConfig::default(),
            coverage: CoverageConfig::default(),
            run: RunConfig::default(),
            judge: None,
            output: OutputConfig::default(),
            citation_stores: Vec::new(),
        }
    }
}

impl Config {
    /// Every rule a plugin-driving command depends on, the plugin launch included.
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.validate_record()?;
        self.validate_plugin()
    }

    /// Every rule except the plugin launch. `veridoc stat` starts no plugin, so a project whose
    /// plugin is named but not installed still gets its record compared: the gate for a job that
    /// cannot call a model must not need a job that can build a plugin (`docs/decisions/0014`).
    pub fn validate_record(&self) -> Result<(), ConfigError> {
        if self.protocol != veridoc_protocol::PROTOCOL_VERSION {
            return Err(ConfigError::msg(format!(
                "protocol \"{}\" is not supported; this build speaks \"{}\"",
                self.protocol,
                veridoc_protocol::PROTOCOL_VERSION
            )));
        }
        if !(0.0..=1.0).contains(&self.coverage.required) {
            return Err(ConfigError::msg(
                "coverage.required must be between 0.0 and 1.0",
            ));
        }
        if self.run.scenario_deadline_ms == 0 {
            return Err(ConfigError::msg(
                "run.scenarioDeadlineMs must be greater than zero",
            ));
        }
        if let Some(judge) = &self.judge {
            crate::judge::Provider::parse(&judge.provider).map_err(ConfigError::msg)?;
            if judge.model.is_empty() {
                return Err(ConfigError::msg("judge.model is required"));
            }
            if judge.timeout_ms == 0 {
                return Err(ConfigError::msg(
                    "judge.timeoutMs must be greater than zero",
                ));
            }
            if judge.max_tokens == 0 {
                return Err(ConfigError::msg(
                    "judge.maxTokens must be greater than zero",
                ));
            }
        }
        self.formatters()?;
        let mut seen = Vec::new();
        for store in &self.citation_stores {
            if seen.contains(&store.name.as_str()) {
                return Err(ConfigError::msg(format!(
                    "citation store \"{}\" is declared twice",
                    store.name
                )));
            }
            seen.push(store.name.as_str());
        }
        Ok(())
    }

    /// The plugin can be launched: a `command` in the config, or a name the registry holds.
    fn validate_plugin(&self) -> Result<(), ConfigError> {
        if self.plugin.command.as_deref().unwrap_or(&[]).is_empty() {
            // A missing `command` is only legal when the name is installed: the registry
            // holds argv the user installed on purpose, so the engine never guesses a
            // runtime, and an uninstalled name fails at load, not at the first spawn.
            let name = self.plugin.name.as_deref().unwrap_or("");
            if name.is_empty() {
                return Err(ConfigError::msg(
                    "plugin.command is required, or plugin.name must name an installed plugin",
                ));
            }
            if crate::plugins::lookup(name).is_none() {
                return Err(ConfigError::msg(format!(
                    "`command` is required for plugin \"{name}\". Install it with `veridoc install-plugin <git-url>`, or set `plugin.command`."
                )));
            }
        }
        Ok(())
    }

    /// The formatter entries in config order, with the exact rejection messages.
    pub fn formatters(&self) -> Result<Vec<Formatter>, ConfigError> {
        let mut out = Vec::new();
        for entry in &self.output.formatters {
            match entry {
                Value::String(name) => {
                    if name == "markdown" || name == "json" {
                        out.push(Formatter::Builtin(name.clone()));
                    } else {
                        return Err(ConfigError::msg(format!(
                            "output.formatters names \"{name}\"; the built-in formatters are \"markdown\" and \"json\""
                        )));
                    }
                }
                Value::Object(map) => {
                    let name = map.get("name").and_then(Value::as_str);
                    let command = map.get("command").and_then(Value::as_array).map(|arr| {
                        arr.iter()
                            .map(|v| v.as_str().map(str::to_string))
                            .collect::<Option<Vec<String>>>()
                    });
                    match (name, command) {
                        (Some(name), Some(Some(command))) if !command.is_empty() => {
                            out.push(Formatter::External {
                                name: name.to_string(),
                                command,
                            });
                        }
                        _ => {
                            return Err(ConfigError::msg(
                                "output.formatters entry must be a string or an object with name and command",
                            ))
                        }
                    }
                }
                _ => return Err(ConfigError::msg(
                    "output.formatters entry must be a string or an object with name and command",
                )),
            }
        }
        Ok(out)
    }
}

/// Where a loaded config came from.
#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub root: PathBuf,
    pub path: PathBuf,
    pub config: Config,
}

impl LoadedConfig {
    /// `sha256:<hex>` over the config file bytes, recorded in the run summary line the CLI
    /// prints.
    pub fn config_hash(&self) -> Result<String, ConfigError> {
        let bytes = std::fs::read(&self.path).map_err(|e| ConfigError::Read {
            path: self.path.clone(),
            source: e,
        })?;
        Ok(format!("sha256:{}", hex64(&bytes)))
    }

    /// The plugin launch. A `plugin.command` is resolved against the project root, because
    /// it is written relative to the project that owns the config. With no command, the
    /// launch comes from the registry record for `plugin.name`; `validate()` has already
    /// rejected a name that is not installed.
    pub fn launch(&self) -> Result<Launch, ConfigError> {
        let argv = match self.config.plugin.command.clone().filter(|a| !a.is_empty()) {
            Some(argv) => argv,
            None => {
                match crate::plugins::launch(self.config.plugin.name.as_deref().unwrap_or("")) {
                    Some(recorded) => recorded.argv,
                    None => {
                        return Err(ConfigError::msg(format!(
                        "`command` is required for plugin \"{}\". Install it with `veridoc install-plugin <git-url>`, or set `plugin.command`.",
                        self.config.plugin.name.as_deref().unwrap_or("")
                    )));
                    }
                }
            }
        };
        Ok(self.launch_of(argv))
    }

    fn launch_of(&self, argv: Vec<String>) -> Launch {
        // `plugin.command` is written relative to the project that owns the config, so a
        // relative file argument is made absolute against the project root, not the invoking
        // cwd. autofuzz resolves against the invoking cwd because there the base and the cwd
        // coincide.
        let root = self.root.clone();
        let argv: Vec<String> = argv
            .into_iter()
            .enumerate()
            .map(|(index, argument)| {
                let probe_root = root.clone();
                absolutized(index, argument, &root, move |path| {
                    probe_root.join(path).exists()
                })
            })
            .collect();
        let mut launch = Launch::new(argv);
        launch
            .env
            .insert("VERIDOC_ROOT".to_string(), self.root.display().to_string());
        launch.env.insert(
            "VERIDOC_CONFIG".to_string(),
            self.path.display().to_string(),
        );
        launch.cwd = Some(self.root.display().to_string());
        launch
    }
}

/// A plugin argument that names a file, made absolute when it names one that exists relative
/// to `base`. Flags are returned untouched, and a program name with no separator is left for
/// `PATH`.
pub fn absolutized(
    index: usize,
    argument: String,
    base: &Path,
    exists: impl Fn(&Path) -> bool,
) -> String {
    if argument.starts_with('-') || (index == 0 && !argument.contains('/')) {
        return argument;
    }
    let path = Path::new(&argument);
    if path.is_absolute() || !exists(path) {
        return argument;
    }
    base.join(path)
        .canonicalize()
        .map(|p| p.display().to_string())
        .unwrap_or(argument)
}

/// Load and validate `.veridoc/config.json` for a command that drives a plugin.
pub fn load(path: &Path) -> Result<LoadedConfig, ConfigError> {
    load_with(path, Config::validate)
}

/// Read and validate the configuration for a command that starts no plugin: `veridoc stat`. A
/// project whose plugin is named but not installed has a record to compare, and the gate that reads
/// it must not be the gate that builds a plugin (`docs/decisions/0014`).
pub fn load_without_plugin(path: &Path) -> Result<LoadedConfig, ConfigError> {
    load_with(path, Config::validate_record)
}

/// The shared read: the project root is the directory that holds `.veridoc/`.
fn load_with(
    path: &Path,
    validate: fn(&Config) -> Result<(), ConfigError>,
) -> Result<LoadedConfig, ConfigError> {
    let abs = absolute(path)?;
    let root = abs
        .parent() // .veridoc/
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let text = std::fs::read_to_string(path).map_err(|e| ConfigError::Read {
        path: path.to_path_buf(),
        source: e,
    })?;
    let mut value: Value = serde_json::from_str(&text).map_err(|e| ConfigError::Parse {
        path: path.to_path_buf(),
        source: e,
    })?;
    expand(&mut value, &dotenv(&root));
    let config: Config = serde_json::from_value(value).map_err(|e| ConfigError::Parse {
        path: path.to_path_buf(),
        source: e,
    })?;
    validate(&config)?;
    Ok(LoadedConfig {
        root,
        path: abs,
        config,
    })
}

/// The values `<root>/.env` declares, parsed by dotenvy. A missing file declares nothing, and a line
/// dotenvy rejects is skipped rather than fatal, because a commented-out experiment in a machine's own
/// file must not stop a run. dotenvy expands `${OTHER}` inside a value the way dotenv always has, so
/// a value that must stay verbatim cannot contain `${`; the engine's own substitution runs later, over
/// the configuration text (`substitute`).
fn dotenv(root: &Path) -> BTreeMap<String, String> {
    let Ok(entries) = dotenvy::from_path_iter(root.join(DOTENV_FILE)) else {
        return BTreeMap::new();
    };
    entries.flatten().collect()
}

/// Substitute `${NAME}` in every string the configuration holds, from the process environment
/// first and `<root>/.env` second. A name nothing supplies is left exactly as written, because the
/// configuration has to load on a machine that has no local model server, which is the state of a
/// continuous-integration job that never calls one. The command that needs the value reports the
/// name (`make_judge` in `pipeline.rs`).
fn expand(value: &mut Value, local: &BTreeMap<String, String>) {
    match value {
        Value::String(text) => *text = substitute(text, local),
        Value::Array(items) => {
            for item in items {
                expand(item, local);
            }
        }
        Value::Object(fields) => {
            for (_, item) in fields {
                expand(item, local);
            }
        }
        _ => {}
    }
}

/// The `${NAME}` occurrences in one string, resolved.
fn substitute(text: &str, local: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("${") {
        out.push_str(&rest[..at]);
        let Some(end) = rest[at..].find('}') else {
            out.push_str(&rest[at..]);
            return out;
        };
        let written = &rest[..at + end + 1];
        let name = &rest[at + 2..at + end];
        match std::env::var(name)
            .ok()
            .or_else(|| local.get(name).cloned())
        {
            Some(found) => out.push_str(&found),
            None => out.push_str(written),
        }
        rest = &rest[at + end + 1..];
    }
    out.push_str(rest);
    out
}

/// Find `.veridoc/config.json`. A directory argument is searched, then upward; a file path is
/// accepted directly.
pub fn discover(start: &Path) -> Result<PathBuf, ConfigError> {
    let start = absolute(start).unwrap_or_else(|_| start.to_path_buf());
    let mut dir = if start.is_file() {
        return Ok(start);
    } else {
        start.clone()
    };
    loop {
        let candidate = dir.join(CONFIG_DIR).join(CONFIG_FILE_NAME);
        if candidate.is_file() {
            return Ok(candidate);
        }
        if !dir.pop() {
            return Err(ConfigError::msg(format!(
                "no {CONFIG_DIR}/{CONFIG_FILE_NAME} found from {}; pass --config <path>",
                start.display()
            )));
        }
    }
}

fn absolute(path: &Path) -> Result<PathBuf, ConfigError> {
    std::path::absolute(path).map_err(|e| ConfigError::Read {
        path: path.to_path_buf(),
        source: e,
    })
}
