//! Report rendering: self-contained printable HTML (no scripts, no external resources) and CSV.
//! Every dynamic string is HTML-escaped: metadata inside submitted files is untrusted.
use metadata_forensics::assess::{Assessment, Finding, FindingState, FindingStatus, Severity};
use metadata_forensics::timeline::Event;
use std::collections::BTreeMap;
use std::fmt::Write;

pub struct ReportInput<'a> {
    pub assessment: &'a Assessment,
    pub events: &'a [Event],
    pub states: &'a BTreeMap<String, FindingState>,
    /// (timestamp, text) reviewer notes.
    pub notes: &'a [(String, String)],
    pub title: Option<&'a str>,
    /// Include per-file embedded timestamps from the timeline (can reveal activity patterns); default off.
    pub include_timeline: bool,
}

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c if c.is_control() && c != '\n' && c != '\t' => o.push(' '),
            c => o.push(c),
        }
    }
    o
}

fn sev_class(s: Severity) -> &'static str {
    match s {
        Severity::HighPriority => "high",
        Severity::Review => "review",
        Severity::Info => "info",
    }
}
fn sev_label(s: Severity) -> &'static str {
    match s {
        Severity::HighPriority => "HIGH PRIORITY",
        Severity::Review => "REVIEW",
        Severity::Info => "INFO",
    }
}

const CSS: &str = r#":root{--bg:#fff;--fg:#1c2330;--dim:#5b6475;--line:#d9dde5;--card:#f7f8fa;--accent:#0b6f8a;--high:#b3261e;--review:#8a5a00;--info:#3d5a80}
@media (prefers-color-scheme:dark){:root{--bg:#12161d;--fg:#e4e8ef;--dim:#9aa3b2;--line:#2c3442;--card:#1a2029;--accent:#6fd0e4;--high:#ff8a80;--review:#f0c060;--info:#9ec0e8}}
*{box-sizing:border-box}body{margin:0 auto;max-width:960px;padding:24px 16px 64px;background:var(--bg);color:var(--fg);font:15px/1.5 -apple-system,Segoe UI,Roboto,Helvetica,Arial,sans-serif}
h1{font-size:1.6rem;margin:0 0 4px}h2{font-size:1.15rem;margin:32px 0 10px;padding-bottom:6px;border-bottom:1px solid var(--line)}h3{font-size:1rem;margin:0}
.dim{color:var(--dim)}.mono{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:.86em}
.notice{border:1px solid var(--line);border-left:4px solid var(--accent);background:var(--card);padding:12px 14px;border-radius:6px;margin:16px 0}
.stats{display:flex;flex-wrap:wrap;gap:10px;margin:14px 0}.stat{flex:1 1 140px;border:1px solid var(--line);background:var(--card);border-radius:6px;padding:10px 12px}.stat b{display:block;font-size:1.5rem}
.badge{display:inline-block;padding:1px 8px;border-radius:999px;font-size:.75rem;font-weight:600;border:1px solid currentColor;margin-right:6px}
.high{color:var(--high)}.review{color:var(--review)}.info{color:var(--info)}
.finding{border:1px solid var(--line);background:var(--card);border-radius:8px;padding:12px 14px;margin:12px 0;break-inside:avoid}
.finding.dismissed{opacity:.7}dl{display:grid;grid-template-columns:150px 1fr;gap:4px 12px;margin:10px 0 0}dt{color:var(--dim)}dd{margin:0;overflow-wrap:anywhere}
table{border-collapse:collapse;width:100%;font-size:.85rem}th,td{text-align:left;padding:5px 8px;border-bottom:1px solid var(--line);vertical-align:top;overflow-wrap:anywhere}th{color:var(--dim);font-weight:600}
ul{margin:4px 0 0 18px;padding:0}@media print{body{max-width:none;padding:0}.finding{page-break-inside:avoid}h2{page-break-after:avoid}a{color:inherit}}
@media (max-width:560px){dl{grid-template-columns:1fr}}"#;

fn finding_html(o: &mut String, f: &Finding, st: Option<&FindingState>) {
    let status = st.map(|s| s.status).unwrap_or(FindingStatus::Open);
    let cls = if status == FindingStatus::Dismissed {
        " dismissed"
    } else {
        ""
    };
    let _ = write!(o, "<section class=\"finding{cls}\" id=\"{id}\"><h3><span class=\"badge {sc}\">{sl}</span><span class=\"mono\">{id}</span> {title}</h3>", id = esc(&f.id), sc = sev_class(f.severity), sl = sev_label(f.severity), title = esc(&f.title));
    let _ = write!(
        o,
        "<p class=\"dim\">Review priority {}/5 · status: <b>{}</b></p><dl>",
        f.review_priority,
        status.as_str()
    );
    let row = |o: &mut String, k: &str, v: &str| {
        let _ = write!(o, "<dt>{}</dt><dd>{}</dd>", esc(k), v);
    };
    row(o, "Why it was flagged", &esc(&f.explanation));
    row(
        o,
        "Comparison rule",
        &format!("<span class=\"mono\">{}</span>", esc(&f.comparison_rule)),
    );
    let mut ev =
        String::from("<table><tr><th>File</th><th>Field</th><th>Value</th><th>Source</th></tr>");
    for e in &f.evidence {
        let _ = write!(
            ev,
            "<tr><td class=\"mono\">{}</td><td class=\"mono\">{}</td><td>{}</td><td>{}</td></tr>",
            esc(&e.file),
            esc(&e.field),
            esc(&e.value),
            e.source.label()
        );
    }
    ev.push_str("</table>");
    if f.evidence_omitted > 0 {
        let _ = write!(
            ev,
            "<p class=\"dim\">… {} more evidence item(s) in the JSON export.</p>",
            f.evidence_omitted
        );
    }
    row(o, "Evidence", &ev);
    let benign: String = f
        .benign_explanations
        .iter()
        .map(|b| format!("<li>{}</li>", esc(b)))
        .collect();
    row(o, "Could also be", &format!("<ul>{benign}</ul>"));
    row(
        o,
        "Confidence",
        &format!(
            "observation <b>{:?}</b> · interpretation <b>{:?}</b>",
            f.observation_confidence, f.interpretation_confidence
        ),
    );
    row(o, "Suggested follow-up", &esc(&f.follow_up));
    if let Some(s) = st {
        if let Some(r) = &s.reason {
            row(o, "Dismissal reason", &esc(r));
        }
        if let Some(n) = &s.note {
            row(o, "Reviewer note", &esc(n));
        }
        row(o, "Updated", &esc(&s.updated_at));
    }
    o.push_str("</dl></section>");
}

pub fn html(i: &ReportInput) -> String {
    let a = i.assessment;
    let title = i
        .title
        .map(str::to_string)
        .unwrap_or_else(|| format!("Metadata review — case {}", a.case_id));
    let mut o = String::new();
    let _ = write!(o, "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><style>{CSS}</style></head><body>", esc(&title));
    let _ = write!(
        o,
        "<h1>{}</h1><p class=\"dim\">Generated by {} on {} · schema {}</p>",
        esc(&title),
        esc(&a.tool),
        esc(&a.assessed_at.iso),
        esc(&a.schema)
    );
    let _ = write!(
        o,
        "<div class=\"notice\"><b>How to read this report.</b> {}</div>",
        esc(&a.disclaimer)
    );

    let open = a
        .findings
        .iter()
        .filter(|f| i.states.get(&f.id).map(|s| s.status) != Some(FindingStatus::Dismissed))
        .count();
    let count = |s: Severity| a.findings.iter().filter(|f| f.severity == s).count();
    o.push_str("<h2>Executive summary</h2>");
    let _ = write!(o, "<div class=\"stats\"><div class=\"stat\"><b>{}</b>files examined</div><div class=\"stat\"><b class=\"high\">{}</b>high priority</div><div class=\"stat\"><b class=\"review\">{}</b>review</div><div class=\"stat\"><b class=\"info\">{}</b>info</div><div class=\"stat\"><b>{}</b>not dismissed</div></div>", a.inventory.len(), count(Severity::HighPriority), count(Severity::Review), count(Severity::Info), open);
    let cl = &a.claims;
    let _ = write!(o, "<p>Declared by the assessor: start <b>{}</b>, completion <b>{}</b>, deadline <b>{}</b>, {} milestone(s){}.</p>", esc(cl.start.as_deref().unwrap_or("not given")), esc(cl.completion.as_deref().unwrap_or("not given")), esc(cl.deadline.as_deref().unwrap_or("not given")), cl.milestones.len(), cl.student.as_ref().map(|s| format!(", student ID <b>{}</b>", esc(s))).unwrap_or_default());
    if let Some(n) = a.git_commits {
        let _ = write!(o, "<p>Git history was available: {n} commit(s). Commit dates are written by the committer's clock and can be rewritten.</p>");
    }
    if let Some(g) = &a.integrity {
        let _ = write!(
            o,
            "<p>Evidence integrity: {} ({} files re-hashed after analysis).</p>",
            if g.unchanged {
                "<b>inputs unchanged by this analysis</b>"
            } else {
                "<b class=\"high\">INPUTS CHANGED DURING ANALYSIS</b>"
            },
            g.files_rehashed
        );
    }
    if a.personal_data_redacted {
        o.push_str("<p class=\"dim\">Personal identifiers (author names, etc.) are masked in this report; location data is not included.</p>");
    }

    o.push_str("<h2>Findings</h2>");
    if a.findings.is_empty() {
        o.push_str("<p>No findings were produced by the available checks. This does not demonstrate that nothing noteworthy exists; see limitations.</p>");
    }
    for f in &a.findings {
        finding_html(&mut o, f, i.states.get(&f.id));
    }

    o.push_str("<h2>Evidence inventory</h2><table><tr><th>Path</th><th>Format</th><th>Size</th><th>SHA-256</th><th>FS modified</th><th>Notes</th></tr>");
    for f in &a.inventory {
        let notes = if let Some(e) = &f.error {
            format!("error: {e}")
        } else if f.extension_match == metadata_core::ExtensionMatch::Mismatch {
            "extension mismatch".into()
        } else {
            String::new()
        };
        let _ = write!(o, "<tr><td class=\"mono\">{}</td><td>{}</td><td>{}</td><td class=\"mono\">{}</td><td>{}</td><td>{}</td></tr>", esc(&f.path), esc(&f.format), f.size, esc(f.sha256.as_deref().map(|h| &h[..h.len().min(16)]).unwrap_or("")), esc(&f.fs_modified.as_ref().map(|t| t.iso.clone()).unwrap_or_default()), esc(&notes));
    }
    o.push_str("</table><p class=\"dim\">SHA-256 truncated to 16 hex characters here; the JSON export has full hashes. Hashes show content equality, not authorship.</p>");

    if !a.project.manifests.is_empty() || a.project.source_file_count > 0 {
        let langs: String = a
            .project
            .languages
            .iter()
            .map(|(k, v)| format!("{} ({v})", esc(k)))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = write!(o, "<h2>Project structure</h2><p>{} source file(s): {}. Manifests: {}. README present: {}. Git repository: {}.</p>", a.project.source_file_count, langs, esc(&a.project.manifests.join(", ")), a.project.has_readme, a.project.has_git);
    }

    if i.include_timeline && !i.events.is_empty() {
        o.push_str("<h2>Timeline</h2><p class=\"dim\">Sources are listed separately; none is independent proof. “?” = no timezone recorded.</p><table><tr><th>Time</th><th>Source</th><th>File</th><th>Field</th></tr>");
        for e in i.events.iter().take(500) {
            let _ = write!(o, "<tr><td class=\"mono\">{}{}</td><td>{}</td><td class=\"mono\">{}</td><td class=\"mono\">{}</td></tr>", esc(&e.time.iso), if e.time.offset_known { "" } else { " ?" }, e.source.label(), esc(&e.file), esc(&e.field));
        }
        o.push_str("</table>");
        if i.events.len() > 500 {
            let _ = write!(
                o,
                "<p class=\"dim\">{} further events omitted; use the JSON export.</p>",
                i.events.len() - 500
            );
        }
    }

    o.push_str("<h2>Limitations</h2><ul>");
    for l in &a.limitations {
        let _ = write!(o, "<li>{}</li>", esc(l));
    }
    for l in &a.checks_not_implemented {
        let _ = write!(o, "<li><b>Not performed:</b> {}</li>", esc(l));
    }
    o.push_str("</ul>");

    o.push_str("<h2>Reviewer notes</h2>");
    if i.notes.is_empty() {
        o.push_str("<p class=\"dim\">No notes recorded.</p>");
    } else {
        o.push_str("<ul>");
        for (at, t) in i.notes {
            let _ = write!(
                o,
                "<li><span class=\"dim\">{}</span> {}</li>",
                esc(at),
                esc(t)
            );
        }
        o.push_str("</ul>");
    }
    let _ = write!(o, "<h2>Reproducibility</h2><p class=\"dim\">Analysis configuration: tolerance {}h, uniform-timestamp minimum {} files, history gap {} days. Re-running with the same claims on the same files yields the same finding identifiers.</p>", a.thresholds.tolerance_hours, a.thresholds.uniform_min_files, a.thresholds.gap_days);
    o.push_str("</body></html>\n");
    o
}

fn csv(s: &str) -> String {
    let g = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{s}")
    } else {
        s.to_string()
    };
    if g.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", g.replace('"', "\"\""))
    } else {
        g
    }
}

pub fn findings_csv(a: &Assessment, states: &BTreeMap<String, FindingState>) -> String {
    let mut o = String::from("id,rule,severity,review_priority,status,files,title,explanation,follow_up,dismissal_reason,note\n");
    for f in &a.findings {
        let st = states.get(&f.id);
        let _ = writeln!(
            o,
            "{},{},{},{},{},{},{},{},{},{},{}",
            csv(&f.id),
            csv(&f.rule),
            sev_label(f.severity),
            f.review_priority,
            st.map(|s| s.status).unwrap_or(FindingStatus::Open).as_str(),
            csv(&f.files.join("; ")), csv(&f.title), csv(&f.explanation), csv(&f.follow_up),
            csv(st.and_then(|s| s.reason.as_deref()).unwrap_or("")), csv(st.and_then(|s| s.note.as_deref()).unwrap_or(""))
        );
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use metadata_forensics::assess::{assess, Input};
    use metadata_forensics::claims::{Claims, Thresholds};
    use metadata_forensics::project::ProjectSummary;

    fn sample(creator: &str) -> Assessment {
        let c = Claims { start: Some("2024-01-10".into()), ..Default::default() };
        let mut r = metadata_core::FileReport {
            schema_version: "1".into(), path: "/p/<img src=x onerror=alert(1)>.docx".into(), size: 3, sha256: Some("a".repeat(64)),
            format: metadata_core::FormatInfo { id: "docx".into(), description: String::new(), mime: String::new(), category: metadata_core::Category::Document, extension_match: metadata_core::ExtensionMatch::Match, signature_hex: String::new() },
            filesystem: Default::default(), fields: vec![], warnings: vec![], backends: vec![], unavailable: vec![], error: None,
        };
        let mut f = metadata_core::Field::new("OOXML-Core", "Created", "2020-01-01T00:00:00Z", "native", metadata_core::Provenance::Stored);
        f.time = metadata_core::time::parse_iso("2020-01-01T00:00:00Z");
        r.fields.push(f);
        r.fields.push(metadata_core::Field::new("OOXML-Core", "Creator", creator, "native", metadata_core::Provenance::Stored));
        let now = metadata_core::time::parse_iso("2024-06-01T00:00:00Z").unwrap();
        assess(&Input { case_id: "c<1>", root: std::path::Path::new("/p"), reports: &[r], claims: &c, thresholds: &Thresholds::default(), git: None, previous: None, project: ProjectSummary::default(), now })
    }

    #[test]
    fn html_escapes_untrusted_metadata_and_has_no_scripts() {
        let a = sample("<script>alert(1)</script>");
        let h = html(&ReportInput { assessment: &a, events: &[], states: &BTreeMap::new(), notes: &[("t".into(), "<b>note</b>".into())], title: None, include_timeline: false });
        assert!(!h.contains("<script"), "no script elements");
        assert!(!h.contains("<img src=x"), "file names are escaped");
        assert!(h.contains("&lt;img src=x onerror"));
        assert!(h.contains("&lt;b&gt;note&lt;/b&gt;"));
        assert!(h.contains("c&lt;1&gt;"));
        for needle in ["Executive summary", "Evidence inventory", "Limitations", "Could also be", "Reviewer notes", "does not establish academic misconduct"] {
            assert!(h.contains(needle), "{needle}");
        }
        assert!(!h.contains("http://") && !h.contains("https://"), "no external resources");
    }

    #[test]
    fn dismissed_findings_show_reason_and_csv_is_guarded() {
        let a = sample("=cmd|' /C calc'!A0");
        let id = a.findings[0].id.clone();
        let mut st = BTreeMap::new();
        st.insert(id.clone(), FindingState { status: FindingStatus::Dismissed, note: Some("n".into()), reason: Some("template, reused".into()), updated_at: "2024".into() });
        let h = html(&ReportInput { assessment: &a, events: &[], states: &st, notes: &[], title: Some("T"), include_timeline: false });
        assert!(h.contains("Dismissal reason") && h.contains("template, reused") && h.contains("finding dismissed"));
        let c = findings_csv(&a, &st);
        assert!(c.lines().next().unwrap().starts_with("id,rule"));
        assert!(c.contains("dismissed") && c.contains("\"template, reused\""));
        assert!(!c.lines().skip(1).any(|l| l.starts_with(['=', '+', '-', '@'])));
    }
}
