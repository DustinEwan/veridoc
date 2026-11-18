//! `veridoc init`: the plugin the project speaks, the configuration, and the files only that plugin
//! can write. Every question has a flag, so one command serves a terminal, a script, and CI: a flag
//! skips its prompt, and a question a script cannot answer is refused by naming the flag rather than
//! by hanging on a read. Prompts go to stderr, which is the terminal dialoguer owns, so stdout
//! carries only the paths written. The host writes no project file the plugin can write:
//! `docs/decisions/0013` puts the root document with the plugin, and `docs/decisions/0006` fixes what
//! a project adds.

use std::io::IsTerminal;
use std::path::Path;
use std::time::Duration;

use veridoc_engine::config::{self, Config, CONFIG_DIR, CONFIG_FILE_NAME};
use veridoc_engine::plugin::{sentence, PluginClient};
use veridoc_engine::plugins;
use veridoc_protocol::{Capability, DocumentDecl, InitReq};

/// The run outputs, kept out of the project's history by the ignore file `init` writes. The harness
/// build a plugin makes during a run is one of them, so it is never committed.
const IGNORED: &str = "result.json\nlast-result-hash\nout/\ntarget/\nengine.log\nverdicts/\n";

/// Everything `veridoc init` asks for. Each field has a flag, and a `None` is a question.
pub struct Args<'a> {
    pub project: &'a Path,
    pub plugin: Option<String>,
    pub plugin_command: Option<String>,
    pub document: Option<String>,
    pub audience: Option<String>,
    pub question: Option<String>,
    pub force: bool,
}

/// The plugin the project will speak: a registry name, or a command line nobody installed.
enum Plugin {
    Name(String),
    Command(Vec<String>),
}

pub fn run(args: &Args) -> Result<i32, String> {
    let dir = args.project.join(CONFIG_DIR);
    let config_path = dir.join(CONFIG_FILE_NAME);
    if config_path.exists() {
        return Err(format!("{} already exists", config_path.display()));
    }
    let plugin = choose(args)?;
    let document = DocumentDecl {
        path: asked(
            args.document.clone(),
            "Where is the first document?",
            "document",
            || Some("docs/architecture.spec.md".to_string()),
        )?,
        audience: asked(
            args.audience.clone(),
            "Who reads the first document?",
            "audience",
            || None,
        )?,
        question: asked(
            args.question.clone(),
            "What one question does it answer?",
            "question",
            || None,
        )?,
        not_applicable: None,
    };

    let mut config = Config::default();
    match &plugin {
        Plugin::Name(name) => config.plugin.name = Some(name.clone()),
        Plugin::Command(argv) => config.plugin.command = Some(argv.clone()),
    }
    // The plugin answers before a single file is written. A project left with a configuration and no
    // root document is a half-written scaffold, and the refusal that produced it is the author's own
    // file colliding with the plugin's.
    let root = args.project.canonicalize().map_err(|e| e.to_string())?;
    let loaded = config::LoadedConfig {
        root,
        path: config_path.clone(),
        config,
    };
    let launch = loaded.launch().map_err(|e| e.to_string())?;
    let timeout = Duration::from_millis(
        loaded
            .config
            .plugin
            .handshake_timeout_ms
            .max(loaded.config.plugin.collect_timeout_ms),
    );
    let (client, handshake) = PluginClient::connect(&launch, timeout).map_err(|e| {
        format!(
            "the plugin {} could not be asked to initialise the project: {e}",
            launch.display()
        )
    })?;
    let created = if handshake.capabilities.contains(&Capability::Initialization) {
        Some(sentence(
            "init",
            client.init(
                &InitReq {
                    root: loaded.root.display().to_string(),
                    document,
                    force: args.force,
                },
                timeout,
            ),
        )?)
    } else {
        eprintln!(
            "{} declares no initialization capability, so `init` writes the configuration and \
             nothing else: ask the plugin how to create the root document, then run `veridoc check`",
            handshake.name
        );
        None
    };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(".gitignore"), IGNORED).map_err(|e| e.to_string())?;
    std::fs::write(
        &config_path,
        serde_json::to_string_pretty(&loaded.config).unwrap() + "\n",
    )
    .map_err(|e| e.to_string())?;
    println!("wrote {}", config_path.display());
    if let Some(created) = created {
        for path in created.written {
            println!("wrote {}", loaded.root.join(&path).display());
        }
        for next in created.next {
            eprintln!("next: {next}");
        }
    }
    Ok(0)
}

/// The plugin, from a flag or from the terminal. A machine run gets a refusal that names the flag,
/// because guessing which plugin a project speaks writes a configuration nobody asked for.
fn choose(args: &Args) -> Result<Plugin, String> {
    match (args.plugin.as_deref(), args.plugin_command.as_deref()) {
        (Some(_), Some(_)) => {
            Err("give `--plugin <name>` or `--plugin-command <argv>`, not both".to_string())
        }
        (Some(name), None) => {
            plugins::check_name(name)?;
            Ok(Plugin::Name(name.to_string()))
        }
        (None, Some(argv)) => Ok(Plugin::Command(
            argv.split_whitespace().map(str::to_string).collect(),
        )),
        (None, None) => {
            let installed = plugins::all();
            if installed.is_empty() {
                return Err(
                    "`--plugin-command <argv>` is required: no plugin is installed, and the plugin \
                     writes the files the host cannot"
                        .to_string(),
                );
            }
            let names: Vec<String> = installed.iter().map(|p| p.name.clone()).collect();
            if names.len() == 1 {
                eprintln!("plugin: {} (the only one installed)", names[0]);
                return Ok(Plugin::Name(names[0].clone()));
            }
            if !terminal() {
                return Err(format!(
                    "`--plugin <name>` is required without a terminal to ask for it; installed: {}",
                    names.join(", ")
                ));
            }
            let picked = dialoguer::Select::new()
                .with_prompt("Which plugin does this project speak?")
                .items(&names)
                .default(0)
                .interact()
                .map_err(|e| format!("the plugin prompt was interrupted: {e}"))?;
            Ok(Plugin::Name(names[picked].clone()))
        }
    }
}

/// One answer: the flag, the prompt, or a refusal naming the flag. `default` is what a blank answer
/// means, and `None` means the question has no default worth guessing.
fn asked(
    answer: Option<String>,
    label: &str,
    flag: &str,
    default: impl FnOnce() -> Option<String>,
) -> Result<String, String> {
    if let Some(answer) = answer {
        return filled(answer, flag);
    }
    let fallback = default();
    if !terminal() {
        return match fallback {
            Some(fallback) => Ok(fallback),
            None => Err(format!(
                "`--{flag}` is required without a terminal to ask for it"
            )),
        };
    }
    let prompt = match fallback {
        Some(fallback) => dialoguer::Input::<String>::new()
            .with_prompt(label)
            .default(fallback.clone())
            .interact_text()
            .map(|answer| {
                if answer.trim().is_empty() {
                    fallback
                } else {
                    answer
                }
            }),
        None => dialoguer::Input::<String>::new()
            .with_prompt(label)
            .interact_text(),
    }
    .map_err(|e| format!("the prompt for `--{flag}` was interrupted: {e}"))?;
    filled(prompt, flag)
}

fn filled(answer: String, flag: &str) -> Result<String, String> {
    if answer.trim().is_empty() {
        return Err(format!("`--{flag}` needs an answer"));
    }
    Ok(answer)
}

/// Whether a person is on the other end. The check is on stderr, because stdout carries paths and a
/// pipe there is a script reading them.
fn terminal() -> bool {
    std::io::stderr().is_terminal()
}
