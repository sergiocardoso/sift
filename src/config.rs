//! Sift's local "Smart Folder" policy: `.sift.toml`.
//!
//! `.sift.toml` describes **how** a directory wants to be organized
//! (strategy, unknown-file handling, declarative rules, watch stability).
//! It never describes **whether** automatic organization is currently
//! running — that is operational state owned by the Watch registry
//! (`watch::registry`), never this file. Saving a `.sift.toml` can never by
//! itself authorize or start any mutation.
//!
//! This module is the ONE authoritative configuration parser/resolver:
//! [`resolve_policy`] is the single entry point manual `organize`/`clean`,
//! recursive organize, Watch, `sift config check`, and `sift explain` all
//! call — none of them parses TOML or walks the filesystem for a config
//! file on its own.

use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

// --------------------------------------------------------------- rules

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// Optional label, purely for display (falls back to `pattern` when
    /// blank). The v1 recommended rule shape does not require a name.
    #[serde(default)]
    pub name: String,
    pub pattern: String,
    pub action: String, // "Move", "Trash", "Skip"
    #[serde(default)]
    pub destination: Option<String>,
    /// Higher priority rules are evaluated first. Ties keep file order.
    pub priority: i32,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub description: Option<String>,
}

impl Rule {
    /// A short label for error/explain messages: the explicit `name` if set,
    /// otherwise the pattern.
    pub fn label(&self) -> &str {
        if self.name.trim().is_empty() {
            &self.pattern
        } else {
            &self.name
        }
    }
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

/// Static, config-load-time validation of one rule: a bad `action` name or
/// an unsafe/missing `Move` destination is rejected here, with a clear
/// message, well before it could ever reach planning. This is in addition
/// to (never a replacement for) the executor's own independent, live,
/// symlink-aware revalidation immediately before any mutation.
fn validate_rule(rule: &Rule) -> Result<(), String> {
    match rule.action.as_str() {
        "Move" => match &rule.destination {
            None => Err(format!(
                "rule '{}': action = \"Move\" requires a destination",
                rule.label()
            )),
            Some(d) if !validate_rule_destination(d) => Err(format!(
                "rule '{}': unsafe destination \"{d}\" (must be a relative path with no '..' and no leading '/')",
                rule.label()
            )),
            Some(_) => Ok(()),
        },
        "Trash" | "Skip" => Ok(()),
        other => Err(format!(
            "rule '{}': invalid action \"{other}\" (expected Move, Trash, or Skip)",
            rule.label()
        )),
    }
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

// ------------------------------------------------------------ strategy

/// How a Smart Folder organizes its files. `Type` reuses Sift's existing
/// classifier and is the only strategy implemented in this version.
///
/// The interface is intentionally left open for future, purely additive
/// variants (`Date`, `Media`, `Photos`, `Audio` — metadata-driven, NOT
/// implemented here): adding one means adding an enum variant and one new
/// arm in `plan_with_strategy`, never a second parallel config format or a
/// second classifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrganizeStrategy {
    Type,
    /// Organizes by the file's date (currently: filesystem modification
    /// time only — see [`DateSource`]) rendered through a [`Template`]
    /// into a relative destination directory. Requires `organize.template`.
    Date,
}

/// Every strategy name this build understands, for error messages and
/// `sift config check`.
pub const SUPPORTED_STRATEGIES: [&str; 2] = ["type", "date"];

impl OrganizeStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            OrganizeStrategy::Type => "type",
            OrganizeStrategy::Date => "date",
        }
    }

    /// Parses a `strategy = "..."` value. Unrecognized values — including
    /// names reserved for future strategies like `"media"` — are a clear,
    /// explicit error, never a silent fallback to `Type`.
    fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "type" => Ok(OrganizeStrategy::Type),
            "date" => Ok(OrganizeStrategy::Date),
            other => Err(format!(
                "Strategy \"{other}\" is not supported by this Sift version.\nSupported strategies:\n  {}",
                SUPPORTED_STRATEGIES.join("\n  ")
            )),
        }
    }
}

// ------------------------------------------------------------ date source

/// Where the Date strategy's calendar date comes from. `Modified` (the
/// file's filesystem mtime) is the only source implemented in this
/// version — deliberately not "created" (not portably available across
/// platforms), and not content-derived sources (EXIF/ID3/filename
/// parsing), which would require reading file contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DateSource {
    Modified,
}

pub const SUPPORTED_DATE_SOURCES: [&str; 1] = ["modified"];

impl DateSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            DateSource::Modified => "modified",
        }
    }

    fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "modified" => Ok(DateSource::Modified),
            other => Err(format!(
                "Unsupported date_source \"{other}\". Supported: {}.",
                SUPPORTED_DATE_SOURCES.join(", ")
            )),
        }
    }
}

/// A file's date, decomposed into calendar components — always derived
/// from UTC seconds-since-epoch (see `utils::civil_from_unix_secs` for
/// why UTC), never local time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DateMetadata {
    pub year: i64,
    pub month: u32,
    pub day: u32,
}

impl DateMetadata {
    pub fn from_unix_secs(secs: i64) -> Self {
        let (year, month, day) = crate::utils::civil_from_unix_secs(secs);
        DateMetadata { year, month, day }
    }
}

// ---------------------------------------------------------------- template

/// One piece of one path component of a [`Template`]: either literal text
/// that must appear verbatim, or a placeholder that renders to a
/// fixed-width, digit-only value. Digit-only, fixed-width rendering is
/// exactly what keeps a rendered template's *shape* (component count, no
/// new `/`, no `..`, never empty) identical to the validated static
/// template string — see `Template::render`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TemplateSegment {
    Literal(String),
    Year,
    Month,
    Day,
}

impl TemplateSegment {
    fn width(&self) -> Option<usize> {
        match self {
            TemplateSegment::Year => Some(4),
            TemplateSegment::Month | TemplateSegment::Day => Some(2),
            TemplateSegment::Literal(_) => None,
        }
    }
}

/// A small, intentionally limited destination-template renderer — no
/// conditionals, loops, functions, or shell interpolation, just literal
/// text and `{year}`/`{month}`/`{day}` placeholders. Parsed and validated
/// once at config-load time (`Template::parse`); `render` re-validates the
/// concrete rendered path with the exact same relative-path safety check
/// used for `[[rules]]` destinations, so there is never a second, weaker
/// path-safety implementation.
///
/// This is the one mechanism any metadata-driven strategy renders a
/// destination through — a future `media`/`photos`/`audio` strategy reuses
/// this exact type with its own metadata and its own placeholder set
/// added to `parse_component`, never a bespoke path-building routine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    raw: String,
    components: Vec<Vec<TemplateSegment>>,
}

fn parse_component(part: &str) -> Result<Vec<TemplateSegment>, String> {
    let mut segs = Vec::new();
    let mut rest = part;
    while let Some(open) = rest.find('{') {
        if open > 0 {
            segs.push(TemplateSegment::Literal(rest[..open].to_string()));
        }
        let Some(close_rel) = rest[open..].find('}') else {
            return Err(format!(
                "organize.template: unterminated placeholder in \"{part}\""
            ));
        };
        let close = open + close_rel;
        let name = &rest[open + 1..close];
        segs.push(match name {
            "year" => TemplateSegment::Year,
            "month" => TemplateSegment::Month,
            "day" => TemplateSegment::Day,
            other => {
                return Err(format!(
                    "organize.template: unknown placeholder \"{{{other}}}\" (supported: year, month, day)"
                ))
            }
        });
        rest = &rest[close + 1..];
    }
    if !rest.is_empty() {
        segs.push(TemplateSegment::Literal(rest.to_string()));
    }
    if segs.is_empty() {
        return Err("organize.template contains an empty path component".to_string());
    }
    Ok(segs)
}

impl Template {
    /// Parses and statically validates `raw`. The template string itself
    /// (with placeholders still in place) is run through the exact same
    /// relative-path safety check used for `[[rules]]` destinations —
    /// since every placeholder only ever renders to a fixed-width,
    /// digit-only string (never `/`, never `..`, never empty), a template
    /// that passes this check is guaranteed to render safely for every
    /// input, so there is no need to (and no weaker second path exists to)
    /// re-derive safety from scratch per render.
    pub fn parse(raw: &str) -> Result<Self, String> {
        if raw.trim().is_empty() {
            return Err("organize.template must not be empty".to_string());
        }
        if !validate_rule_destination(raw) {
            return Err(format!(
                "organize.template \"{raw}\" is not a safe relative path (no leading '/', no '..', no empty components)"
            ));
        }
        let mut components = Vec::new();
        for comp in Path::new(raw).components() {
            match comp {
                Component::Normal(part) => {
                    let part = part
                        .to_str()
                        .ok_or_else(|| "organize.template contains invalid UTF-8".to_string())?;
                    components.push(parse_component(part)?);
                }
                _ => unreachable!("validate_rule_destination already rejected this"),
            }
        }
        Ok(Template {
            raw: raw.to_string(),
            components,
        })
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// How many path-depth levels this template's rendered destination
    /// spans, e.g. `"{year}/{month}"` → 2.
    pub fn depth(&self) -> usize {
        self.components.len()
    }

    /// Renders this template against `date` into a relative directory
    /// path, re-validating the concrete result with the same safety check
    /// used at parse time (defense in depth; see `parse`'s doc comment for
    /// why this can never actually fail given a template that parsed).
    pub fn render(&self, date: &DateMetadata) -> Result<PathBuf, String> {
        let mut out = PathBuf::new();
        for comp in &self.components {
            let mut s = String::new();
            for seg in comp {
                match seg {
                    TemplateSegment::Literal(l) => s.push_str(l),
                    TemplateSegment::Year => s.push_str(&format!("{:04}", date.year)),
                    TemplateSegment::Month => s.push_str(&format!("{:02}", date.month)),
                    TemplateSegment::Day => s.push_str(&format!("{:02}", date.day)),
                }
            }
            out.push(s);
        }
        if !validate_rule_destination(&out.to_string_lossy()) {
            return Err(format!(
                "rendered destination \"{}\" is not a safe relative path",
                out.display()
            ));
        }
        Ok(out)
    }

    /// Whether `component` could have been produced by rendering this
    /// template's `level`-th path component (0-indexed) for *some* valid
    /// date — a structural match derived directly from the parsed
    /// template (literal pieces must match verbatim; a placeholder piece
    /// must match its fixed digit width), never a byte-for-byte comparison
    /// against one specific rendered value and never an ad hoc heuristic
    /// independent of the actual configured template.
    ///
    /// This is the single mechanism that keeps recursive organize and
    /// Watch from ever re-entering (and thus re-organizing, and thus
    /// endlessly nesting) a directory this same policy would itself
    /// generate.
    pub fn component_could_be_generated(&self, level: usize, component: &str) -> bool {
        let Some(segs) = self.components.get(level) else {
            return false;
        };
        let mut rest = component;
        for seg in segs {
            match seg.width() {
                Some(w) => {
                    if rest.len() < w || !rest.as_bytes()[..w].iter().all(u8::is_ascii_digit) {
                        return false;
                    }
                    rest = &rest[w..];
                }
                None => {
                    let TemplateSegment::Literal(l) = seg else {
                        unreachable!()
                    };
                    if !rest.starts_with(l.as_str()) {
                        return false;
                    }
                    rest = &rest[l.len()..];
                }
            }
        }
        rest.is_empty()
    }
}

// -------------------------------------------------------- unknown policy

/// What happens to an ordinary regular file the classifier cannot put in a
/// specific category. Applies only after normal safety checks, rules, and
/// classification have already run and found nothing more specific — it
/// can never widen what's eligible for mutation, only narrow it (`Skip`)
/// or keep today's default (`Other`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnknownPolicy {
    Other,
    Skip,
}

impl UnknownPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            UnknownPolicy::Other => "other",
            UnknownPolicy::Skip => "skip",
        }
    }

    fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "other" => Ok(UnknownPolicy::Other),
            "skip" => Ok(UnknownPolicy::Skip),
            other => Err(format!(
                "Unsupported unknown policy \"{other}\". Supported: other, skip."
            )),
        }
    }
}

// -------------------------------------------------------------- raw toml

const SUPPORTED_VERSION: i64 = 1;
const MIN_STABILITY_SECONDS: i64 = 1;
const MAX_STABILITY_SECONDS: i64 = 300;

/// Exactly what `toml::from_str` parses `.sift.toml` into. Deliberately
/// close to the file's textual shape; [`Validated`] is the checked,
/// typed result actually used everywhere else. `deny_unknown_fields` is
/// applied at every level: a typo'd key in a file that can drive automatic
/// mutation must be a loud error, never a silently-ignored no-op.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(default)]
    version: Option<i64>,
    #[serde(default)]
    organize: RawOrganize,
    #[serde(default)]
    watch: RawWatch,
    #[serde(default)]
    rules: Vec<Rule>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOrganize {
    #[serde(default)]
    strategy: Option<String>,
    #[serde(default)]
    unknown: Option<String>,
    #[serde(default)]
    template: Option<String>,
    #[serde(default)]
    date_source: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWatch {
    #[serde(default)]
    stability_seconds: Option<i64>,
}

/// The validated, typed result of one `.sift.toml` (or global config) file
/// — everything [`EffectivePolicy`] needs, without a source/version wrapper
/// (those are attached by [`resolve_policy`], which knows where the file
/// came from).
#[derive(Debug, Clone)]
struct Validated {
    version: i64,
    strategy: OrganizeStrategy,
    unknown_policy: UnknownPolicy,
    template: Option<Template>,
    date_source: Option<DateSource>,
    stability: Duration,
    rules: Vec<Rule>,
}

fn validate(raw: RawConfig) -> Result<Validated, String> {
    let version = raw.version.unwrap_or(SUPPORTED_VERSION);
    if version != SUPPORTED_VERSION {
        return Err(format!(
            "Unsupported .sift.toml version {version}.\nThis Sift build supports version {SUPPORTED_VERSION}."
        ));
    }

    let strategy = match &raw.organize.strategy {
        Some(s) => OrganizeStrategy::parse(s)?,
        None => OrganizeStrategy::Type,
    };
    let unknown_policy = match raw.organize.unknown {
        Some(s) => UnknownPolicy::parse(&s)?,
        None => UnknownPolicy::Other,
    };

    // Strong validation over ignored fields: `template`/`date_source` are
    // meaningless for `type` and required for `date` — either mismatch is
    // a clear config error, never a silently-ignored field.
    let (template, date_source) = match strategy {
        OrganizeStrategy::Type => {
            if raw.organize.template.is_some() {
                return Err("organize.template is only valid with strategy = \"date\"".to_string());
            }
            if raw.organize.date_source.is_some() {
                return Err(
                    "organize.date_source is only valid with strategy = \"date\"".to_string(),
                );
            }
            (None, None)
        }
        OrganizeStrategy::Date => {
            let template = match &raw.organize.template {
                Some(t) => Template::parse(t)?,
                None => return Err("strategy = \"date\" requires organize.template".to_string()),
            };
            let date_source = match &raw.organize.date_source {
                Some(s) => DateSource::parse(s)?,
                None => DateSource::Modified,
            };
            (Some(template), Some(date_source))
        }
    };

    let stability = match raw.watch.stability_seconds {
        Some(secs) => {
            if !(MIN_STABILITY_SECONDS..=MAX_STABILITY_SECONDS).contains(&secs) {
                return Err(format!(
                    "watch.stability_seconds must be between {MIN_STABILITY_SECONDS} and {MAX_STABILITY_SECONDS} (got {secs})."
                ));
            }
            Duration::from_secs(secs as u64)
        }
        None => crate::watch::stability::DEFAULT_STABILITY_WINDOW,
    };

    for rule in &raw.rules {
        validate_rule(rule)?;
    }

    Ok(Validated {
        version,
        strategy,
        unknown_policy,
        template,
        date_source,
        stability,
        rules: raw.rules,
    })
}

fn parse_and_validate(text: &str) -> Result<Validated, String> {
    let raw: RawConfig = toml::from_str(text).map_err(|e| format!("invalid .sift.toml: {e}"))?;
    validate(raw)
}

// -------------------------------------------------------- effective policy

/// Where an [`EffectivePolicy`] came from — always shown to the user so the
/// active policy source is never a mystery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PolicySource {
    Local { path: PathBuf },
    Global { path: PathBuf },
    Default,
}

impl PolicySource {
    pub fn describe(&self) -> String {
        match self {
            PolicySource::Local { path } | PolicySource::Global { path } => {
                path.display().to_string()
            }
            PolicySource::Default => "built-in defaults".to_string(),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            PolicySource::Local { .. } => ".sift.toml",
            PolicySource::Global { .. } => "global",
            PolicySource::Default => "built-in defaults",
        }
    }
}

/// The single, fully-resolved, already-validated policy in effect for one
/// root. This is what every consumer (manual organize, recursive organize,
/// Watch, `config check`, `explain`) is handed — never a raw TOML value.
#[derive(Debug, Clone)]
pub struct EffectivePolicy {
    pub source: PolicySource,
    pub version: i64,
    pub strategy: OrganizeStrategy,
    pub unknown_policy: UnknownPolicy,
    /// `Some` iff `strategy == Date` (enforced by `resolve_policy`'s
    /// validation — never constructed any other way outside tests).
    pub template: Option<Template>,
    /// `Some` iff `strategy == Date`.
    pub date_source: Option<DateSource>,
    pub stability: Duration,
    pub rules: Vec<Rule>,
}

impl Default for EffectivePolicy {
    /// Built-in defaults: no local/global config, always valid.
    fn default() -> Self {
        EffectivePolicy {
            source: PolicySource::Default,
            version: SUPPORTED_VERSION,
            strategy: OrganizeStrategy::Type,
            unknown_policy: UnknownPolicy::Other,
            template: None,
            date_source: None,
            stability: crate::watch::stability::DEFAULT_STABILITY_WINDOW,
            rules: Vec::new(),
        }
    }
}

impl EffectivePolicy {
    /// Test/engine convenience: the default policy with an explicit rule
    /// set, used where a caller wants strategy=type/unknown=other defaults
    /// but a specific set of declarative rules (mirrors how `process_candidate`
    /// used to take a bare `&[Rule]`).
    pub fn with_rules(mut self, rules: Vec<Rule>) -> Self {
        self.rules = rules;
        self
    }
}

/// Locates a config file for `start`: only `start/.sift.toml`, then the
/// global config. Does not walk up parent directories. This is a pure path
/// lookup — it does not parse or validate anything found there.
pub fn find_config(start: &str) -> Option<PathBuf> {
    let local = PathBuf::from(start).join(".sift.toml");
    if local.is_file() {
        return Some(local);
    }
    let global = global_config_path()?;
    if global.is_file() {
        return Some(global);
    }
    None
}

fn local_config_path(root: &str) -> PathBuf {
    PathBuf::from(root).join(".sift.toml")
}

// Test-only global config directory override, mirroring
// `history::set_test_history_dir`/`watch::registry::set_test_watch_dir`:
// never used in production, exists purely so tests (including the
// separate-crate integration tests under `tests/`) can exercise the
// "global config" resolution branch without ever touching the real
// user's XDG config directory. Deliberately NOT `#[cfg(test)]`-gated —
// integration tests compile this crate as a plain dependency, where
// `cfg(test)` items from here would not exist at all.
thread_local! {
    static TEST_GLOBAL_CONFIG_DIR: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Set a test-only override for the global config file's *directory*
/// (current thread only). The global config path becomes
/// `<dir>/config.toml`.
pub fn set_test_global_config_dir(dir: PathBuf) {
    TEST_GLOBAL_CONFIG_DIR.with(|c| *c.borrow_mut() = Some(dir));
}

pub fn clear_test_global_config_dir() {
    TEST_GLOBAL_CONFIG_DIR.with(|c| *c.borrow_mut() = None);
}

fn global_config_path() -> Option<PathBuf> {
    if let Some(dir) = TEST_GLOBAL_CONFIG_DIR.with(|c| c.borrow().clone()) {
        return Some(dir.join("config.toml"));
    }
    directories::BaseDirs::new().map(|home| home.config_dir().join("sift").join("config.toml"))
}

/// The single authoritative configuration resolver. Every consumer of
/// policy — manual organize, recursive organize, Watch (validate-before-
/// start, hot reload), `sift config check`, `sift explain` — calls this
/// and only this.
///
/// Resolution order (no arbitrary parent-directory walking, ever):
///   1. `<root>/.sift.toml`, if present. A local file fully replaces any
///      global policy (no merging) — the local file is either the whole
///      effective policy, or (if broken) a hard error; it is never
///      silently patched over with global/default values.
///   2. Otherwise, the global config file, if present. Same fail-closed
///      rule: broken global config is an error, never silently defaulted.
///   3. Otherwise, built-in defaults (always valid).
///
/// A missing file at a given level is not an error — it just means "try
/// the next level." A file that exists but fails to parse or validate
/// always is.
pub fn resolve_policy(root: &str) -> Result<EffectivePolicy, String> {
    let local = local_config_path(root);
    if local.is_file() {
        let text =
            std::fs::read_to_string(&local).map_err(|e| format!("{}: {e}", local.display()))?;
        let v = parse_and_validate(&text).map_err(|e| format!("{}: {e}", local.display()))?;
        return Ok(EffectivePolicy {
            source: PolicySource::Local { path: local },
            version: v.version,
            strategy: v.strategy,
            unknown_policy: v.unknown_policy,
            template: v.template,
            date_source: v.date_source,
            stability: v.stability,
            rules: v.rules,
        });
    }

    if let Some(global) = global_config_path() {
        if global.is_file() {
            let text = std::fs::read_to_string(&global)
                .map_err(|e| format!("{}: {e}", global.display()))?;
            let v = parse_and_validate(&text).map_err(|e| format!("{}: {e}", global.display()))?;
            return Ok(EffectivePolicy {
                source: PolicySource::Global { path: global },
                version: v.version,
                strategy: v.strategy,
                unknown_policy: v.unknown_policy,
                template: v.template,
                date_source: v.date_source,
                stability: v.stability,
                rules: v.rules,
            });
        }
    }

    Ok(EffectivePolicy::default())
}

// ----------------------------------------------------------------- init

/// Writes a starter, canonical v1 `.sift.toml` into a directory.
pub fn cmd_init(path: String, force: bool) {
    let target = PathBuf::from(&path).join(".sift.toml");
    // No-follow, like every other filesystem safety check in this crate:
    // `.exists()` follows symlinks, which would let a symlinked (possibly
    // broken, possibly pointing anywhere) `.sift.toml` cause a plain
    // `fs::write` below to write through it. A symlink here is refused
    // outright, even with `--force` — `--force` authorizes overwriting an
    // existing plain config file, never writing through a symlink.
    match std::fs::symlink_metadata(&target) {
        Ok(md) if md.file_type().is_symlink() => {
            eprintln!(
                "{} is a symlink; refusing to write through it.",
                target.display()
            );
            return;
        }
        Ok(_) if !force => {
            eprintln!(".sift.toml exists, use --force to overwrite");
            return;
        }
        _ => {}
    }
    let example = "\
# Sift Smart Folder configuration (v1)
# This file describes HOW this directory wants to be organized. It never
# starts, stops, or authorizes automatic organization by itself — that is
# controlled separately with `sift watch add/start/pause/stop`.

version = 1

[organize]
# The only strategy implemented in this version. Reuses Sift's existing
# file classifier (Documents/Images/Video/Audio/Archives/3D/Code/Data).
strategy = \"type\"
# What to do with an ordinary file the classifier can't put in a specific
# category: \"other\" moves it to Other/ (the default); \"skip\" leaves it
# untouched instead.
unknown = \"other\"

[watch]
# How long (in seconds, 1-300) a file must sit unchanged before a running
# watch organizes it. Only used if this directory is registered with
# `sift watch add --auto-apply`.
stability_seconds = 3

# Declarative rules always win over the strategy above. Each rule needs a
# pattern and an action (\"Move\", \"Trash\", or \"Skip\"); \"Move\" also
# needs a destination (a plain relative folder name, created if missing).
[[rules]]
enabled = true
priority = 100
pattern = \"*.tmp\"
action = \"Trash\"
description = \"Trash all .tmp files\"

[[rules]]
enabled = true
priority = 90
pattern = \"*.zip\"
action = \"Move\"
destination = \"Archives\"
description = \"Move .zip to Archives\"
";
    match std::fs::write(&target, example) {
        Ok(()) => println!("Wrote {}", target.display()),
        Err(e) => eprintln!("Failed to write {}: {}", target.display(), e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Isolates the "global config" lookup to an empty tempdir for the
    /// duration of `f`, so a test with no local `.sift.toml` can never
    /// accidentally discover the real user's `~/.config/sift/config.toml`.
    /// Every test below that does not write its own local `.sift.toml`
    /// uses this.
    fn with_empty_global<R>(f: impl FnOnce() -> R) -> R {
        let empty_global = tempfile::tempdir().unwrap();
        set_test_global_config_dir(empty_global.path().to_path_buf());
        let r = f();
        clear_test_global_config_dir();
        r
    }

    #[test]
    fn defaults_when_no_config_present() {
        with_empty_global(|| {
            let d = tempfile::tempdir().unwrap();
            let policy = resolve_policy(d.path().to_str().unwrap()).unwrap();
            assert_eq!(policy.source, PolicySource::Default);
            assert_eq!(policy.strategy, OrganizeStrategy::Type);
            assert_eq!(policy.unknown_policy, UnknownPolicy::Other);
            assert_eq!(
                policy.stability,
                crate::watch::stability::DEFAULT_STABILITY_WINDOW
            );
            assert!(policy.rules.is_empty());
        });
    }

    #[test]
    fn global_config_used_when_local_absent() {
        with_empty_global(|| {
            let global_dir = tempfile::tempdir().unwrap();
            std::fs::write(
                global_dir.path().join("config.toml"),
                "[organize]\nunknown = \"skip\"\n",
            )
            .unwrap();
            set_test_global_config_dir(global_dir.path().to_path_buf());
            let d = tempfile::tempdir().unwrap();
            let policy = resolve_policy(d.path().to_str().unwrap()).unwrap();
            assert!(matches!(policy.source, PolicySource::Global { .. }));
            assert_eq!(policy.unknown_policy, UnknownPolicy::Skip);
        });
    }

    #[test]
    fn local_config_wins_over_global() {
        with_empty_global(|| {
            let global_dir = tempfile::tempdir().unwrap();
            std::fs::write(
                global_dir.path().join("config.toml"),
                "[organize]\nunknown = \"skip\"\n",
            )
            .unwrap();
            set_test_global_config_dir(global_dir.path().to_path_buf());
            let d = tempfile::tempdir().unwrap();
            std::fs::write(
                d.path().join(".sift.toml"),
                "[organize]\nunknown = \"other\"\n",
            )
            .unwrap();
            let policy = resolve_policy(d.path().to_str().unwrap()).unwrap();
            assert!(matches!(policy.source, PolicySource::Local { .. }));
            assert_eq!(policy.unknown_policy, UnknownPolicy::Other);
        });
    }

    #[test]
    fn broken_global_config_fails_closed() {
        with_empty_global(|| {
            let global_dir = tempfile::tempdir().unwrap();
            std::fs::write(
                global_dir.path().join("config.toml"),
                "[organize]\nstrategy = \"banana\"\n",
            )
            .unwrap();
            set_test_global_config_dir(global_dir.path().to_path_buf());
            let d = tempfile::tempdir().unwrap();
            assert!(resolve_policy(d.path().to_str().unwrap()).is_err());
        });
    }

    #[test]
    fn valid_v1_local_config_resolved() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join(".sift.toml"),
            "version = 1\n[organize]\nstrategy = \"type\"\nunknown = \"skip\"\n[watch]\nstability_seconds = 7\n",
        )
        .unwrap();
        let policy = resolve_policy(d.path().to_str().unwrap()).unwrap();
        assert!(matches!(policy.source, PolicySource::Local { .. }));
        assert_eq!(policy.unknown_policy, UnknownPolicy::Skip);
        assert_eq!(policy.stability, Duration::from_secs(7));
    }

    #[test]
    fn unsupported_version_rejected() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(".sift.toml"), "version = 99\n").unwrap();
        let err = resolve_policy(d.path().to_str().unwrap()).unwrap_err();
        assert!(err.contains("version 99"));
        assert!(err.contains("version 1"));
    }

    #[test]
    fn legacy_config_without_version_is_v1() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join(".sift.toml"),
            "[[rules]]\nname = \"old\"\npattern = \"*.log\"\naction = \"Trash\"\npriority = 1\nenabled = true\n",
        )
        .unwrap();
        let policy = resolve_policy(d.path().to_str().unwrap()).unwrap();
        assert_eq!(policy.version, 1);
        assert_eq!(policy.rules.len(), 1);
    }

    #[test]
    fn unknown_strategy_rejected() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join(".sift.toml"),
            "[organize]\nstrategy = \"types\"\n",
        )
        .unwrap();
        let err = resolve_policy(d.path().to_str().unwrap()).unwrap_err();
        assert!(err.contains("not supported"));
    }

    #[test]
    fn future_strategy_name_rejected_not_silently_ignored() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join(".sift.toml"),
            "[organize]\nstrategy = \"media\"\n",
        )
        .unwrap();
        let err = resolve_policy(d.path().to_str().unwrap()).unwrap_err();
        assert!(err.contains("media"));
        assert!(err.contains("not supported"));
    }

    #[test]
    fn unknown_policy_rejected() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join(".sift.toml"),
            "[organize]\nunknown = \"others\"\n",
        )
        .unwrap();
        let err = resolve_policy(d.path().to_str().unwrap()).unwrap_err();
        assert!(err.contains("Unsupported unknown policy"));
    }

    #[test]
    fn zero_stability_rejected() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join(".sift.toml"),
            "[watch]\nstability_seconds = 0\n",
        )
        .unwrap();
        assert!(resolve_policy(d.path().to_str().unwrap()).is_err());
    }

    #[test]
    fn negative_stability_rejected() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join(".sift.toml"),
            "[watch]\nstability_seconds = -1\n",
        )
        .unwrap();
        assert!(resolve_policy(d.path().to_str().unwrap()).is_err());
    }

    #[test]
    fn excessive_stability_rejected() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join(".sift.toml"),
            "[watch]\nstability_seconds = 301\n",
        )
        .unwrap();
        assert!(resolve_policy(d.path().to_str().unwrap()).is_err());
    }

    #[test]
    fn unsafe_rule_destination_rejected_at_load_time() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join(".sift.toml"),
            "[[rules]]\npattern = \"*.zip\"\naction = \"Move\"\ndestination = \"../escape\"\npriority = 1\nenabled = true\n",
        )
        .unwrap();
        assert!(resolve_policy(d.path().to_str().unwrap()).is_err());
    }

    #[test]
    fn invalid_action_rejected_at_load_time() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join(".sift.toml"),
            "[[rules]]\npattern = \"*.zip\"\naction = \"Copy\"\npriority = 1\nenabled = true\n",
        )
        .unwrap();
        assert!(resolve_policy(d.path().to_str().unwrap()).is_err());
    }

    #[test]
    fn malformed_toml_rejected() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(".sift.toml"), "this is not [ valid toml\n").unwrap();
        assert!(resolve_policy(d.path().to_str().unwrap()).is_err());
    }

    #[test]
    fn unknown_top_level_field_rejected() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join(".sift.toml"),
            "[general]\napply_by_default = true\n",
        )
        .unwrap();
        assert!(resolve_policy(d.path().to_str().unwrap()).is_err());
    }

    #[test]
    fn rule_without_explicit_name_uses_pattern_as_label() {
        let rule = Rule {
            name: String::new(),
            pattern: "*.torrent".into(),
            action: "Move".into(),
            destination: Some("Torrents".into()),
            priority: 100,
            enabled: true,
            description: None,
        };
        assert_eq!(rule.label(), "*.torrent");
    }

    #[test]
    fn no_arbitrary_parent_walking() {
        with_empty_global(|| {
            let d = tempfile::tempdir().unwrap();
            let child = d.path().join("nested");
            std::fs::create_dir(&child).unwrap();
            std::fs::write(
                d.path().join(".sift.toml"),
                "[organize]\nunknown = \"skip\"\n",
            )
            .unwrap();
            // The parent has a config; the child does not. Resolving the
            // child must never discover the parent's file.
            let policy = resolve_policy(child.to_str().unwrap()).unwrap();
            assert_eq!(policy.source, PolicySource::Default);
            assert_eq!(policy.unknown_policy, UnknownPolicy::Other);
        });
    }
}
