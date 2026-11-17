//! `veridoc validate-plugin [plugin]`: drive the conformance suite against a plugin and
//! report, rule by rule, whether it conforms to the plugin contract. The argument is an
//! installed plugin name or a plugin checkout directory (default `.`). The plugin's
//! entrypoint is that checkout's `veridoc-plugin.json` `command`, and the fixture project
//! is whatever that plugin's emitter renders, so a checkout validation needs nothing
//! beyond this executable and the checkout — no registry record, no engine source.
//!
//! A checkout is validated by running the manifest's `build` steps and never its
//! `install` steps: an author's inner loop costs one compile and never a dependency
//! reinstall, and a build step recompiles from current source on every run, which is
//! what keeps a pass honest. `veridoc install-plugin` runs both before it records.

use std::path::PathBuf;

use veridoc_engine::{install, plugins};

pub fn run(plugin: Option<&str>) -> Result<i32, String> {
    let arg = plugin.unwrap_or(".");
    let (name, root, argv, emitter, deadline_ms) = match plugins::lookup(arg) {
        Some(record) => {
            let conformance = record.conformance.clone().ok_or(format!(
                "the record for \"{}\" carries no conformance emitter; reinstall it",
                record.name
            ))?;
            (
                record.name.clone(),
                record.root.clone(),
                record.argv.clone(),
                conformance.emitter.clone(),
                conformance.deadline_ms,
            )
        }
        None => {
            let dir = std::path::absolute(PathBuf::from(arg)).map_err(|e| format!("{arg}: {e}"))?;
            if !dir.join("veridoc-plugin.json").is_file() {
                return Err(format!(
                    "no installed plugin \"{arg}\" and no veridoc-plugin.json in {arg}. \
                     Run this command inside a plugin checkout, or pass an installed plugin name."
                ));
            }
            let manifest = plugins::read_manifest(&dir)?;
            plugins::check_name(&manifest.name)?;
            for build_argv in &manifest.build {
                install::run_step("build", &dir, build_argv)?;
            }
            if manifest.command.is_empty() {
                return Err(format!(
                    "plugin \"{}\" declares an empty \"command\"",
                    manifest.name
                ));
            }
            // Name the unbuilt plugin here rather than letting it die inside
            // `PluginClient::connect` with an OS error: at least one command element must
            // name a file that now exists.
            if !manifest
                .command
                .iter()
                .any(|argument| !argument.starts_with('-') && dir.join(argument).exists())
            {
                return Err(format!(
                    "veridoc-plugin.json \"command\" names no existing file under {}. \
                     Run the build steps in veridoc-plugin.json and try again.",
                    dir.display()
                ));
            }
            let conformance = manifest.conformance.as_ref().ok_or(format!(
                "plugin \"{}\" declares no conformance emitter",
                manifest.name
            ))?;
            let emitter = plugins::resolve_argv(&dir, &conformance.emitter)
                .into_iter()
                .map(PathBuf::from)
                .collect();
            (
                manifest.name.clone(),
                dir.clone(),
                plugins::resolve_argv(&dir, &manifest.command),
                emitter,
                conformance.deadline_ms,
            )
        }
    };
    let report = crate::install_plugin::drive(&name, &root, &argv, &emitter, deadline_ms)?;
    println!("{name}:");
    print!("{}", report.render());
    Ok(if report.passed() { 0 } else { 1 })
}
