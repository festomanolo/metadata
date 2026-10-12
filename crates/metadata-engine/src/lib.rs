//! Extraction engine: detection → backends (native + external) → merged `FileReport`.
pub mod backends;
pub mod capabilities;
pub mod detect;
pub mod explain;
pub mod fsinfo;
pub mod native;
pub mod scan;
pub mod search;

use metadata_core::{hash, BackendUsed, Field, FileReport, Warning, SCHEMA_VERSION};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub use backends::{BackendStatus, Backends};
pub use scan::{collect_files, ScanOptions};

/// Resource limits protecting against hostile input.
#[derive(Debug, Clone)]
pub struct Limits {
    /// Max bytes read for in-memory structural parsing of one file.
    pub max_structural_bytes: u64,
    pub max_archive_entries: usize,
    /// Max bytes read from a single archive entry or XML part.
    pub max_entry_bytes: u64,
    pub max_total_uncompressed: u64,
    /// Compression ratio above which an entry is flagged.
    pub max_ratio: u64,
    pub max_depth: usize,
    /// Cap on external-tool stdout.
    pub max_tool_output: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_structural_bytes: 64 << 20,
            max_archive_entries: 20_000,
            max_entry_bytes: 8 << 20,
            max_total_uncompressed: 4 << 30,
            max_ratio: 200,
            max_depth: 3,
            max_tool_output: 32 << 20,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Options {
    /// Include raw/unknown/rarely-used fields and every archive entry.
    pub all: bool,
    /// Descend into nested archives.
    pub deep: bool,
    pub hash: bool,
    /// Allow external backends (exiftool/ffprobe) when installed.
    pub use_external: bool,
    pub timeout_secs: u64,
    pub limits: Limits,
    pub cancel: Option<Arc<AtomicBool>>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            all: false,
            deep: false,
            hash: true,
            use_external: true,
            timeout_secs: 30,
            limits: Limits::default(),
            cancel: None,
        }
    }
}

/// What one extractor contributes.
#[derive(Default)]
pub struct Extraction {
    pub fields: Vec<Field>,
    pub warnings: Vec<Warning>,
    pub unavailable: Vec<String>,
}

impl Extraction {
    pub fn warn(&mut self, backend: &str, msg: impl Into<String>) {
        self.warnings.push(Warning {
            backend: backend.into(),
            message: msg.into(),
        });
    }
}

pub struct Engine {
    pub backends: Backends,
    pub opts: Options,
}

impl Engine {
    pub fn new(opts: Options) -> Self {
        Engine {
            backends: Backends::discover(),
            opts,
        }
    }

    /// Never fails: problems are recorded in `FileReport::error` / `warnings`.
    pub fn inspect(&self, path: &Path) -> FileReport {
        let mut report = FileReport {
            schema_version: SCHEMA_VERSION.into(),
            path: path.display().to_string(),
            size: 0,
            sha256: None,
            format: unknown_format(),
            filesystem: fsinfo::fs_info(path),
            fields: vec![],
            warnings: vec![],
            backends: vec![],
            unavailable: vec![],
            error: None,
        };
        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) => {
                report.error = Some(format!("{}: {e}", path.display()));
                return report;
            }
        };
        if !meta.is_file() {
            report.error = Some("not a regular file".into());
            return report;
        }
        report.size = meta.len();
        let det = match detect::detect_file(path) {
            Ok(d) => d,
            Err(e) => {
                report.error = Some(format!("cannot read: {e}"));
                return report;
            }
        };
        report.format = det.info.clone();
        let mut ex = Extraction::default();
        native::extract(path, &det.info, &self.opts, &mut ex);
        report.backends.push(BackendUsed {
            name: "native".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        });
        if self.opts.use_external {
            self.backends.extract_external(
                path,
                &det.info,
                &self.opts,
                &mut ex,
                &mut report.backends,
            );
        }
        if det.info.extension_match == metadata_core::ExtensionMatch::Mismatch {
            ex.warn(
                "native",
                format!("extension does not match detected format ({})", det.info.id),
            );
        }
        // Dedupe identical fields reported by several backends (keep first = most trusted).
        let mut seen = std::collections::HashSet::new();
        ex.fields
            .retain(|f| seen.insert((f.group.clone(), f.name.clone(), f.raw.clone())));
        report.fields = ex.fields;
        report.warnings = ex.warnings;
        report.unavailable = ex.unavailable;
        if self.opts.hash {
            match hash::sha256_file(path) {
                Ok(h) => report.sha256 = Some(h),
                Err(e) => report.warnings.push(Warning {
                    backend: "native".into(),
                    message: format!("hash failed: {e}"),
                }),
            }
        }
        report
    }
}

fn unknown_format() -> metadata_core::FormatInfo {
    metadata_core::FormatInfo {
        id: "unknown".into(),
        description: "Unknown".into(),
        mime: "application/octet-stream".into(),
        category: metadata_core::Category::Other,
        extension_match: metadata_core::ExtensionMatch::Unknown,
        signature_hex: String::new(),
    }
}
