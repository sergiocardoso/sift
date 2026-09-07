use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default = "default_root_rules")]
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub general: General,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct General {
    #[serde(default = "default_apply_flag")]
    pub apply_by_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub name: String,
    pub pattern: String,
    pub action: String, // "Move", "Trash", "Skip"
    pub destination: Option<String>,
    pub priority: i32,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub description: Option<String>,
}

pub fn find_config(start: &str) -> Option<PathBuf> {
    let mut cur = PathBuf::from(start);
    loop {
        let cfg = cur.join(".sift.toml");
        if cfg.is_file() {
            return Some(cfg);
        }
        if !cur.pop() {
            break;
        }
    }
    if let Some(home) = directories::BaseDirs::new() {
        let global = home.config_dir().join("sift").join("config.toml");
        if global.is_file() {
            return Some(global);
        }
    }
    None
}

fn default_root_rules() -> Vec<Rule> {
    vec![]
}

fn default_apply_flag() -> bool {
    false
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
    let example = "# Sift TOML config\n# Each rule is matched by glob pattern (path or ext, deterministic).\n# Action: Move, Trash, Skip.\n\n[[rules]]\nname = 'tmp files'\npattern = '*.tmp'\naction = 'Trash'\npriority = 1\nenabled = true\ndescription = 'Trash all .tmp files'\n\n[[rules]]\nname = 'archive files'\npattern = '*.zip'\naction = 'Move'\ndestination = 'Archives'\npriority = 2\nenabled = true\ndescription = 'Move .zip to Archives'\n\n[general]\napply_by_default = false\n";
    std::fs::write(&target, example).expect("write sift config");
    println!("Wrote {}", target.display());
}
