use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "sift", about = "Local-first safe file organizer/cleaner.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    Scan {
        path: String,
        #[arg(long)]
        json: bool,
    },
    Organize {
        path: String,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        json: bool,
    },
    Clean {
        path: String,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        json: bool,
    },
    Doctor {
        path: String,
        #[arg(long)]
        json: bool,
    },
    History,
    Undo {
        id: String,
    },
    Init {
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
    match cli.command {
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
