use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InlineMode {
    All,
    Tagged,
    Off,
}
impl Default for InlineMode {
    fn default() -> Self { Self::All }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EmphasisDialect {
    Commonmark,
    #[serde(rename = "star-bold")]
    StarBold,
}
impl Default for EmphasisDialect {
    fn default() -> Self { Self::Commonmark }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnknownTags {
    Ignore,
    Highlight,
}
impl Default for UnknownTags {
    fn default() -> Self { Self::Ignore }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Off,
    Hint,
    Information,
    Warning,
    Error,
}
impl Severity {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "hint" => Some(Self::Hint),
            "information" | "info" => Some(Self::Information),
            "warning" | "warn" => Some(Self::Warning),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Hint => "hint",
            Self::Information => "information",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}
impl Default for Severity {
    fn default() -> Self { Self::Off }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyntaxConfig {
    #[serde(default = "default_true")]
    pub conventional: bool,
    #[serde(default = "default_true")]
    pub delimited: bool,
    #[serde(default)]
    pub inline: InlineMode,
    #[serde(default)]
    pub emphasis: EmphasisDialect,
    #[serde(default)]
    pub unknown_tags: UnknownTags,
}
impl Default for SyntaxConfig {
    fn default() -> Self {
        Self { conventional: true, delimited: true, inline: InlineMode::All, emphasis: EmphasisDialect::Commonmark, unknown_tags: UnknownTags::Ignore }
    }
}

fn default_true() -> bool { true }
fn default_true_opt() -> bool { true }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagConfig {
    pub name: String,
    pub class: String,
    #[serde(default)]
    pub severity: Severity,
    #[serde(default = "default_true_opt")]
    pub symbol: bool,
    #[serde(default)]
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverrideConfig {
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub syntax: SyntaxConfig,
    #[serde(default)]
    pub tags: Vec<TagConfig>,
    #[serde(default)]
    pub overrides: Vec<OverrideConfig>,
}

fn default_version() -> u32 { 1 }

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            syntax: SyntaxConfig::default(),
            tags: default_tags(),
            overrides: Vec::new(),
        }
    }
}

pub fn default_tags() -> Vec<TagConfig> {
    vec![
        TagConfig { name: "NOTE".into(), class: "note".into(), severity: Severity::Off, symbol: true, aliases: vec![] },
        TagConfig { name: "TODO".into(), class: "todo".into(), severity: Severity::Hint, symbol: true, aliases: vec![] },
        TagConfig { name: "FIXME".into(), class: "fixme".into(), severity: Severity::Information, symbol: true, aliases: vec!["XXX".into()] },
        TagConfig { name: "BUG".into(), class: "bug".into(), severity: Severity::Warning, symbol: true, aliases: vec![] },
        TagConfig { name: "HACK".into(), class: "hack".into(), severity: Severity::Information, symbol: true, aliases: vec![] },
    ]
}

impl Config {
    pub fn from_toml_str(s: &str) -> Result<Self, ConfigError> {
        // Start from defaults, then overlay parsed TOML if it has fields.
        // We do this by deserializing into a partial struct.
        #[derive(Deserialize, Serialize, Default)]
        struct Partial {
            version: Option<u32>,
            syntax: Option<SyntaxConfig>,
            tags: Option<Vec<TagConfig>>,
            overrides: Option<Vec<OverrideConfig>>,
        }
        let partial: Partial = toml::from_str(s).map_err(|e| ConfigError::Parse(e.to_string()))?;
        let mut cfg = Config::default();
        if let Some(v) = partial.version { cfg.version = v; }
        if let Some(syn) = partial.syntax { cfg.syntax = syn; }
        if let Some(tags) = partial.tags { cfg.tags = tags; }
        if let Some(over) = partial.overrides { cfg.overrides = over; }
        Ok(cfg)
    }

    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let contents = std::fs::read_to_string(path).map_err(|e| ConfigError::Io(e.to_string()))?;
        Self::from_toml_str(&contents)
    }

    /// Look up a tag (including aliases). Returns (class, severity, symbol).
    pub fn lookup_tag(&self, tag: &str) -> Option<(String, Severity, bool)> {
        for t in &self.tags {
            if t.name == tag || t.aliases.iter().any(|a| a == tag) {
                return Some((t.class.clone(), t.severity, t.symbol));
            }
        }
        None
    }

    pub fn tag_names_set(&self) -> Vec<String> {
        self.tags.iter().map(|t| t.name.clone()).collect()
    }

    /// Whether this config file enables scanning for the given path
    /// (vendor/third_party overrides etc.).
    pub fn enabled_for_path(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        for over in &self.overrides {
            if over.paths.iter().any(|glob_pat| {
                if glob_pat.contains('*') {
                    // simple glob matching using `glob` crate Pattern
                    glob::Pattern::new(glob_pat).map(|p| p.matches(&path_str)).unwrap_or(false)
                        || glob::Pattern::new(glob_pat).map(|p| p.matches(path_str.trim_start_matches("./"))).unwrap_or(false)
                } else {
                    path_str.contains(glob_pat)
                }
            }) {
                return over.enabled;
            }
        }
        true
    }

    /// Find project config by walking up from `start_dir`, plus global fallback.
    pub fn load_for_file(start_path: &Path, global_override: Option<&Path>) -> Result<Self, ConfigError> {
        let mut current: Option<&Path> = start_path.parent();
        while let Some(dir) = current {
            for cand in [dir.join("anchors.toml"), dir.join(".anchors.toml"), dir.join("comment-annotations.toml"), dir.join(".comment-annotations.toml")] {
                if cand.exists() {
                    // project config overrides global: start from global then overlay project?
                    // Simple: if project exists, use it (merged over global if global exists).
                    let global_cfg = global_override.and_then(|g| if g.exists() { Self::from_file(g).ok() } else { None })
                        .or_else(|| default_global_config_path().and_then(|g| if g.exists() { Self::from_file(&g).ok() } else { None }));
                    let project_cfg = Self::from_file(&cand)?;
                    if let Some(g) = global_cfg {
                        return Ok(merge_configs(g, project_cfg));
                    } else {
                        return Ok(project_cfg);
                    }
                }
            }
            current = dir.parent();
        }
        if let Some(g) = global_override {
            if g.exists() { return Self::from_file(g); }
        } else if let Some(g) = default_global_config_path() {
            if g.exists() { return Self::from_file(&g); }
        }
        Ok(Config::default())
    }
}

pub fn default_global_config_path() -> Option<PathBuf> {
    dirs_config().map(|d| d.join("canc").join("anchors.toml"))
}

fn dirs_config() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
}

pub fn merge_configs(global: Config, project: Config) -> Config {
    // Project syntax wins if project specified non-default? Simplified: project wins.
    // Tags: union by name, project overrides global.
    let mut tags_by_name: HashMap<String, TagConfig> = global.tags.iter().map(|t| (t.name.clone(), t.clone())).collect();
    for t in project.tags { tags_by_name.insert(t.name.clone(), t); }
    let mut tags: Vec<_> = tags_by_name.into_values().collect();
    tags.sort_by(|a, b| a.name.cmp(&b.name));
    // Preserve default tag order for those without sorting? Keep sorted for determinism.
    Config { version: project.version, syntax: project.syntax, tags, overrides: project.overrides }
}

#[derive(Debug)]
pub enum ConfigError {
    Parse(String),
    Io(String),
}
impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self { Self::Parse(s) => write!(f, "config parse error: {s}"), Self::Io(s) => write!(f, "config io error: {s}") }
    }
}
impl std::error::Error for ConfigError {}
