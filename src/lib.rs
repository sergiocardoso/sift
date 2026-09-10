pub mod classifier;
pub mod cli;
pub mod config;
pub mod domain;
pub mod executor;
pub mod explain;
pub mod folders;
pub mod fs;
pub mod history;
pub mod metadata;
pub mod planner;
pub mod render;
pub mod scanner;
pub mod update_check;
pub mod utils;
pub mod watch;

/// Sift's own version (from this crate's `Cargo.toml`), for anything that
/// wants to display it — e.g. `sift-tray`'s "About" menu item — without
/// hardcoding a number that could drift out of sync.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
