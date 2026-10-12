//! Versioned output schema. Breaking changes bump `SCHEMA_VERSION` major.
use crate::time::TimeValue;
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: &str = "1.0.0";

/// Where a value came from, so users can judge reliability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    /// Stored inside the file's own metadata structures.
    Stored,
    /// Derived by a parser from file content (e.g. dimensions, counts).
    Inferred,
    /// Reported by the filesystem (mutable by copy/extract/sync).
    Filesystem,
    /// Computed by this tool (hashes, detection).
    Computed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub group: String,
    pub name: String,
    /// Value exactly as the backend reported it.
    pub raw: String,
    /// Typed normalized value, when one exists.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub normalized: Option<serde_json::Value>,
    /// Parsed time, preserving whether a timezone/offset was present.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub time: Option<TimeValue>,
    pub backend: String,
    pub provenance: Provenance,
    /// `Some(true/false)` only if verified for this format+backend; `None` = unknown.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub writable: Option<bool>,
    /// Personal/location data: redacted in logs and default reports.
    #[serde(default)]
    pub sensitive: bool,
}

impl Field {
    pub fn new(
        group: &str,
        name: &str,
        raw: impl Into<String>,
        backend: &str,
        provenance: Provenance,
    ) -> Self {
        let name_s = name.to_string();
        Field {
            group: group.into(),
            sensitive: is_sensitive_name(&name_s),
            name: name_s,
            raw: raw.into(),
            normalized: None,
            time: None,
            backend: backend.into(),
            provenance,
            writable: None,
        }
    }
    pub fn with_time(mut self, t: Option<TimeValue>) -> Self {
        self.time = t;
        self
    }
    pub fn with_normalized(mut self, v: serde_json::Value) -> Self {
        self.normalized = Some(v);
        self
    }
    /// `Group:Name`, the identifier used by `--set` / `--field`.
    pub fn key(&self) -> String {
        format!("{}:{}", self.group, self.name)
    }
}

pub fn is_sensitive_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("gps")
        || ["author", "creator", "lastmodifiedby", "lastsavedby", "artist", "owner", "serial", "copyright", "email", "username"]
            .iter()
            .any(|k| n.contains(k))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionMatch {
    Match,
    Mismatch,
    /// No extension, or detected format has no canonical extension list.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormatInfo {
    /// Short id, e.g. "jpeg", "pdf", "docx", "zip", "unknown".
    pub id: String,
    pub description: String,
    pub mime: String,
    pub category: Category,
    pub extension_match: ExtensionMatch,
    /// First bytes (hex) the detection was based on.
    pub signature_hex: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Image,
    Video,
    Audio,
    Document,
    Archive,
    Source,
    Text,
    Other,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FsInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<TimeValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<TimeValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accessed: Option<TimeValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<String>,
    pub is_symlink: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Warning {
    pub backend: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackendUsed {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileReport {
    pub schema_version: String,
    pub path: String,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    pub format: FormatInfo,
    pub filesystem: FsInfo,
    pub fields: Vec<Field>,
    pub warnings: Vec<Warning>,
    pub backends: Vec<BackendUsed>,
    /// Field families this file's format could expose but no active backend could read.
    pub unavailable: Vec<String>,
    /// Set when extraction failed entirely.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl FileReport {
    /// All fields whose name matches (case-insensitive), optionally `Group:Name`.
    pub fn find(&self, name: &str) -> Vec<&Field> {
        let (g, n) = match name.split_once(':') {
            Some((g, n)) => (Some(g), n),
            None => (None, name),
        };
        self.fields
            .iter()
            .filter(|f| {
                f.name.eq_ignore_ascii_case(n)
                    && g.is_none_or(|g| f.group.eq_ignore_ascii_case(g))
            })
            .collect()
    }
}
