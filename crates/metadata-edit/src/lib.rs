//! Safe metadata editing: plan → write to temp → re-extract & verify → atomic replace → log.
//!
//! The source file is never modified unless the caller asks to replace it, and then only after a backup
//! (unless disabled) and a successful verification of the temporary output.
pub mod exiftool;
pub mod native;

use metadata_core::hash::sha256_file;
use metadata_core::{time, Field, FileReport, MetaError, Result};
use metadata_engine::Engine;
use native::{Resolved, WProfile};
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum Op {
    Set(String, String),
    Remove(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    /// Personal identifiers and embedded metadata blocks likely to identify a person or place.
    Privacy,
    /// Everything the backend can remove.
    All,
    /// Location data only (exiftool backend).
    Gps,
}

impl From<Profile> for WProfile {
    fn from(p: Profile) -> WProfile {
        match p {
            Profile::Privacy => WProfile::Privacy,
            Profile::All => WProfile::All,
            Profile::Gps => WProfile::Gps,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct EditRequest {
    pub ops: Vec<Op>,
    pub profile: Option<Profile>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Change {
    pub key: String,
    pub action: &'static str,
    pub before: Option<String>,
    pub after: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub source: String,
    pub format: String,
    pub backend: String,
    pub changes: Vec<Change>,
    pub warnings: Vec<String>,
    pub signature_risk: bool,
    pub residual_note: Option<String>,
    #[serde(skip)]
    resolved: Vec<Resolved>,
    #[serde(skip)]
    profile: Option<Profile>,
    #[serde(skip)]
    before: Option<FileReport>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backup {
    None,
    /// Copy the original beside it before replacing (only relevant when replacing in place).
    Copy,
}

#[derive(Debug, Clone)]
pub struct ApplyOpts {
    pub backup: Backup,
    /// Required to write over the source file.
    pub replace_original: bool,
    /// Required to proceed when the file appears to be digitally signed.
    pub allow_signature_loss: bool,
    /// Replace an existing, different destination file.
    pub force: bool,
    pub log_path: Option<PathBuf>,
    pub timeout: Duration,
}

impl Default for ApplyOpts {
    fn default() -> Self {
        ApplyOpts {
            backup: Backup::Copy,
            replace_original: false,
            allow_signature_loss: false,
            force: false,
            log_path: None,
            timeout: Duration::from_secs(60),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub key: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EditOutcome {
    pub source: String,
    pub destination: String,
    pub backup: Option<String>,
    pub backend: String,
    pub source_sha256: String,
    pub destination_sha256: String,
    pub verified: bool,
    pub checks: Vec<Check>,
    /// Fields that differ between the original and the result without having been requested.
    pub unexpected_changes: Vec<String>,
    pub residual_note: Option<String>,
}

fn field_value<'a>(r: &'a FileReport, key: &str) -> Option<&'a Field> {
    r.find(key).into_iter().next()
}

/// Fields a profile is expected to remove, derived from the original report.
fn profile_targets(format: &str, p: Profile, r: &FileReport, backend_native: bool) -> Vec<String> {
    let keep_structural = |f: &&Field| {
        !matches!(
            f.group.as_str(),
            "File" | "Archive" | "Document" | "OOXML-Custom"
        ) || f.name == "Comment"
    };
    r.fields
        .iter()
        .filter(keep_structural)
        .filter(|f| match (p, backend_native) {
            (Profile::Gps, _) => f.group == "GPS" || f.name.starts_with("GPS"),
            (_, true) => match format {
                "jpeg" => {
                    matches!(
                        f.group.as_str(),
                        "EXIF" | "GPS" | "Interop" | "EXIF-Thumbnail" | "XMP" | "IPTC"
                    ) || f.key() == "File:Comment"
                }
                "png" => {
                    f.group == "PNG"
                        && !matches!(
                            f.name.as_str(),
                            "BitDepth"
                                | "ColorType"
                                | "Interlace"
                                | "PixelsPerUnit"
                                | "ImageDataBytes"
                                | "ProfileName"
                        )
                        || f.group == "XMP"
                        || f.group == "EXIF"
                }
                "pdf" => {
                    (f.group == "PDF"
                        && !matches!(f.name.as_str(), "PDFVersion" | "Revisions" | "PageCount"))
                        && (p == Profile::All || matches!(f.name.as_str(), "Author" | "Creator"))
                        || f.group == "XMP"
                }
                _ => {
                    native::keys(format)
                        .iter()
                        .any(|k| k.eq_ignore_ascii_case(&f.key()))
                        && (p == Profile::All || is_personal(&f.name))
                }
            },
            (_, false) => {
                !matches!(f.group.as_str(), "ICC" | "JFIF")
                    && (p == Profile::All
                        || metadata_core::is_sensitive_name(&f.name)
                        || matches!(f.group.as_str(), "GPS" | "EXIF" | "IPTC" | "XMP"))
            }
        })
        .map(|f| f.key())
        .collect()
}

fn is_personal(name: &str) -> bool {
    matches!(
        name,
        "Creator" | "LastModifiedBy" | "Company" | "Manager" | "InitialCreator"
    )
}

pub fn plan(engine: &Engine, src: &Path, req: &EditRequest) -> Result<Plan> {
    if req.ops.is_empty() && req.profile.is_none() {
        return Err(MetaError::InvalidArgument(
            "nothing to do: give --set/--remove/--profile".into(),
        ));
    }
    let mut report = engine.inspect(src);
    if let Some(e) = &report.error {
        return Err(MetaError::Other(e.clone()));
    }
    // The planning read must never include sensitive redaction; we need real before-values.
    let fmt = report.format.id.clone();
    let wprofile: Option<WProfile> = req.profile.map(Into::into);
    let native_ok = !native::keys(&fmt).is_empty()
        && wprofile.is_none_or(|p| native::supports_profile(&fmt, p));
    let mut resolved = vec![];
    let mut native_err = None;
    if native_ok {
        for op in &req.ops {
            let (k, v) = match op {
                Op::Set(k, v) => (k, Some(v.clone())),
                Op::Remove(k) => (k, None),
            };
            match native::resolve_key(&fmt, k) {
                Ok(key) => resolved.push(Resolved { key, value: v }),
                Err(e) => {
                    native_err = Some(e);
                    break;
                }
            }
        }
    }
    let use_native = native_ok && native_err.is_none();
    let mut warnings = vec![];
    let backend = if use_native {
        "native"
    } else if engine.backends.exiftool.is_some()
        && !matches!(
            report.format.category,
            metadata_core::Category::Archive
                | metadata_core::Category::Text
                | metadata_core::Category::Source
        )
    {
        resolved = req
            .ops
            .iter()
            .map(|op| match op {
                Op::Set(k, v) => Resolved {
                    key: k.clone(),
                    value: Some(v.clone()),
                },
                Op::Remove(k) => Resolved {
                    key: k.clone(),
                    value: None,
                },
            })
            .collect();
        warnings.push("written by exiftool: results are re-read and verified, but exiftool support is not covered by this project's automated tests on a real exiftool".into());
        "exiftool"
    } else {
        return Err(native_err.unwrap_or_else(|| {
            MetaError::Unsupported(format!(
                "{fmt} metadata cannot be written without exiftool (not installed). Natively writable formats: docx/xlsx/pptx, odt/ods/odp, pdf, png (text chunks), jpeg (comment/strip)"
            ))
        }));
    };
    let mut changes = vec![];
    for r in &resolved {
        let before = field_value(&report, &r.key).map(|f| f.raw.clone());
        let note = match (&r.value, &before) {
            (None, None) => Some("field not present; nothing to remove".to_string()),
            (Some(v), Some(b)) if v == b => Some("value already set".to_string()),
            _ => None,
        };
        changes.push(Change {
            key: r.key.clone(),
            action: if r.value.is_some() { "set" } else { "remove" },
            before,
            after: r.value.clone(),
            note,
        });
    }
    if let Some(p) = req.profile {
        for key in profile_targets(&fmt, p, &report, use_native) {
            if changes.iter().any(|c| c.key == key) {
                continue;
            }
            let before = field_value(&report, &key).map(|f| f.raw.clone());
            changes.push(Change {
                key,
                action: "remove",
                before,
                after: None,
                note: Some(format!("profile {p:?}")),
            });
        }
        if changes.is_empty() {
            warnings.push("the selected profile matches no removable metadata in this file".into());
        }
    }
    let data = std::fs::read(src).map_err(|e| MetaError::io(src, e));
    let signed = data.as_ref().is_ok_and(|d| native::has_signature(&fmt, d));
    if signed {
        warnings.push("this file appears to be digitally signed; any modification will invalidate the signature".into());
    }
    if fmt == "pdf" && backend == "native" {
        warnings.push(
            "the PDF is rewritten as a whole: earlier incremental revisions are not preserved"
                .into(),
        );
    }
    let removal = changes.iter().any(|c| c.action == "remove") || req.profile.is_some();
    let residual_note = removal.then(|| "Removing fields does not guarantee every trace is gone: embedded thumbnails or previews, duplicate tags in other metadata blocks, application-specific structures, document revision history, and unsupported formats' metadata may remain.".to_string());
    report.sha256 = report.sha256.or_else(|| sha256_file(src).ok());
    Ok(Plan {
        source: src.display().to_string(),
        format: fmt,
        backend: backend.into(),
        changes,
        warnings,
        signature_risk: signed,
        residual_note,
        resolved,
        profile: req.profile,
        before: Some(report),
    })
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

fn values_match(expected: &str, actual: &str) -> bool {
    if expected == actual {
        return true;
    }
    match (time::parse_any(expected), time::parse_any(actual)) {
        (Some(a), Some(b)) => a.sort_key() == b.sort_key(),
        _ => false,
    }
}

fn backup_name(src: &Path) -> PathBuf {
    let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    src.with_file_name(format!("{name}.metadata-backup-{ts}"))
}

pub fn execute(
    engine: &Engine,
    src: &Path,
    dest: &Path,
    plan: &Plan,
    opts: &ApplyOpts,
) -> Result<EditOutcome> {
    let in_place = dest.exists() && same_file(src, dest);
    if plan.signature_risk && !opts.allow_signature_loss {
        return Err(MetaError::InvalidArgument("file is digitally signed; editing invalidates the signature. Re-run with --accept-signature-loss to proceed".into()));
    }
    if in_place && !opts.replace_original {
        return Err(MetaError::InvalidArgument(
            "refusing to modify the original; choose a different --output or pass --in-place"
                .into(),
        ));
    }
    if !in_place && dest.exists() && !opts.force {
        return Err(MetaError::InvalidArgument(format!(
            "{} already exists (use --force to replace)",
            dest.display()
        )));
    }
    let dir = dest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let before = plan
        .before
        .as_ref()
        .ok_or_else(|| MetaError::Other("plan is missing its baseline".into()))?;
    let source_hash = before.sha256.clone().unwrap_or_default();

    // 1. produce the new file in a private temp file next to the destination
    let mut tmp = tempfile::Builder::new()
        .prefix(".metadata-edit-")
        .tempfile_in(dir)
        .map_err(|e| MetaError::io(dir, e))?;
    let wprofile: Option<WProfile> = plan.profile.map(Into::into);
    match plan.backend.as_str() {
        "native" => {
            let meta = std::fs::metadata(src).map_err(|e| MetaError::io(src, e))?;
            if meta.len() > engine.opts.limits.max_structural_bytes {
                return Err(MetaError::LimitExceeded(format!(
                    "file larger than {} bytes; too large for native editing",
                    engine.opts.limits.max_structural_bytes
                )));
            }
            let data = std::fs::read(src).map_err(|e| MetaError::io(src, e))?;
            let out = native::write(&plan.format, &data, &plan.resolved, wprofile)?;
            tmp.write_all(&out)
                .map_err(|e| MetaError::io(tmp.path(), e))?;
        }
        "exiftool" => {
            let (exe, _) =
                engine
                    .backends
                    .exiftool
                    .as_ref()
                    .ok_or_else(|| MetaError::BackendUnavailable {
                        backend: "exiftool".into(),
                        reason: "not installed".into(),
                    })?;
            // exiftool refuses to overwrite its -o target, so give it a fresh path in a private directory.
            let scratch = tempfile::tempdir_in(dir).map_err(|e| MetaError::io(dir, e))?;
            let target = scratch.path().join(dest.file_name().unwrap_or_default());
            let args = exiftool::build_args(&plan.resolved, wprofile, src, &target)?;
            exiftool::run(exe, &args, opts.timeout)?;
            let bytes = std::fs::read(&target).map_err(|e| MetaError::io(&target, e))?;
            tmp.write_all(&bytes)
                .map_err(|e| MetaError::io(tmp.path(), e))?;
        }
        other => return Err(MetaError::Other(format!("unknown backend {other}"))),
    }
    tmp.flush().ok();
    if let Ok(m) = std::fs::metadata(src) {
        let _ = std::fs::set_permissions(tmp.path(), m.permissions());
    }

    // 2. re-extract the temp output and verify before anything is replaced
    let probe_path = tmp
        .path()
        .with_extension(src.extension().unwrap_or_default());
    let probe = tempfile::Builder::new()
        .prefix(".metadata-verify-")
        .suffix(&format!(
            ".{}",
            src.extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default()
        ))
        .tempfile_in(dir)
        .map_err(|e| MetaError::io(dir, e))?;
    std::fs::copy(tmp.path(), probe.path()).map_err(|e| MetaError::io(probe.path(), e))?;
    let _ = probe_path;
    let after = engine.inspect(probe.path());
    drop(probe);
    let mut checks = vec![];
    if let Some(e) = &after.error {
        checks.push(Check {
            key: "(file)".into(),
            ok: false,
            detail: format!("result unreadable: {e}"),
        });
    }
    checks.push(Check {
        key: "(format)".into(),
        ok: after.format.id == plan.format,
        detail: format!("{} → {}", plan.format, after.format.id),
    });
    for c in &plan.changes {
        let now = field_value(&after, &c.key);
        let (ok, detail) = match (c.action, &c.after, now) {
            ("set", Some(v), Some(f)) => (
                values_match(v, &f.raw),
                format!("reads back as {:?}", f.raw),
            ),
            ("set", Some(_), None) => (false, "field missing after write".into()),
            ("remove", _, None) => (true, "absent".into()),
            ("remove", _, Some(f)) => {
                // a backend may legitimately keep an unrelated duplicate; only flag if the old value survived
                (
                    c.before.as_deref() != Some(f.raw.as_str()),
                    format!("still present as {:?}", f.raw),
                )
            }
            _ => (false, "unexpected state".into()),
        };
        checks.push(Check {
            key: c.key.clone(),
            ok,
            detail,
        });
    }
    let verified = checks.iter().all(|c| c.ok);
    let touched: Vec<&str> = plan.changes.iter().map(|c| c.key.as_str()).collect();
    let mut unexpected = vec![];
    for f in &before.fields {
        if matches!(f.group.as_str(), "Archive" | "Document")
            || touched.iter().any(|t| t.eq_ignore_ascii_case(&f.key()))
            || f.name == "Revisions"
        {
            continue;
        }
        let was_profile_removed =
            plan.profile.is_some() && !after.fields.iter().any(|a| a.key() == f.key());
        match field_value(&after, &f.key()) {
            Some(a) if a.raw == f.raw => {}
            Some(a) => unexpected.push(format!("{} changed: {:?} → {:?}", f.key(), f.raw, a.raw)),
            None if !was_profile_removed => unexpected.push(format!("{} disappeared", f.key())),
            None => {}
        }
    }
    if !verified {
        return Err(MetaError::Other(format!(
            "verification failed; original left untouched. {}",
            checks
                .iter()
                .filter(|c| !c.ok)
                .map(|c| format!("{}: {}", c.key, c.detail))
                .collect::<Vec<_>>()
                .join("; ")
        )));
    }

    // 3. back up, then atomically replace
    let backup = if in_place && opts.backup == Backup::Copy {
        let b = backup_name(src);
        if b.exists() {
            return Err(MetaError::InvalidArgument(format!(
                "backup {} already exists",
                b.display()
            )));
        }
        std::fs::copy(src, &b).map_err(|e| MetaError::io(&b, e))?;
        Some(b)
    } else {
        None
    };
    tmp.persist(dest)
        .map_err(|e| MetaError::io(dest, e.error))?;
    let dest_hash = sha256_file(dest)?;
    let outcome = EditOutcome {
        source: src.display().to_string(),
        destination: dest.display().to_string(),
        backup: backup.map(|b| b.display().to_string()),
        backend: plan.backend.clone(),
        source_sha256: source_hash,
        destination_sha256: dest_hash,
        verified,
        checks,
        unexpected_changes: unexpected,
        residual_note: plan.residual_note.clone(),
    };
    if let Some(lp) = &opts.log_path {
        log_operation(lp, plan, &outcome);
    }
    Ok(outcome)
}

/// Append one JSON line linking the edited file to its original by hash. Failures are non-fatal (warned by caller via return).
fn log_operation(path: &Path, plan: &Plan, o: &EditOutcome) {
    let line = serde_json::json!({
        "at": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "operation": "edit",
        "source": o.source, "source_sha256": o.source_sha256,
        "destination": o.destination, "destination_sha256": o.destination_sha256,
        "backup": o.backup, "backend": o.backend,
        "changes": plan.changes.iter().map(|c| serde_json::json!({"key": c.key, "action": c.action})).collect::<Vec<_>>(),
        "verified": o.verified,
    });
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{line}");
    }
}
