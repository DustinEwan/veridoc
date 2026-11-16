//! `.veridoc/config.json`: upward discovery, defaults, the exact rejection messages, and the
//! plugin launch. Validation and user-facing failure share these strings, so the assertions
//! here are the contract the user reads.

use std::path::{Path, PathBuf};

use veridoc_engine::config;

fn tmp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vd-engine-config-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Canonical so the loaded absolute paths compare equal without symlink surprises.
    dir.canonicalize().unwrap()
}

fn write_config(root: &Path, json: &str) -> PathBuf {
    let dir = root.join(".veridoc");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.json");
    std::fs::write(&path, json).unwrap();
    path
}

const MINIMAL: &str = r#"{
  "plugin": { "command": ["true"] },
  "judge": { "command": ["true"], "model": "scripted-1" }
}"#;

#[test]
fn defaults_apply_to_every_omitted_key() {
    let root = tmp_dir("defaults");
    let path = write_config(&root, MINIMAL);
    let loaded = config::load(&path).expect("minimal config loads");
    assert_eq!(loaded.root, root.canonicalize().unwrap());
    assert_eq!(loaded.config.protocol, "vd/1");
    assert_eq!(loaded.config.plugin.handshake_timeout_ms, 30_000);
    assert_eq!(loaded.config.plugin.collect_timeout_ms, 120_000);
    assert_eq!(loaded.config.coverage.required, 0.0);
    assert_eq!(loaded.config.run.scenario_deadline_ms, 30_000);
    assert_eq!(loaded.config.judge.transport, "command");
    assert_eq!(loaded.config.judge.prompt_version, "vd-prompt-1");
    assert_eq!(loaded.config.judge.timeout_ms, 120_000);
    assert_eq!(loaded.config.output.dir, ".veridoc/out");
    assert_eq!(
        loaded.config.formatters().unwrap(),
        vec![config::Formatter::Builtin("markdown".to_string())]
    );
}

#[test]
fn an_unknown_field_is_a_configuration_error_naming_it() {
    let root = tmp_dir("unknown");
    let path = write_config(
        &root,
        &MINIMAL.replace(
            "\"judge\"",
            "\"semanticRules\": { \"k\": 1 }, \"judge\"",
        ),
    );
    let err = config::load(&path).unwrap_err().to_string();
    assert!(
        err.contains("unknown field `semanticRules`"),
        "unexpected message: {err}"
    );
}

#[test]
fn each_rejection_carries_its_exact_message() {
    let cases: Vec<(&str, &str)> = vec![
        (
            r#"{ "protocol": "vd/2", "plugin": { "command": ["true"] },
                "judge": { "command": ["true"], "model": "m" } }"#,
            "protocol \"vd/2\" is not supported; this build speaks \"vd/1\"",
        ),
        (
            r#"{ "judge": { "command": ["true"], "model": "m" } }"#,
            "plugin.command is required, or plugin.name must name an installed plugin",
        ),
        (
            r#"{ "plugin": { "name": "not-installed-xyz" },
                "judge": { "command": ["true"], "model": "m" } }"#,
            "`command` is required for plugin \"not-installed-xyz\". Install it with `veridoc install-plugin <git-url>`, or set `plugin.command`.",
        ),
        (
            r#"{ "plugin": { "command": ["true"] }, "coverage": { "required": 1.5 },
                "judge": { "command": ["true"], "model": "m" } }"#,
            "coverage.required must be between 0.0 and 1.0",
        ),
        (
            r#"{ "plugin": { "command": ["true"] }, "run": { "scenarioDeadlineMs": 0 },
                "judge": { "command": ["true"], "model": "m" } }"#,
            "run.scenarioDeadlineMs must be greater than zero",
        ),
        (
            r#"{ "plugin": { "command": ["true"] },
                "judge": { "transport": "openai", "model": "m" } }"#,
            "judge.transport must be \"command\" or \"anthropic\"",
        ),
        (
            r#"{ "plugin": { "command": ["true"] }, "judge": { "model": "m" } }"#,
            "judge.command is required when judge.transport is \"command\"",
        ),
        (
            r#"{ "plugin": { "command": ["true"] }, "judge": { "command": ["true"] } }"#,
            "judge.model is required",
        ),
        (
            r#"{ "plugin": { "command": ["true"] },
                "judge": { "command": ["true"], "model": "m" },
                "output": { "formatters": ["html"] } }"#,
            "output.formatters names \"html\"; the built-in formatters are \"markdown\" and \"json\"",
        ),
        (
            r#"{ "plugin": { "command": ["true"] },
                "judge": { "command": ["true"], "model": "m" },
                "output": { "formatters": [{ "name": "html" }] } }"#,
            "output.formatters entry must be a string or an object with name and command",
        ),
        (
            r#"{ "plugin": { "command": ["true"] },
                "judge": { "command": ["true"], "model": "m" },
                "citationStores": [ { "name": "s", "kind": "file" },
                                    { "name": "s", "kind": "git" } ] }"#,
            "citation store \"s\" is declared twice",
        ),
    ];
    for (index, (json, want)) in cases.iter().enumerate() {
        let root = tmp_dir(&format!("reject-{index}"));
        let path = write_config(&root, json);
        let got = config::load(&path).unwrap_err().to_string();
        assert_eq!(&got, want, "case {index}");
    }
}

#[test]
fn discovery_walks_upward_to_the_nearest_veridoc_directory() {
    let root = tmp_dir("upward");
    let path = write_config(&root, MINIMAL);
    let deep = root.join("a").join("b").join("c");
    std::fs::create_dir_all(&deep).unwrap();
    assert_eq!(config::discover(&deep).unwrap(), path.canonicalize().unwrap());
    // A file path is accepted directly.
    assert_eq!(config::discover(&path).unwrap(), path.canonicalize().unwrap());
}

#[test]
fn discovery_names_the_start_when_nothing_is_found() {
    let root = tmp_dir("absent");
    let deep = root.join("x");
    std::fs::create_dir_all(&deep).unwrap();
    let err = config::discover(&deep).unwrap_err().to_string();
    assert!(
        err.starts_with("no .veridoc/config.json found from ")
            && err.ends_with("; pass --config <path>"),
        "unexpected message: {err}"
    );
}

#[test]
fn launch_resolves_file_arguments_against_the_project_and_injects_the_environment() {
    let root = tmp_dir("launch");
    std::fs::write(root.join("server.js"), "// fake").unwrap();
    let path = write_config(
        &root,
        r#"{ "plugin": { "command": ["node", "server.js"] },
            "judge": { "command": ["true"], "model": "m" } }"#,
    );
    let loaded = config::load(&path).unwrap();
    let launch = loaded.launch().unwrap();
    assert_eq!(launch.argv[0], "node", "a program name with no separator stays for PATH");
    assert_eq!(
        launch.argv[1],
        root.join("server.js").canonicalize().unwrap().display().to_string(),
        "a relative file argument is resolved against the project, not the invoking cwd"
    );
    assert_eq!(launch.cwd.as_deref(), Some(root.canonicalize().unwrap().to_str().unwrap()));
    assert_eq!(
        launch.env.get("VERIDOC_ROOT").map(String::as_str),
        Some(root.canonicalize().unwrap().to_str().unwrap())
    );
    assert_eq!(
        launch.env.get("VERIDOC_CONFIG").map(String::as_str),
        Some(path.canonicalize().unwrap().to_str().unwrap())
    );
}

#[test]
fn config_hash_is_the_sha256_of_the_config_bytes() {
    let root = tmp_dir("hash");
    let path = write_config(&root, MINIMAL);
    let loaded = config::load(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let want = format!("sha256:{}", veridoc_engine::hash::hex64(&bytes));
    assert_eq!(loaded.config_hash().unwrap(), want);
}
