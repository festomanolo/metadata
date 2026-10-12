//! User configuration (TOML). Precedence: CLI flags > config file > built-in defaults.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    /// "human" | "json" | "csv" — default for commands that accept --format.
    pub default_format: Option<String>,
    /// Worker threads for scans (default: CPU count, max 16).
    pub jobs: Option<usize>,
    /// Per external-tool timeout in seconds.
    pub timeout_secs: Option<u64>,
    /// Use exiftool/ffprobe when installed.
    pub use_external: Option<bool>,
    /// Compute SHA-256 for every inspected file.
    pub hash: Option<bool>,
    /// Mask GPS/personal identifiers in output by default.
    pub redact: Option<bool>,
    pub limits: LimitsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LimitsConfig {
    pub max_archive_entries: Option<usize>,
    pub max_entry_mib: Option<u64>,
    pub max_total_uncompressed_mib: Option<u64>,
    pub max_ratio: Option<u64>,
    pub max_depth: Option<usize>,
    pub max_structural_mib: Option<u64>,
}

pub fn config_path() -> PathBuf {
    if let Some(p) = std::env::var_os("METADATA_CONFIG") {
        return PathBuf::from(p);
    }
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.unwrap_or_else(|| PathBuf::from(".")).join("metadata").join("config.toml")
}

pub fn load() -> anyhow::Result<Config> {
    let p = config_path();
    match std::fs::read_to_string(&p) {
        Ok(s) => toml::from_str(&s).map_err(|e| anyhow::anyhow!("invalid config {}: {e}", p.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(anyhow::anyhow!("cannot read config {}: {e}", p.display())),
    }
}

pub const TEMPLATE: &str = r#"# metadata configuration. Every key is optional; flags override these values.
# default_format = "human"     # human | json | csv
# jobs = 8                      # parallel workers
# timeout_secs = 30             # per external tool
# use_external = true           # use exiftool/ffprobe when installed
# hash = true                   # SHA-256 for each inspected file
# redact = false                # mask GPS / personal identifiers in output

[limits]
# max_archive_entries = 20000
# max_entry_mib = 8
# max_total_uncompressed_mib = 4096
# max_ratio = 200
# max_depth = 3
# max_structural_mib = 64
"#;
