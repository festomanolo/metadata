//! Versioned evidence manifest with verifiable content hashes.
//!
//! A manifest digest proves the *manifest content* is unchanged relative to a trusted copy of the
//! digest, and per-file hashes prove file *content equality* with what was recorded. Neither proves
//! authorship or when a file was created. `recorded_at` is the local clock at intake and is not
//! independently verified.
use chrono::{SecondsFormat, Utc};
use metadata_core::hash::{digest_file, sha256_bytes, Algorithm};
use metadata_core::{MetaError, Result, TimeValue};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MANIFEST_SCHEMA: &str = "metadata.manifest/1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManifestEntry {
    /// Path relative to `root` using `/` separators.
    pub path: String,
    pub size: u64,
    pub hash: String,
    pub modified: Option<TimeValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub schema: String,
    pub tool: String,
    pub algorithm: Algorithm,
    /// Local clock at creation; NOT independently verified.
    pub recorded_at: String,
    pub root: String,
    pub entries: Vec<ManifestEntry>,
    /// Digest over the canonical JSON of `entries` (+algorithm); detects accidental/unsynchronized edits.
    pub entries_digest: String,
    pub limitations: String,
}

pub const LIMITATIONS: &str = "Hashes verify content equality against this record only. They do not establish authorship, \
original creation time, or that this manifest itself is authentic; recorded_at is the intake machine's local clock.";

fn digest_entries(alg: Algorithm, entries: &[ManifestEntry]) -> String {
    let canon = serde_json::to_vec(&(alg, entries)).expect("serializable");
    sha256_bytes(&canon)
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// Build a manifest for `files` (all under `root`, or absolute).
pub fn create(root: &Path, files: &[PathBuf], alg: Algorithm) -> Result<Manifest> {
    let mut entries = Vec::with_capacity(files.len());
    for p in files {
        let md = std::fs::metadata(p).map_err(|e| MetaError::io(p, e))?;
        entries.push(ManifestEntry {
            path: rel(root, p),
            size: md.len(),
            hash: digest_file(p, alg)?,
            modified: md.modified().ok().map(TimeValue::from_system),
        });
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let entries_digest = digest_entries(alg, &entries);
    Ok(Manifest {
        schema: MANIFEST_SCHEMA.into(),
        tool: format!("metadata {}", env!("CARGO_PKG_VERSION")),
        algorithm: alg,
        recorded_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        root: root.display().to_string(),
        entries,
        entries_digest,
        limitations: LIMITATIONS.into(),
    })
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EntryStatus {
    Ok,
    Modified,
    Missing,
    Unreadable,
}

#[derive(Debug, Clone, Serialize)]
pub struct EntryResult {
    pub path: String,
    pub status: EntryStatus,
    pub expected: String,
    pub actual: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerifyReport {
    pub manifest_digest_ok: bool,
    pub results: Vec<EntryResult>,
    /// Files under root present on disk but absent from the manifest (only when requested).
    pub extra: Vec<String>,
}

impl VerifyReport {
    pub fn all_ok(&self) -> bool {
        self.manifest_digest_ok
            && self.extra.is_empty()
            && self.results.iter().all(|r| r.status == EntryStatus::Ok)
    }
}

pub fn load(path: &Path) -> Result<Manifest> {
    let b = std::fs::read(path).map_err(|e| MetaError::io(path, e))?;
    let m: Manifest = serde_json::from_slice(&b)
        .map_err(|e| MetaError::InvalidArgument(format!("not a valid manifest: {e}")))?;
    if !m.schema.starts_with("metadata.manifest/") {
        return Err(MetaError::InvalidArgument(format!(
            "unknown manifest schema '{}'",
            m.schema
        )));
    }
    Ok(m)
}

/// Verify files under `root`. `extra_files`: when `Some`, lists on-disk files to flag if not in the manifest.
pub fn verify(m: &Manifest, root: &Path, extra_files: Option<&[PathBuf]>) -> VerifyReport {
    let manifest_digest_ok = digest_entries(m.algorithm, &m.entries) == m.entries_digest;
    let mut results = vec![];
    for e in &m.entries {
        // Refuse entries that escape the root.
        let safe = !e.path.starts_with('/')
            && !e.path.split('/').any(|c| c == "..")
            && !e.path.contains(':');
        let p = root.join(&e.path);
        let (status, actual) = if !safe {
            (EntryStatus::Unreadable, None)
        } else if !p.exists() {
            (EntryStatus::Missing, None)
        } else {
            match digest_file(&p, m.algorithm) {
                Ok(h) if h == e.hash => (EntryStatus::Ok, Some(h)),
                Ok(h) => (EntryStatus::Modified, Some(h)),
                Err(_) => (EntryStatus::Unreadable, None),
            }
        };
        results.push(EntryResult {
            path: e.path.clone(),
            status,
            expected: e.hash.clone(),
            actual,
        });
    }
    let extra = extra_files
        .map(|fs| {
            let known: std::collections::HashSet<&str> =
                m.entries.iter().map(|e| e.path.as_str()).collect();
            fs.iter()
                .map(|p| rel(root, p))
                .filter(|r| !known.contains(r.as_str()))
                .collect()
        })
        .unwrap_or_default();
    VerifyReport {
        manifest_digest_ok,
        results,
        extra,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (tempfile::TempDir, Vec<PathBuf>) {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a.txt");
        let b = d.path().join("sub");
        std::fs::create_dir(&b).unwrap();
        let b = b.join("b.txt");
        std::fs::write(&a, "one").unwrap();
        std::fs::write(&b, "two").unwrap();
        (d, vec![a, b])
    }
    #[test]
    fn roundtrip_ok() {
        let (d, files) = setup();
        let m = create(d.path(), &files, Algorithm::Sha256).unwrap();
        assert_eq!(m.entries[1].path, "sub/b.txt");
        assert!(verify(&m, d.path(), None).all_ok());
    }
    #[test]
    fn detects_modified_missing_and_tampered_manifest() {
        let (d, files) = setup();
        let mut m = create(d.path(), &files, Algorithm::Sha256).unwrap();
        std::fs::write(&files[0], "changed").unwrap();
        std::fs::remove_file(&files[1]).unwrap();
        let r = verify(&m, d.path(), None);
        assert_eq!(r.results[0].status, EntryStatus::Modified);
        assert_eq!(r.results[1].status, EntryStatus::Missing);
        m.entries[0].hash = "00".into();
        assert!(!verify(&m, d.path(), None).manifest_digest_ok);
    }
    #[test]
    fn rejects_traversal_entries() {
        let (d, files) = setup();
        let mut m = create(d.path(), &files, Algorithm::Sha256).unwrap();
        m.entries[0].path = "../../etc/passwd".into();
        m.entries_digest = digest_entries(m.algorithm, &m.entries);
        assert_eq!(
            verify(&m, d.path(), None).results[0].status,
            EntryStatus::Unreadable
        );
    }
    #[test]
    fn flags_extra_files() {
        let (d, files) = setup();
        let m = create(d.path(), &files[..1], Algorithm::Sha256).unwrap();
        let r = verify(&m, d.path(), Some(&files));
        assert_eq!(r.extra, vec!["sub/b.txt".to_string()]);
    }
}
