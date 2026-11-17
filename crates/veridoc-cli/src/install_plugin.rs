//! `veridoc install-plugin <git-url>`: clone, pin, build, conform, record. The previous
//! install survives any failure: the registry swap happens only after the conformance suite
//! passes, so a broken new version cannot take down a project that names the plugin.

use std::path::PathBuf;

use veridoc_engine::install::{self, Staged};
use veridoc_engine::plugins::{self, InstalledConformance, InstalledPlugin};

pub fn run(url: &str, reference: Option<&str>, name: Option<&str>) -> Result<i32, String> {
    let staged = install::stage(url, reference)?;
    if let Err(e) = finish(&staged, url, name) {
        install::discard(&staged.path);
        return Err(e);
    }
    Ok(0)
}

fn finish(staged: &Staged, url: &str, name: Option<&str>) -> Result<(), String> {
    let manifest_name = staged.manifest.name.clone();
    plugins::check_name(&manifest_name)?;
    if let Some(want) = name {
        if want != manifest_name {
            return Err(format!(
                "--name \"{want}\" does not match the manifest name \"{manifest_name}\""
            ));
        }
    }
    install::run_steps(staged)?;
    if staged.manifest.command.is_empty() {
        return Err(format!(
            "veridoc-plugin.json declares no command for plugin \"{manifest_name}\""
        ));
    }
    let Some(conformance) = &staged.manifest.conformance else {
        return Err(format!(
            "plugin \"{manifest_name}\" declares no conformance emitter. Add \"conformance\": {{ \"emitter\": [...] }} to its veridoc-plugin.json."
        ));
    };

    // Conformance runs against the staged checkout, before anything in the registry moves.
    let argv = plugins::resolve_argv(&staged.path, &staged.manifest.command);
    let emitter: Vec<PathBuf> = plugins::resolve_argv(&staged.path, &conformance.emitter)
        .into_iter()
        .map(PathBuf::from)
        .collect();
    let report = veridoc_conformance::drive(
        &manifest_name,
        &staged.path,
        &argv,
        &emitter,
        conformance.deadline_ms,
    )?;
    print!("{}", report.render());
    if !report.passed() {
        return Err(format!(
            "plugin \"{manifest_name}\" does not conform; the previous install is unchanged"
        ));
    }

    // Swap: the previous checkout moves aside, the staging directory becomes the checkout,
    // and the record is written last. A failed move restores the previous checkout.
    let dest = plugins::root().join(&manifest_name);
    let checkout = dest.join("checkout");
    let old = plugins::root().join(format!("{manifest_name}.old"));
    let _ = std::fs::remove_dir_all(&old);
    std::fs::create_dir_all(&dest).map_err(|e| format!("cannot create {}: {e}", dest.display()))?;
    let had_old = checkout.exists();
    if had_old {
        std::fs::rename(&checkout, &old)
            .map_err(|e| format!("cannot move the previous checkout aside: {e}"))?;
    }
    if let Err(e) = std::fs::rename(&staged.path, &checkout) {
        if had_old {
            let _ = std::fs::rename(&old, &checkout);
        }
        return Err(format!("cannot move the checkout into place: {e}"));
    }
    // The recorded argv points at the final location, not at the staging directory the
    // conformance drive ran from.
    let argv = plugins::resolve_argv(&checkout, &staged.manifest.command);
    let emitter = plugins::resolve_argv(&checkout, &conformance.emitter)
        .into_iter()
        .map(PathBuf::from)
        .collect();
    let record = InstalledPlugin {
        name: manifest_name.clone(),
        url: url.to_string(),
        commit: staged.commit.clone(),
        root: checkout,
        argv,
        conformance: Some(InstalledConformance {
            emitter,
            deadline_ms: conformance.deadline_ms,
        }),
    };
    plugins::record(&record)?;
    let _ = std::fs::remove_dir_all(&old);
    println!(
        "installed {} at {} from {url}",
        record.name,
        &record.commit[..record.commit.len().min(12)]
    );
    Ok(())
}
