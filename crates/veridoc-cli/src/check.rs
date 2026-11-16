//! `veridoc check`: load config, run the pipeline, print diagnostics. Errors and warnings
//! that name a project file go to stdout; a message with no file goes to stderr.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use veridoc_engine::config;
use veridoc_engine::pipeline;
pub fn run(
    project: &Path,
    config_path: Option<&Path>,
    force: bool,
    stop: &AtomicBool,
) -> Result<i32, String> {
    let path = match config_path {
        Some(explicit) => explicit.to_path_buf(),
        None => config::discover(project).map_err(|e| e.to_string())?,
    };
    let loaded = config::load(&path).map_err(|e| e.to_string())?;
    let report = pipeline::check(&loaded, force, stop);
    for diagnostic in &report.diagnostics {
        if diagnostic.stdout() {
            println!("{}", diagnostic.render());
        } else {
            eprintln!("{}", diagnostic.render());
        }
    }
    Ok(report.exit)
}
