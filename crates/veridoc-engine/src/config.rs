//! The project configuration: `.veridoc/config.json`, found by walking up from the project
//! directory. `#[serde(deny_unknown_fields)]` makes an unknown key a configuration error
//! rather than a silently disabled check, so a project carrying a `semanticRules` block from
//! the draft spec fails with `unknown field semanticRules` instead of ignoring it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::hash::hex64;
use crate::plugin::Launch;

pub const CONFIG_DIR: &str = ".veridoc";
pub const CONFIG_FILE_NAME: &str = "config.json";

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

/// The judge transport. `"command"` runs `command` with the brief JSON on stdin and a verdict
/// JSON on stdout; `"anthropic"` POSTs to the Messages API. `"command"` is the default
/// because it is testable without a network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct JudgeConfig {
    pub transport: String,
    pub command: Option<Vec<String>>,
    pub model: Option<String>,
    pub prompt_version: String,
    pub timeout_ms: u64,
}

impl Default for JudgeConfig {
    fn default() -> Self {
        JudgeConfig {
            transport: "command".to_string(),
            command: None,
            model: None,
            prompt_version: "veridoc-prompt-1".to_string(),
            timeout_ms: 120_000,
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
    pub judge: JudgeConfig,
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
            judge: JudgeConfig::default(),
            output: OutputConfig::default(),
            citation_stores: Vec::new(),
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.protocol != veridoc_protocol::PROTOCOL_VERSION {
            return Err(ConfigError::msg(format!(
                "protocol \"{}\" is not supported; this build speaks \"{}\"",
                self.protocol,
                veridoc_protocol::PROTOCOL_VERSION
            )));
        }
        let has = |c: &Option<Vec<String>>| c.as_ref().is_some_and(|a| !a.is_empty());
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
        if self.judge.transport != "command" && self.judge.transport != "anthropic" {
            return Err(ConfigError::msg(
                "judge.transport must be \"command\" or \"anthropic\"",
            ));
        }
        if self.judge.transport == "command" && !has(&self.judge.command) {
            return Err(ConfigError::msg(
                "judge.command is required when judge.transport is \"command\"",
            ));
        }
        if self.judge.model.as_deref().unwrap_or("").is_empty() {
            return Err(ConfigError::msg("judge.model is required"));
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

    /// The judge launch for a `"command"` transport, resolved the same way as the plugin.
    pub fn judge_launch(&self) -> Result<Launch, ConfigError> {
        let argv = self
            .config
            .judge
            .command
            .clone()
            .filter(|a| !a.is_empty())
            .ok_or_else(|| {
                ConfigError::msg("judge.command is required when judge.transport is \"command\"")
            })?;
        Ok(self.launch_of(argv))
    }

    fn launch_of(&self, argv: Vec<String>) -> Launch {
        // `plugin.command` and `judge.command` are written relative to the project that owns
        // the config, so a relative file argument is made absolute against the project root,
        // not the invoking cwd. autofuzz resolves against the invoking cwd because there the
        // base and the cwd coincide.
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

/// Load and validate `.veridoc/config.json`. The project root is the directory that holds
/// `.veridoc/`.
pub fn load(path: &Path) -> Result<LoadedConfig, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|e| ConfigError::Read {
        path: path.to_path_buf(),
        source: e,
    })?;
    let config: Config = serde_json::from_str(&text).map_err(|e| ConfigError::Parse {
        path: path.to_path_buf(),
        source: e,
    })?;
    config.validate()?;
    let abs = absolute(path)?;
    let root = abs
        .parent() // .veridoc/
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(LoadedConfig {
        root,
        path: abs,
        config,
    })
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

/// The `.veridoc/` skeleton `veridoc init` writes. The committed set is the config plus the
/// citation stores; everything else the engine produces is ignored.
pub fn init(dir: &Path) -> Result<(), ConfigError> {
    let cfg_dir = dir.join(CONFIG_DIR);
    std::fs::create_dir_all(&cfg_dir).map_err(|e| ConfigError::Read {
        path: cfg_dir.clone(),
        source: e,
    })?;
    let config_path = cfg_dir.join(CONFIG_FILE_NAME);
    if config_path.exists() {
        return Err(ConfigError::msg(format!(
            "{} already exists; not overwriting",
            config_path.display()
        )));
    }
    let template = serde_json::json!({
        "protocol": veridoc_protocol::PROTOCOL_VERSION,
        "plugin": { "command": ["node", "plugin/veridoc-plugin.js"] },
        "coverage": { "required": 0.6 },
        "run": { "scenarioDeadlineMs": 30_000 },
        "judge": {
            "transport": "command",
            "command": ["node", "tools/judge.mjs"],
            "model": "scripted-1",
            "promptVersion": "veridoc-prompt-1",
            "timeoutMs": 120_000
        },
        "output": { "dir": ".veridoc/out", "formatters": ["markdown"] },
        "citationStores": []
    });
    std::fs::write(
        &config_path,
        serde_json::to_string_pretty(&template).expect("template is serializable") + "\n",
    )
    .map_err(|e| ConfigError::Read {
        path: config_path,
        source: e,
    })?;
    std::fs::write(
        cfg_dir.join(".gitignore"),
        "result.json\nlast-result-hash\nout/\nengine.log\nverdicts/\n",
    )
    .map_err(|e| ConfigError::Read {
        path: cfg_dir.join(".gitignore"),
        source: e,
    })?;
    let index = dir.join("index.doc.ts");
    if !index.exists() {
        std::fs::write(
            &index,
            "import { tree } from \"@veridoc/tree\";\n\n\
             export default tree({\n  concepts: [],\n  documents: [],\n});\n",
        )
        .map_err(|e| ConfigError::Read {
            path: index,
            source: e,
        })?;
    }
    Ok(())
}

fn absolute(path: &Path) -> Result<PathBuf, ConfigError> {
    std::path::absolute(path).map_err(|e| ConfigError::Read {
        path: path.to_path_buf(),
        source: e,
    })
}
