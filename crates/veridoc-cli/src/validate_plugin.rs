//! `veridoc validate-plugin [--name <name>]`: re-drive the conformance suite against
//! installed checkouts. Without `--name`, every installed plugin is validated.

use veridoc_engine::plugins;

pub fn run(name: Option<&str>) -> Result<i32, String> {
    let installed: Vec<_> = match name {
        Some(n) => vec![plugins::lookup(n).ok_or(format!(
            "no plugin \"{n}\" is installed. Install it with `veridoc install-plugin <git-url>`."
        ))?],
        None => {
            let all = plugins::all();
            if all.is_empty() {
                return Err("no plugins are installed. Install one with `veridoc install-plugin <git-url>`.".to_string());
            }
            all
        }
    };
    let mut failed = false;
    for plugin in installed {
        let Some(conformance) = &plugin.conformance else {
            println!(
                "FAIL  {:<22} the record carries no conformance emitter; reinstall it",
                plugin.name
            );
            failed = true;
            continue;
        };
        let report = crate::install_plugin::drive(
            &plugin.name,
            &plugin.root,
            &plugin.argv,
            &conformance.emitter,
            conformance.deadline_ms,
        )?;
        println!("{} ({}):", plugin.name, &plugin.commit[..plugin.commit.len().min(12)]);
        print!("{}", report.render());
        failed |= !report.passed();
    }
    Ok(if failed { 1 } else { 0 })
}
