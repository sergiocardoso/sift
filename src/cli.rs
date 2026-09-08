use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "sift", about = "Local-first safe file organizer/cleaner.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
    /// Path to inspect when no subcommand is given; runs an organize dry-run.
    #[arg(default_value = ".")]
    pub path: String,
}

#[derive(Subcommand)]
pub enum Commands {
    Scan {
        #[arg(default_value = ".")]
        path: String,
        #[arg(long)]
        json: bool,
    },
    Organize {
        #[arg(default_value = ".")]
        path: String,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        json: bool,
    },
    Clean {
        #[arg(default_value = ".")]
        path: String,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        json: bool,
    },
    Doctor {
        #[arg(default_value = ".")]
        path: String,
        #[arg(long)]
        json: bool,
    },
    History,
    Undo {
        id: String,
    },
    Init {
        #[arg(default_value = ".")]
        path: String,
        #[arg(long)]
        force: bool,
    },
}

pub fn run() {
    let cli = Cli::parse();
    dispatch(cli);
}

pub fn dispatch(cli: Cli) {
    let Some(command) = cli.command else {
        // Bare `sift [path]`: safe dry-run preview, never mutates.
        crate::planner::cmd_organize(cli.path, false, false);
        return;
    };
    match command {
        Commands::Scan { path, json } => {
            crate::scanner::cmd_scan(path, json);
        }
        Commands::Organize { path, apply, json } => {
            crate::planner::cmd_organize(path, apply, json);
        }
        Commands::Clean { path, apply, json } => {
            crate::planner::cmd_clean(path, apply, json);
        }
        Commands::Doctor { path, json } => {
            crate::scanner::cmd_doctor(path, json);
        }
        Commands::History => {
            crate::history::cmd_history();
        }
        Commands::Undo { id } => {
            crate::history::cmd_undo(id);
        }
        Commands::Init { path, force } => {
            crate::config::cmd_init(path, force);
        }
    }
}
