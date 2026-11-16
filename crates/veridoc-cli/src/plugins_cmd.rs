//! `veridoc plugins`: the installed-plugin registry, one line per plugin, name first so CI
//! can grep it.

use veridoc_engine::plugins;

pub fn run() -> Result<i32, String> {
    for plugin in plugins::all() {
        println!(
            "{} {} {} {}",
            plugin.name,
            &plugin.commit[..plugin.commit.len().min(12)],
            plugin.url,
            plugin.root.display()
        );
    }
    Ok(0)
}
