//! The veridoc command-line tool. One module per subcommand; `main()` ends in the
//! `veridoc: {e}` / exit 3 shape shared by the project's CLIs.

mod check;
mod init;
mod install_plugin;
mod plugins_cmd;
mod probe_plugin;
mod stat;
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
    /// Report whether this checkout is the one the last completed run recorded, by comparing every
    /// file the commit would carry against `.veridoc/inputs.json`. Exits 0 when the record covers
    /// this checkout, 1 when it does not, 3 when there is no record. No plugin is started, no
    /// harness is built, and no model is called: the gate for a job that has no model to ask.
    Stat {
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Write `.veridoc/config.json` and `.veridoc/.gitignore`, then ask the plugin to create the
    /// files only it can write. Every question has a flag, so a script answers them all, and a
    /// question a script cannot answer is refused by naming its flag.
    Init {
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// An installed plugin, as `veridoc plugins` lists it.
        #[arg(long)]
        plugin: Option<String>,
        /// A plugin command line for a plugin nobody installed, for example "node plugin/server.mjs".
        #[arg(long)]
        plugin_command: Option<String>,
        /// The first document's path, relative to the project.
        #[arg(long)]
        document: Option<String>,
        /// Who reads the first document.
        #[arg(long)]
        audience: Option<String>,
        /// The one question the first document answers.
        #[arg(long)]
        question: Option<String>,
        /// Let the plugin replace a file it would create.
        #[arg(long)]
        force: bool,
    },
    /// Print the artefact's current sha256 and nothing else. `claim_name` is the name the report
    /// prints, `<subject>#<predicate>` (`docs/decisions/0009-the-subject-is-named-not-derived-from-a-path.md`).
    Witness {
        claim_name: String,
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
        /// The suite-program mode of the pluginrpc mechanism (spec §12 item 6): read the
        /// profile spec and the manifest `<dir>` holds, drive the plugin they name, and write
        /// `<dir>/verdicts.json` instead of a report. The `prpc-core` runner writes `<dir>` and
        /// appends this flag; it takes no plugin argument, because the manifest names one.
        #[arg(long, value_name = "DIR", conflicts_with = "plugin")]
        out: Option<PathBuf>,
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
        Commands::Check {
            project,
            config,
            force,
        } => check::run(&project, config.as_deref(), force, &STOP),
        Commands::Stat { project } => stat::run(&project),
        Commands::Init {
            project,
            plugin,
            plugin_command,
            document,
            audience,
            question,
            force,
        } => init::run(&init::Args {
            project: &project,
            plugin,
            plugin_command,
            document,
            audience,
            question,
            force,
        }),
        Commands::Witness {
            claim_name,
            project,
        } => witness::run(&project, &claim_name),
        Commands::ProbePlugin { project } => probe_plugin::run(&project),
        Commands::InstallPlugin {
            git_url,
            reference,
            name,
        } => install_plugin::run(&git_url, reference.as_deref(), name.as_deref()),
        Commands::Plugins => plugins_cmd::run(),
        Commands::ValidatePlugin { plugin, out } => validate_plugin::run(plugin.as_deref(), out),
    };
    match result {
        Ok(code) => exit(code),
        Err(e) => {
            eprintln!("veridoc: {e}");
            exit(3)
        }
    }
}
