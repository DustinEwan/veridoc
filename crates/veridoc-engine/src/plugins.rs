//! The installed-plugin registry: one directory per plugin, written by `veridoc install-plugin`.
//!
//! A project may name a plugin (`plugin.name`) without a `command`; the launch then comes from
//! the record this module owns. The engine never invents a runtime and never guesses a plugin
//! path: an uninstalled name is still an error, and `plugin.command` stays the way to run a
//! plugin nobody installed.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::absolutized;
use crate::plugin::Launch;

/// The `veridoc-plugin.json` at the root of a plugin repository. Flat, not an array: a
/// repository holds one plugin.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct PluginManifest {
    pub name: String,
    /// Dependency fetch: `npm ci` and anything else slow that a checkout needs once. Only
    /// `veridoc install-plugin` runs these.
    pub install: Vec<Vec<String>>,
    /// Compilation of the plugin and the emitter. `veridoc install-plugin` and
    /// `veridoc validate-plugin` both run these, so a validation never tests a stale build.
    pub build: Vec<Vec<String>>,
    pub command: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conformance: Option<Conformance>,
}

/// How this plugin is validated. `emitter` is a program the driver runs as `<argv…> --out <dir>`:
/// it renders the whole conformance project — source tree, sidecars, and
/// `veridoc-conformance.json`. That rendered project is the only project validation ever
/// drives, which is why `veridoc validate-plugin` takes no project of its own.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct Conformance {
    pub emitter: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_ms: Option<u64>,
}

/// `<VERIDOC_HOME or XDG_DATA_HOME/veridoc or HOME/.local/share/veridoc>/plugins`.
pub fn root() -> PathBuf {
    let unset = |v: Option<std::ffi::OsString>| v.filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = unset(std::env::var_os("VERIDOC_HOME"))
        .or_else(|| unset(std::env::var_os("XDG_DATA_HOME")).map(|d| d.join("veridoc")))
        .unwrap_or_else(|| {
            unset(std::env::var_os("HOME"))
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".local/share/veridoc")
        });
    base.join("plugins")
}

/// Read `<dir>/veridoc-plugin.json`.
pub fn read_manifest(dir: &Path) -> Result<PluginManifest, String> {
    let text = std::fs::read_to_string(dir.join("veridoc-plugin.json"))
        .map_err(|_| format!("no veridoc-plugin.json in {}", dir.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("veridoc-plugin.json: {e}"))
}

/// Resolve a manifest argv against the plugin repository root: every element that names an
/// existing file under `root` becomes absolute, so the argv runs from any working directory.
/// Program names and flags stay as written, exactly as [`absolutized`] rules them.
pub fn resolve_argv(root: &Path, argv: &[String]) -> Vec<String> {
    argv.iter()
        .enumerate()
        .map(|(index, argument)| {
            let probe_root = root.to_path_buf();
            absolutized(index, argument.clone(), root, move |path| {
                probe_root.join(path).exists()
            })
        })
        .collect()
}

/// One plugin recorded by `veridoc install-plugin`, at `<registry>/<name>/plugin.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstalledPlugin {
    pub name: String,
    pub url: String,
    pub commit: String,
    /// The installed repository root, absolute: `<registry>/<name>/checkout`.
    pub root: PathBuf,
    /// The plugin argv, absolute.
    pub argv: Vec<String>,
    /// The manifest's `conformance` block with its emitter argv resolved against `root`, so a
    /// caller never has to guess where a plugin repository keeps its emitter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conformance: Option<InstalledConformance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstalledConformance {
    /// The emitter argv from the manifest, each element that names an existing file resolved
    /// against `root`, so a caller can run it as it stands with `--out <temp>` appended.
    pub emitter: Vec<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_ms: Option<u64>,
}

/// The record at `<root()>/<name>/plugin.json`, or `None` when absent or unparseable. An
/// unreadable record must not break an unrelated check, so this never returns an error.
pub fn lookup(name: &str) -> Option<InstalledPlugin> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return None;
    }
    let text = std::fs::read_to_string(root().join(name).join("plugin.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// Every record under `root()`, sorted by name.
pub fn all() -> Vec<InstalledPlugin> {
    let mut plugins = Vec::new();
    let Ok(entries) = std::fs::read_dir(root()) else {
        return plugins;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(plugin) = lookup(&name) {
            plugins.push(plugin);
        }
    }
    plugins.sort_by(|a, b| a.name.cmp(&b.name));
    plugins
}

/// Write `<root()>/<name>/plugin.json`.
pub fn record(plugin: &InstalledPlugin) -> Result<PathBuf, String> {
    let dir = root().join(&plugin.name);
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let path = dir.join("plugin.json");
    let text = serde_json::to_string_pretty(plugin)
        .map_err(|e| format!("cannot serialize the record for {}: {e}", plugin.name))?;
    std::fs::write(&path, text.as_bytes())
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(path)
}

/// A `Launch` carrying the recorded argv, with no cwd and no environment: the caller sets
/// both, exactly as `LoadedConfig::launch` does.
pub fn launch(name: &str) -> Option<Launch> {
    lookup(name).map(|plugin| Launch::new(plugin.argv))
}
