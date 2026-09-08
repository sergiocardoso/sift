use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default = "default_root_rules")]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub name: String,
    pub pattern: String,
    pub action: String, // "Move", "Trash", "Skip"
    pub destination: Option<String>,
    /// Higher priority rules are evaluated first. Ties keep file order.
    pub priority: i32,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub description: Option<String>,
}

/// Returns config rules sorted by descending priority (highest first),
/// keeping the original relative order for equal priorities.
pub fn rules_by_priority(rules: &[Rule]) -> Vec<&Rule> {
    let mut sorted: Vec<&Rule> = rules.iter().filter(|r| r.enabled).collect();
    sorted.sort_by_key(|r| std::cmp::Reverse(r.priority));
    sorted
}

/// Syntactic validation only: rejects absolute paths, empty paths, and any
/// component other than a plain path segment (no `..`, `.`, or roots).
/// Does not touch the filesystem; see `safe_join_under` for the
/// symlink-aware check performed at plan time.
pub fn validate_rule_destination(dest: &str) -> bool {
    if dest.is_empty() {
        return false;
    }
    let path = Path::new(dest);
    path.components().all(|c| matches!(c, Component::Normal(_)))
}

/// Joins `rel` onto `base`, refusing to cross through any existing symlink
/// component (including the final one). Returns `None` if `rel` is not a
/// plain relative path or if any existing intermediate component is a
/// symlink, which would let a destination escape the selected target.
pub fn safe_join_under(base: &Path, rel: &Path) -> Option<PathBuf> {
    let mut cur = base.to_path_buf();
    for comp in rel.components() {
        match comp {
            Component::Normal(part) => {
                cur.push(part);
                if let Ok(md) = std::fs::symlink_metadata(&cur) {
                    if md.file_type().is_symlink() {
                        return None;
                    }
                }
            }
            _ => return None,
        }
    }
    Some(cur)
}

/// Locates a config file for `start`: only `start/.sift.toml`, then the
/// global config. Does not walk up parent directories.
pub fn find_config(start: &str) -> Option<PathBuf> {
    let local = PathBuf::from(start).join(".sift.toml");
    if local.is_file() {
        return Some(local);
    }
    let home = directories::BaseDirs::new()?;
    let global = home.config_dir().join("sift").join("config.toml");
    if global.is_file() {
        return Some(global);
    }
    None
}

fn default_root_rules() -> Vec<Rule> {
    vec![]
}

pub fn load_config(path: &PathBuf) -> Result<Config, std::io::Error> {
    let content = std::fs::read_to_string(path)?;
    let cfg: Config = toml::from_str(&content).map_err(std::io::Error::other)?;
    Ok(cfg)
}

pub fn save_config(cfg: &Config, path: &PathBuf) -> Result<(), std::io::Error> {
    let toml_str = toml::to_string_pretty(cfg).map_err(std::io::Error::other)?;
    std::fs::write(path, toml_str)
}

pub fn cmd_init(path: String, force: bool) {
    let target = PathBuf::from(path).join(".sift.toml");
    if target.exists() && !force {
        eprintln!(".sift.toml exists, use --force to overwrite");
        return;
    }
    let example = "# Sift TOML config\n# Each rule is matched by glob pattern (path or ext, deterministic).\n# Action: Move, Trash, Skip.\n# Mutation always requires passing --apply on the command line; there is no\n# config option to enable it implicitly.\n\n[[rules]]\nname = 'tmp files'\npattern = '*.tmp'\naction = 'Trash'\npriority = 1\nenabled = true\ndescription = 'Trash all .tmp files'\n\n[[rules]]\nname = 'archive files'\npattern = '*.zip'\naction = 'Move'\ndestination = 'Archives'\npriority = 2\nenabled = true\ndescription = 'Move .zip to Archives'\n";
    match std::fs::write(&target, example) {
        Ok(()) => println!("Wrote {}", target.display()),
        Err(e) => eprintln!("Failed to write {}: {}", target.display(), e),
    }
}
