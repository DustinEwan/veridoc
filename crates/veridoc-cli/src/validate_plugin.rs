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
//!
//! `--out <dir>` is the same suite in the role the pluginrpc mechanism calls its suite
//! program: the mechanism writes the profile spec, the plugin manifest, and the profile's
//! schemas into `<dir>`, and this writes `<dir>/verdicts.json` back. That mode takes no
//! plugin argument, because the manifest the mechanism wrote names one, and it prints no
//! report, because `verdicts.json` is the output (spec §12 item 6).

use std::path::PathBuf;

use veridoc_engine::{install, plugins};

pub fn run(plugin: Option<&str>, out: Option<PathBuf>) -> Result<i32, String> {
    if let Some(dir) = out {
        // A suite error returns `Err` and writes no file, which is how §12 item 5 keeps the
        // suite's own failure — an unreadable spec, a plugin that will not start — off the
        // plugin's record.
        let verdicts = veridoc_conformance::suite::run(&dir)?;
        eprintln!(
            "wrote {} verdicts to {}",
            verdicts.len(),
            dir.join("verdicts.json").display()
        );
        return Ok(0);
    }
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
    let report = veridoc_conformance::drive(&name, &root, &argv, &emitter, deadline_ms)?;
    println!("{name}:");
    print!("{}", report.render());
    Ok(if report.passed() { 0 } else { 1 })
}
