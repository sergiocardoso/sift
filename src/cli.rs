use clap::{Parser, Subcommand};

const EXAMPLES: &str = "\
EXAMPLES:
  sift                       Preview an organize plan for the current directory
  sift scan .                List what's in a directory (never mutates)
  sift organize .            Preview an organize plan (dry-run, no changes)
  sift organize . --apply    Actually move files according to the plan
  sift organize . --recursive
                             Organize every eligible subdirectory in place
  sift clean . --apply       Send high-confidence junk (.tmp/.swp/.swo) to the trash
  sift doctor .              Report large files, stale archives, sensitive-looking names
  sift history               List past operations
  sift undo <operation-id>   Reverse a successful move
  sift init .                Write a starter .sift.toml with example rules
  sift watch add ~/Inbox --auto-apply
                             Register a folder for automatic organization
  sift watch start ~/Inbox   Start watching (does not touch pre-existing files)
  sift watch list            Show every registered watch and its state
  sift folders .             Preview which whole subfolders would move into Documents/Images/...
  sift folders . --apply     Actually move high-confidence folders (never merges, never overwrites)
  sift folders . --remove-duplicates --apply
                             Also send exact-content duplicate files (inside similarly-named
                             folders) to the Trash
  sift config check ~/Downloads
                             Validate the effective .sift.toml policy for a directory
  sift explain ~/Downloads/movie.mp4
                             Read-only: show exactly what organize would do to one file, and why

Add --json to scan, organize, clean, doctor, folders, config check, or
explain for machine-readable output.
Add --recursive to scan, organize, or doctor (not clean, not folders) to
descend into eligible subdirectories; hidden, symlinked, protected,
project-root, and Sift's own category directories are never entered.
`sift folders` only ever looks at immediate child folders and moves whole
folders intact — it never dismantles one, and medium-confidence folders are
only ever suggested, never auto-moved. `--remove-duplicates` only compares
files inside folders whose names already look like duplicates of each other,
and only ever removes a file that is byte-for-byte identical to one already
kept — sent to the Trash, never permanently deleted.
See `sift watch --help` for the full watch command group.";

/// Sift: a local-first, safe CLI to organize and clean up a directory.
///
/// Every command is a dry-run by default and only prints a plan; nothing
/// moves or gets trashed until you add --apply. Moves are undoable with
/// `sift undo <id>`; cleanup sends files to the system trash, never
/// deletes them outright. Hidden files, symlinks, and software project
/// directories (containing .git, Cargo.toml, package.json, pyproject.toml,
/// or pubspec.yaml) are never touched.
#[derive(Parser)]
#[command(name = "sift", version, after_help = EXAMPLES)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
    /// Path to inspect when no subcommand is given (runs an organize dry-run)
    #[arg(default_value = ".")]
    pub path: String,
}

#[derive(Subcommand)]
pub enum Commands {
    /// List what's in a directory (read-only, never mutates)
    Scan {
        /// Directory to scan
        #[arg(default_value = ".")]
        path: String,
        /// Print machine-readable JSON instead of a table
        #[arg(long)]
        json: bool,
        /// Descend into eligible subdirectories (never symlinks, hidden,
        /// protected, project-root, or Sift's own category directories)
        #[arg(long)]
        recursive: bool,
    },
    /// Preview or apply an organize plan (sorts files into Documents/Images/Audio/...)
    Organize {
        /// Directory to organize
        #[arg(default_value = ".")]
        path: String,
        /// Actually perform the moves (default is a dry-run preview)
        #[arg(long)]
        apply: bool,
        /// Print the plan as JSON instead of a table
        #[arg(long)]
        json: bool,
        /// List every skipped entry individually instead of collapsing large lists
        #[arg(long)]
        verbose: bool,
        /// Organize every eligible subdirectory in place, each as its own
        /// local context (never flattened into the target directory)
        #[arg(long)]
        recursive: bool,
    },
    /// Preview or apply a cleanup plan (sends high-confidence junk to the trash)
    Clean {
        /// Directory to clean
        #[arg(default_value = ".")]
        path: String,
        /// Actually send files to the trash (default is a dry-run preview)
        #[arg(long)]
        apply: bool,
        /// Print the plan as JSON instead of a table
        #[arg(long)]
        json: bool,
        /// List every skipped entry individually instead of a single count
        #[arg(long)]
        verbose: bool,
    },
    /// Report-only: large files, stale archives, sensitive-looking filenames, build output dirs
    Doctor {
        /// Directory to inspect
        #[arg(default_value = ".")]
        path: String,
        /// Print findings as JSON instead of a table
        #[arg(long)]
        json: bool,
        /// Inspect eligible subdirectories too (same traversal boundaries as
        /// `organize --recursive`); never reads file contents either way
        #[arg(long)]
        recursive: bool,
    },
    /// List past operations recorded in history
    History,
    /// Reverse a successful move from a past operation
    Undo {
        /// Operation id, as shown by `sift history`
        id: String,
    },
    /// Write a starter .sift.toml with example rules into a directory
    Init {
        /// Directory to write .sift.toml into
        #[arg(default_value = ".")]
        path: String,
        /// Overwrite an existing .sift.toml
        #[arg(long)]
        force: bool,
    },
    /// Persistent, controllable automatic organization for a folder
    Watch {
        #[command(subcommand)]
        action: WatchCommands,
    },
    /// Preview or apply moving whole immediate child folders into Documents/Images/...
    /// based on the types of files they contain (never based on folder name alone).
    /// Distinct from `organize --recursive`, which organizes files inside a folder but
    /// never moves the folder itself. Only high-confidence folders are ever moved;
    /// medium-confidence folders are only ever suggested, and uncertain/mixed/empty
    /// folders and software projects are always left alone. There is no `--recursive`
    /// flag: candidate selection is always exactly one level of immediate children.
    Folders {
        /// Directory whose immediate child folders to analyze
        #[arg(default_value = ".")]
        path: String,
        /// Actually move high-confidence folders (default is a dry-run preview)
        #[arg(long)]
        apply: bool,
        /// Print the analysis as JSON instead of a table
        #[arg(long)]
        json: bool,
        /// Also compare files inside each name-based possible-duplicate
        /// folder group and remove exact content matches (verified
        /// byte-for-byte, never on a hash match alone) from every folder in
        /// the group except the alphabetically-first one. Removal always
        /// means sending to the system Trash — recoverable there, never
        /// undoable via `sift undo`. Report-only unless combined with
        /// --apply.
        #[arg(long)]
        remove_duplicates: bool,
    },
    /// Manage and validate `.sift.toml` Smart Folder policy
    Config {
        #[command(subcommand)]
        action: ConfigCommands,
    },
    /// Read-only: explain exactly what `sift organize` would do to one file, and why
    Explain {
        /// The file to explain
        file: String,
        /// The policy root whose `.sift.toml` (or global/default policy)
        /// applies. Defaults to the file's own parent directory — never
        /// discovered by walking further up.
        #[arg(long)]
        root: Option<String>,
        /// Print the explanation as JSON instead of a table
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Validate the effective `.sift.toml` policy for a directory
    Check {
        /// Directory whose effective policy to check
        #[arg(default_value = ".")]
        path: String,
        /// Print the result as JSON instead of a table
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum WatchCommands {
    /// Register a folder as a watch (starts in `stopped` state; does not
    /// start monitoring and does not organize pre-existing files)
    Add {
        /// Directory to watch
        path: String,
        /// Required: explicit, persistent authorization for the watch to
        /// mutate this folder automatically once started
        #[arg(long)]
        auto_apply: bool,
        /// Organize eligible subdirectories in place too, same boundaries
        /// as `organize --recursive`
        #[arg(long)]
        recursive: bool,
    },
    /// List every registered watch and its state
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show one watch's detail, or all watches plus daemon status if no path is given
    Status {
        /// Registered watch to inspect (all watches if omitted)
        path: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Start monitoring a registered (stopped) watch; does not back-fill
    /// files that already existed before this
    Start {
        /// Registered watch to start
        path: String,
    },
    /// Pause a running watch: stays registered, stops processing events;
    /// any candidate not yet stable is discarded, not queued
    Pause {
        /// Registered watch to pause
        path: String,
    },
    /// Resume a paused watch; only events from now on are handled, never
    /// a catch-up of what happened while paused
    Resume {
        /// Registered watch to resume
        path: String,
    },
    /// Stop a watch: stays registered, stops processing events entirely
    Stop {
        /// Registered watch to stop
        path: String,
    },
    /// Remove a watch's registration entirely
    Remove {
        /// Registered watch to remove
        path: String,
    },
    /// Manage the single background watch daemon
    Daemon {
        #[command(subcommand)]
        action: DaemonCommands,
    },
}

#[derive(Subcommand)]
pub enum DaemonCommands {
    /// Whether the watch daemon is currently running
    Status,
    /// Ask the running daemon to shut down safely
    Stop,
    /// Internal: run the daemon in the foreground (normally launched
    /// detached by `watch start`; not meant to be run directly in a
    /// terminal you intend to keep using)
    Run,
}

pub fn run() -> std::process::ExitCode {
    let cli = Cli::parse();
    dispatch(cli)
}

fn exit_code(ok: bool) -> std::process::ExitCode {
    if ok {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}

pub fn dispatch(cli: Cli) -> std::process::ExitCode {
    let Some(command) = cli.command else {
        // Bare `sift [path]`: safe dry-run preview, never mutates.
        return exit_code(crate::planner::cmd_organize(
            cli.path, false, false, false, false,
        ));
    };
    match command {
        Commands::Scan {
            path,
            json,
            recursive,
        } => {
            crate::scanner::cmd_scan(path, json, recursive);
            std::process::ExitCode::SUCCESS
        }
        Commands::Organize {
            path,
            apply,
            json,
            verbose,
            recursive,
        } => exit_code(crate::planner::cmd_organize(
            path, apply, json, verbose, recursive,
        )),
        Commands::Clean {
            path,
            apply,
            json,
            verbose,
        } => exit_code(crate::planner::cmd_clean(path, apply, json, verbose)),
        Commands::Doctor {
            path,
            json,
            recursive,
        } => {
            crate::scanner::cmd_doctor(path, json, recursive);
            std::process::ExitCode::SUCCESS
        }
        Commands::History => {
            crate::history::cmd_history();
            std::process::ExitCode::SUCCESS
        }
        Commands::Undo { id } => exit_code(crate::history::cmd_undo(id)),
        Commands::Init { path, force } => {
            crate::config::cmd_init(path, force);
            std::process::ExitCode::SUCCESS
        }
        Commands::Watch { action } => dispatch_watch(action),
        Commands::Folders {
            path,
            apply,
            json,
            remove_duplicates,
        } => exit_code(crate::folders::cmd_folders(
            path,
            apply,
            json,
            remove_duplicates,
        )),
        Commands::Config { action } => match action {
            ConfigCommands::Check { path, json } => {
                let result = crate::config::resolve_policy(&path);
                let ok = result.is_ok();
                if json {
                    println!("{}", crate::render::config_check_json(&result));
                } else {
                    crate::render::config_check(&path, &result);
                }
                exit_code(ok)
            }
        },
        Commands::Explain { file, root, json } => {
            exit_code(crate::explain::cmd_explain(file, root, json))
        }
    }
}

fn dispatch_watch(action: WatchCommands) -> std::process::ExitCode {
    match action {
        WatchCommands::Add {
            path,
            auto_apply,
            recursive,
        } => exit_code(crate::watch::cmd_watch_add(path, auto_apply, recursive)),
        WatchCommands::List { json } => exit_code(crate::watch::cmd_watch_list(json)),
        WatchCommands::Status { path, json } => {
            exit_code(crate::watch::cmd_watch_status(path, json))
        }
        WatchCommands::Start { path } => exit_code(crate::watch::cmd_watch_start(path)),
        WatchCommands::Pause { path } => exit_code(crate::watch::cmd_watch_pause(path)),
        WatchCommands::Resume { path } => exit_code(crate::watch::cmd_watch_resume(path)),
        WatchCommands::Stop { path } => exit_code(crate::watch::cmd_watch_stop(path)),
        WatchCommands::Remove { path } => exit_code(crate::watch::cmd_watch_remove(path)),
        WatchCommands::Daemon { action } => match action {
            DaemonCommands::Status => exit_code(crate::watch::cmd_watch_daemon_status()),
            DaemonCommands::Stop => exit_code(crate::watch::cmd_watch_daemon_stop()),
            DaemonCommands::Run => exit_code(crate::watch::cmd_watch_daemon_run()),
        },
    }
}
