//! The veridoc command-line tool. One module per subcommand; `main()` ends in the
//! `veridoc: {e}` / exit 3 shape shared by the project's CLIs.

mod check;
mod init;
mod install_plugin;
mod plugins_cmd;
mod probe_plugin;
mod validate_plugin;
mod witness;

use std::path::PathBuf;
use std::process::exit;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

static STOP: AtomicBool = AtomicBool::new(false);
static PRESSES: AtomicU8 = AtomicU8::new(0);

#[derive(clap::Parser)]
#[command(
    name = "veridoc",
    version,
    about = "documentation the build proves",
    after_help = "exit codes: 0 documentation built; 1 at least one blocking finding; \
2 run failure (plugin unreachable, build or fixture failure, deadline, judge transport \
error or unusable verdict, formatter exited non-zero); 3 usage or configuration error; \
130 on a second Ctrl-C"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Run the whole pipeline. There is no flag that skips the semantic stage, because a
    /// claim on the judge channel with no verdict is a run failure.
    Check {
        #[arg(long, default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        force: bool,
    },
    /// Write `.veridoc/config.json`, `.veridoc/.gitignore`, and `index.doc.ts`.
    Init {
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Print the artefact's current sha256 and nothing else.
    Witness {
        claim_id: String,
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Spawn the plugin, handshake, collect, resolve, and print the claim and referent
    /// tables. The transport proof for a harness author.
    ProbePlugin {
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Clone a plugin repository, run its install and build steps, drive the conformance
    /// suite against it, and record it. A plugin that does not conform is not installed;
    /// the previous install survives.
    InstallPlugin {
        git_url: String,
        /// Branch, tag, or commit to check out before pinning.
        #[arg(long)]
        reference: Option<String>,
        /// Must match the manifest's name; a mismatch is an error, not a rename.
        #[arg(long)]
        name: Option<String>,
    },
    /// List installed plugins: name, commit, url, checkout path.
    Plugins,
    /// Drive the conformance suite against a plugin: an installed plugin name or a
    /// checkout directory (default `.`). A checkout needs only this executable; its
    /// build steps run, then the suite.
    ValidatePlugin {
        /// Installed plugin name or plugin checkout directory.
        plugin: Option<String>,
    },
}

fn main() -> ! {
    // The first Ctrl-C sets the stop flag so the run ends as a run failure with a complete
    // (absent) result.json; the second leaves immediately with 130.
    ctrlc::set_handler(|| {
        if PRESSES.fetch_add(1, Ordering::SeqCst) >= 1 {
            exit(130);
        }
        STOP.store(true, Ordering::Relaxed);
        eprintln!("veridoc: interrupt requested; press Ctrl-C again to abort");
    })
    .ok();

    let cli = <Cli as clap::Parser>::parse();
    let result = match cli.command {
        Commands::Check { project, config, force } => {
            check::run(&project, config.as_deref(), force, &STOP)
        }
        Commands::Init { project } => init::run(&project),
        Commands::Witness { claim_id, project } => witness::run(&project, &claim_id),
        Commands::ProbePlugin { project } => probe_plugin::run(&project),
        Commands::InstallPlugin {
            git_url,
            reference,
            name,
        } => install_plugin::run(&git_url, reference.as_deref(), name.as_deref()),
        Commands::Plugins => plugins_cmd::run(),
        Commands::ValidatePlugin { plugin } => validate_plugin::run(plugin.as_deref()),
    };
    match result {
        Ok(code) => exit(code),
        Err(e) => {
            eprintln!("veridoc: {e}");
            exit(3)
        }
    }
}
