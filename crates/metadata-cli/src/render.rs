//! Output renderers. JSON/CSV are strictly machine-readable; human output may use color.
use metadata_core::{ExtensionMatch, Field, FileReport, SCHEMA_VERSION};
use metadata_engine::explain::explain;
use serde_json::json;
use std::fmt::Write;

#[derive(Clone, Copy)]
pub struct Style(pub bool);

impl Style {
    fn w(self, code: &str, s: &str) -> String { if self.0 { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() } }
    pub fn bold(self, s: &str) -> String { self.w("1", s) }
    pub fn dim(self, s: &str) -> String { self.w("2", s) }
    pub fn warn(self, s: &str) -> String { self.w("33", s) }
    pub fn err(self, s: &str) -> String { self.w("31", s) }
    pub fn ok(self, s: &str) -> String { self.w("32", s) }
    pub fn accent(self, s: &str) -> String { self.w("36", s) }
}

pub const REDACTED: &str = "[redacted]";

pub fn redact(r: &mut FileReport) {
    for f in r.fields.iter_mut().filter(|f| f.sensitive) {
        f.raw = REDACTED.into();
        f.normalized = None;
        f.time = None;
    }
}

pub fn has_sensitive(reports: &[FileReport]) -> bool {
    reports.iter().any(|r| r.fields.iter().any(|f| f.sensitive && f.raw != REDACTED))
}

pub fn tool() -> String { format!("metadata {}", env!("CARGO_PKG_VERSION")) }

pub fn json_envelope(reports: &[FileReport], skipped: &[(String, String)]) -> String {
    let v = json!({
        "schema_version": SCHEMA_VERSION,
        "tool": tool(),
        "files": reports,
        "skipped": skipped.iter().map(|(p, r)| json!({"path": p, "reason": r})).collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&v).expect("json") + "\n"
}

fn csv_cell(s: &str) -> String {
    // Neutralize spreadsheet formula injection from untrusted metadata, then quote.
    let guarded = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{s}") } else { s.to_string() };
    if guarded.contains([',', '"', '\n', '\r']) { format!("\"{}\"", guarded.replace('"', "\"\"")) } else { guarded }
}

pub fn csv_fields(reports: &[FileReport]) -> String {
    let mut o = String::from("path,group,name,value,time_utc,offset_known,backend,provenance,sensitive\n");
    for r in reports {
        for f in &r.fields {
            let (utc, known) = f.time.as_ref().map(|t| (t.utc.clone().unwrap_or_default(), t.offset_known.to_string())).unwrap_or_default();
            let prov = serde_json::to_value(f.provenance).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
            let _ = writeln!(o, "{},{},{},{},{},{},{},{},{}", csv_cell(&r.path), csv_cell(&f.group), csv_cell(&f.name), csv_cell(&f.raw), csv_cell(&utc), known, csv_cell(&f.backend), prov, f.sensitive);
        }
    }
    o
}

pub fn human_size(n: u64) -> String {
    const U: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let (mut v, mut i) = (n as f64, 0);
    while v >= 1024.0 && i < 4 { v /= 1024.0; i += 1; }
    if i == 0 { format!("{n} B") } else { format!("{v:.1} {}", U[i]) }
}

fn matches_filter(f: &Field, filt: &Option<String>) -> bool {
    filt.as_ref().is_none_or(|t| {
        let t = t.to_ascii_lowercase();
        f.group.to_ascii_lowercase().contains(&t) || f.name.to_ascii_lowercase().contains(&t) || f.raw.to_ascii_lowercase().contains(&t)
    })
}

fn clip(s: &str, max: usize) -> String {
    let one: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    if one.chars().count() > max { format!("{}…", one.chars().take(max - 1).collect::<String>()) } else { one }
}

pub fn human_report(r: &FileReport, st: Style, filter: &Option<String>, explain_fields: bool) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "{}", st.bold(&r.path));
    if let Some(e) = &r.error {
        let _ = writeln!(o, "  {} {e}\n", st.err("error"));
        return o;
    }
    let ext = match r.format.extension_match {
        ExtensionMatch::Match => st.dim("extension matches"),
        ExtensionMatch::Mismatch => st.warn("extension does NOT match content"),
        ExtensionMatch::Unknown => st.dim("extension n/a"),
    };
    let row = |o: &mut String, k: &str, v: String| { let _ = writeln!(o, "  {:<12}{v}", st.dim(k)); };
    row(&mut o, "Format", format!("{} · {} · {ext}", r.format.description, r.format.mime));
    row(&mut o, "Size", format!("{} ({} bytes)", human_size(r.size), r.size));
    if let Some(h) = &r.sha256 { row(&mut o, "SHA-256", h.clone()); }
    row(&mut o, "Backends", r.backends.iter().map(|b| format!("{} {}", b.name, b.version)).collect::<Vec<_>>().join(", "));
    let fs = &r.filesystem;
    let _ = writeln!(o, "\n  {}  {}", st.accent("Filesystem"), st.dim("(changes on copy/download/extract; weak evidence alone)"));
    for (k, t) in [("Created", &fs.created), ("Modified", &fs.modified), ("Accessed", &fs.accessed)] {
        if let Some(t) = t { let _ = writeln!(o, "    {:<12}{}", st.dim(k), t.iso); }
    }
    if let Some(p) = &fs.permissions { let _ = writeln!(o, "    {:<12}{p}", st.dim("Permissions")); }

    let mut groups: Vec<(&str, Vec<&Field>)> = vec![];
    for f in r.fields.iter().filter(|f| matches_filter(f, filter)) {
        match groups.iter_mut().find(|(g, _)| *g == f.group) {
            Some((_, v)) => v.push(f),
            None => groups.push((&f.group, vec![f])),
        }
    }
    if groups.is_empty() && r.error.is_none() {
        let _ = writeln!(o, "\n  {}", st.dim(if filter.is_some() { "No fields match the filter." } else { "No embedded metadata fields found." }));
    }
    for (g, fields) in groups {
        let _ = writeln!(o, "\n  {} {}", st.accent(g), st.dim(&format!("({})", fields.len())));
        let w = fields.iter().map(|f| f.name.chars().count()).max().unwrap_or(8).clamp(8, 30);
        for f in fields {
            let mut v = clip(&f.raw, 100);
            if f.time.as_ref().is_some_and(|t| !t.offset_known) { v.push_str(&st.dim("  (no timezone in source)")); }
            let name = clip(&f.name, w);
            let _ = writeln!(o, "    {name:<w$}  {v}");
            if explain_fields {
                if let Some(x) = explain(&f.group, &f.name) { let _ = writeln!(o, "    {:<w$}  {}", "", st.dim(&format!("↳ {x}"))); }
            }
        }
    }
    if !r.warnings.is_empty() {
        let _ = writeln!(o, "\n  {}", st.warn("Warnings"));
        for w in &r.warnings { let _ = writeln!(o, "    ! [{}] {}", w.backend, w.message); }
    }
    if !r.unavailable.is_empty() {
        let _ = writeln!(o, "\n  {}", st.dim("Not available"));
        for u in &r.unavailable { let _ = writeln!(o, "    – {u}"); }
    }
    o.push('\n');
    o
}
