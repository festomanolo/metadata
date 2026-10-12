//! Archive inspection (ZIP, TAR, GZIP). Lists only; never extracts to disk or executes anything.
use crate::{Extraction, Limits, Options};
use metadata_core::{time, Field, Provenance, TimeValue};
use std::io::{Cursor, Read, Seek};
use std::path::Path;

const B: &str = "native";
const MAX_SHOWN: usize = 500;

pub fn extract(path: &Path, id: &str, opts: &Options, ex: &mut Extraction, container_only: bool) {
    match id {
        "zip" => match std::fs::File::open(path) {
            Ok(f) => zip_list(f, "", opts, ex, container_only, 0),
            Err(e) => ex.warn(B, format!("cannot open: {e}")),
        },
        "tar" => match std::fs::File::open(path) {
            Ok(f) => tar_list(f, "", opts, ex, 0),
            Err(e) => ex.warn(B, format!("cannot open: {e}")),
        },
        "gzip" => gzip(path, opts, ex),
        _ => {}
    }
}

fn zip_time(d: Option<zip::DateTime>) -> Option<TimeValue> {
    let d = d?;
    time::parse_iso(&format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        d.year(),
        d.month(),
        d.day(),
        d.hour(),
        d.minute(),
        d.second()
    ))
}

fn f(name: &str, raw: impl Into<String>, prov: Provenance) -> Field {
    Field::new("Archive", name, raw, B, prov)
}

pub fn zip_list<R: Read + Seek>(
    r: R,
    prefix: &str,
    opts: &Options,
    ex: &mut Extraction,
    container_only: bool,
    depth: usize,
) {
    let lim: &Limits = &opts.limits;
    let mut z = match zip::ZipArchive::new(r) {
        Ok(z) => z,
        Err(e) => return ex.warn(B, format!("{prefix}not a readable ZIP: {e}")),
    };
    if prefix.is_empty() {
        let c = String::from_utf8_lossy(z.comment()).trim().to_string();
        if !c.is_empty() {
            ex.fields.push(f("Comment", c, Provenance::Stored));
        }
    }
    let n = z.len();
    ex.fields.push(f(
        &format!("{prefix}EntryCount"),
        n.to_string(),
        Provenance::Inferred,
    ));
    if n > lim.max_archive_entries {
        ex.warn(
            B,
            format!(
                "{prefix}archive has {n} entries; only the first {} inspected",
                lim.max_archive_entries
            ),
        );
    }
    let (mut total, mut packed, mut shown) = (0u64, 0u64, 0usize);
    let (mut earliest, mut latest): (Option<TimeValue>, Option<TimeValue>) = (None, None);
    let (mut unsafe_paths, mut symlinks, mut encrypted, mut bombs) = (0, 0, 0, 0);
    let mut nested: Vec<(String, Vec<u8>)> = vec![];
    for i in 0..n.min(lim.max_archive_entries) {
        let e = match z.by_index_raw(i) {
            Ok(e) => e,
            Err(err) => {
                ex.warn(B, format!("{prefix}entry {i} unreadable: {err}"));
                continue;
            }
        };
        let name = e.name().to_string();
        let (size, csize) = (e.size(), e.compressed_size());
        total = total.saturating_add(size);
        packed = packed.saturating_add(csize);
        if total > lim.max_total_uncompressed {
            ex.warn(
                B,
                format!(
                    "{prefix}declared uncompressed size exceeds limit ({} bytes); listing stopped",
                    lim.max_total_uncompressed
                ),
            );
            break;
        }
        if e.enclosed_name().is_none() {
            unsafe_paths += 1;
            ex.warn(
                B,
                format!("{prefix}unsafe entry path (traversal/absolute): {name:?}"),
            );
        }
        if e.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            symlinks += 1;
        }
        if e.encrypted() {
            encrypted += 1;
        }
        if size > (1 << 20) && csize > 0 && size / csize > lim.max_ratio {
            bombs += 1;
            ex.warn(B, format!("{prefix}entry {name:?} has compression ratio {}:1 (possible decompression bomb); not expanded", size / csize));
        }
        let t = zip_time(e.last_modified());
        if let Some(tv) = &t {
            if earliest
                .as_ref()
                .is_none_or(|x| tv.sort_key() < x.sort_key())
            {
                earliest = Some(tv.clone());
            }
            if latest.as_ref().is_none_or(|x| tv.sort_key() > x.sort_key()) {
                latest = Some(tv.clone());
            }
        }
        if !container_only && !e.is_dir() && (shown < MAX_SHOWN || opts.all) {
            shown += 1;
            let mut fld =
                f("Entry", format!("{prefix}{name}"), Provenance::Stored).with_time(t.clone());
            fld.normalized = Some(
                serde_json::json!({"path": format!("{prefix}{name}"), "size": size, "compressed": csize, "method": format!("{:?}", e.compression())}),
            );
            ex.fields.push(fld);
        }
        // Nested archives (deep mode only), strictly bounded.
        if opts.deep
            && !container_only
            && depth < lim.max_depth
            && !e.is_dir()
            && size <= lim.max_entry_bytes
            && e.enclosed_name().is_some()
        {
            let low = name.to_ascii_lowercase();
            if [
                ".zip", ".jar", ".war", ".apk", ".docx", ".xlsx", ".pptx", ".odt", ".epub",
            ]
            .iter()
            .any(|x| low.ends_with(x))
            {
                let mut buf = Vec::new();
                // by_index_raw gives compressed bytes; re-open decompressed below
                drop(e);
                if let Ok(en) = z.by_index(i) {
                    if en.take(lim.max_entry_bytes).read_to_end(&mut buf).is_ok()
                        && buf.starts_with(b"PK")
                    {
                        nested.push((name, buf));
                    }
                }
                continue;
            }
        }
    }
    if !container_only && shown >= MAX_SHOWN && !opts.all && n > shown {
        ex.warn(
            B,
            format!("{prefix}only {MAX_SHOWN} entries shown; use --all to list every entry"),
        );
    }
    ex.fields.push(f(
        &format!("{prefix}TotalUncompressedSize"),
        total.to_string(),
        Provenance::Inferred,
    ));
    ex.fields.push(f(
        &format!("{prefix}TotalCompressedSize"),
        packed.to_string(),
        Provenance::Inferred,
    ));
    if let Some(t) = earliest {
        ex.fields.push(
            f(
                &format!("{prefix}EarliestEntryTime"),
                t.iso.clone(),
                Provenance::Stored,
            )
            .with_time(Some(t)),
        );
    }
    if let Some(t) = latest {
        ex.fields.push(
            f(
                &format!("{prefix}LatestEntryTime"),
                t.iso.clone(),
                Provenance::Stored,
            )
            .with_time(Some(t)),
        );
    }
    for (k, v) in [
        ("UnsafePathCount", unsafe_paths),
        ("SymlinkEntryCount", symlinks),
        ("EncryptedEntryCount", encrypted),
        ("HighRatioEntryCount", bombs),
    ] {
        if v > 0 {
            ex.fields.push(f(
                &format!("{prefix}{k}"),
                v.to_string(),
                Provenance::Inferred,
            ));
        }
    }
    for (name, buf) in nested {
        zip_list(
            Cursor::new(buf),
            &format!("{prefix}{name}!/"),
            opts,
            ex,
            false,
            depth + 1,
        );
    }
}

fn tar_list<R: Read>(r: R, prefix: &str, opts: &Options, ex: &mut Extraction, _depth: usize) {
    let lim = &opts.limits;
    let mut a = tar::Archive::new(r.take(lim.max_total_uncompressed));
    let entries = match a.entries() {
        Ok(e) => e,
        Err(e) => return ex.warn(B, format!("TAR unreadable: {e}")),
    };
    let (mut n, mut total, mut shown) = (0usize, 0u64, 0usize);
    for e in entries {
        let Ok(e) = e else {
            ex.warn(B, "corrupt TAR entry; listing stopped");
            break;
        };
        n += 1;
        if n > lim.max_archive_entries {
            ex.warn(B, "entry limit reached; listing truncated");
            break;
        }
        let size = e.size();
        total = total.saturating_add(size);
        let name = e
            .path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "<invalid path>".into());
        let ty = e.header().entry_type();
        let unsafe_path = e
            .path()
            .map(|p| {
                p.is_absolute()
                    || p.components()
                        .any(|c| matches!(c, std::path::Component::ParentDir))
            })
            .unwrap_or(true);
        if unsafe_path {
            ex.warn(
                B,
                format!("unsafe entry path (traversal/absolute): {name:?}"),
            );
        }
        if ty.is_symlink() || ty.is_hard_link() {
            ex.warn(B, format!("link entry {name:?} (not followed)"));
        }
        if ty.is_file() && (shown < MAX_SHOWN || opts.all) {
            shown += 1;
            let t = e.header().mtime().ok().filter(|&m| m > 0).map(|m| {
                TimeValue::from_system(std::time::UNIX_EPOCH + std::time::Duration::from_secs(m))
            });
            let mut fld = f("Entry", format!("{prefix}{name}"), Provenance::Stored).with_time(t);
            fld.normalized = Some(
                serde_json::json!({"path": name, "size": size, "mode": e.header().mode().ok().map(|m| format!("{m:04o}"))}),
            );
            ex.fields.push(fld);
        }
    }
    ex.fields
        .push(f("EntryCount", n.to_string(), Provenance::Inferred));
    ex.fields.push(f(
        "TotalUncompressedSize",
        total.to_string(),
        Provenance::Inferred,
    ));
}

fn gzip(path: &Path, opts: &Options, ex: &mut Extraction) {
    let Ok(fh) = std::fs::File::open(path) else {
        return;
    };
    let mut dec = flate2::read::GzDecoder::new(fh);
    let mut head = [0u8; 512];
    let mut got = 0;
    while got < head.len() {
        match dec.read(&mut head[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) => return ex.warn(B, format!("GZIP data corrupt: {e}")),
        }
    }
    if let Some(h) = dec.header() {
        if let Some(n) = h.filename() {
            ex.fields.push(f(
                "OriginalName",
                String::from_utf8_lossy(n).to_string(),
                Provenance::Stored,
            ));
        }
        if h.mtime() > 0 {
            let t = TimeValue::from_system(
                std::time::UNIX_EPOCH + std::time::Duration::from_secs(h.mtime() as u64),
            );
            ex.fields
                .push(f("ModifyTime", t.iso.clone(), Provenance::Stored).with_time(Some(t)));
        }
    }
    if got >= 262 && &head[257..262] == b"ustar" {
        ex.fields.push(f("Contains", "tar", Provenance::Inferred));
        if let Ok(fh) = std::fs::File::open(path) {
            tar_list(flate2::read::GzDecoder::new(fh), "", opts, ex, 0);
        }
    }
}
