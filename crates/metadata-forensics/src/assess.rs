//! Transparent assessment engine. Produces *findings for human review*, never verdicts.
//!
//! Every finding states the rule that fired, the exact evidence, why it was flagged, benign
//! explanations, and separates confidence in the *observation* (is the data really there?) from
//! confidence in any *interpretation* (does it imply anything about the work?).
use crate::claims::{parse_claim, Claims, DayRange, Thresholds};
use crate::git::GitHistory;
use crate::project::ProjectSummary;
use crate::timeline::{self, rel, Event, Role, Source};
use metadata_core::hash::{sha256_bytes, sha256_file};
use metadata_core::{Category, ExtensionMatch, FileReport, TimeValue};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

pub const ASSESSMENT_SCHEMA: &str = "metadata.assessment/1";
pub const DISCLAIMER: &str = "This report lists observations in digital metadata that may merit a closer look. It does not establish \
academic misconduct and contains no verdict or probability of misconduct. Metadata can change through copying, downloading, \
exporting, synchronization, extraction or editing, and can be deliberately altered. Findings should be weighed with other evidence \
and the student should have an opportunity to respond.";

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    #[serde(rename = "INFO")]
    Info,
    #[serde(rename = "REVIEW")]
    Review,
    #[serde(rename = "HIGH PRIORITY")]
    HighPriority,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize)]
pub struct Evidence {
    pub file: String,
    pub field: String,
    pub value: String,
    pub source: Source,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    /// Stable across runs for the same rule + evidence (paths relative to the analysis root).
    pub id: String,
    pub rule: String,
    pub title: String,
    pub severity: Severity,
    /// 1 (low) – 5 (look first).
    pub review_priority: u8,
    pub files: Vec<String>,
    pub evidence: Vec<Evidence>,
    pub evidence_omitted: usize,
    pub comparison_rule: String,
    pub explanation: String,
    pub benign_explanations: Vec<String>,
    pub observation_confidence: Confidence,
    pub interpretation_confidence: Confidence,
    pub follow_up: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InventoryItem {
    pub path: String,
    pub size: u64,
    pub sha256: Option<String>,
    pub format: String,
    pub extension_match: ExtensionMatch,
    pub fs_created: Option<TimeValue>,
    pub fs_modified: Option<TimeValue>,
    pub fields: usize,
    pub warnings: Vec<String>,
    pub backends: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Integrity {
    pub files_rehashed: usize,
    pub unchanged: bool,
    pub changed: Vec<String>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Assessment {
    pub schema: String,
    pub tool: String,
    pub case_id: String,
    pub assessed_at: TimeValue,
    pub root: String,
    pub claims: Claims,
    pub thresholds: Thresholds,
    pub inventory: Vec<InventoryItem>,
    pub project: ProjectSummary,
    pub git_commits: Option<usize>,
    pub findings: Vec<Finding>,
    pub integrity: Option<Integrity>,
    pub checks_not_implemented: Vec<String>,
    pub limitations: Vec<String>,
    pub disclaimer: String,
}

pub struct Input<'a> {
    pub case_id: &'a str,
    pub root: &'a Path,
    pub reports: &'a [FileReport],
    pub claims: &'a Claims,
    pub thresholds: &'a Thresholds,
    pub git: Option<&'a GitHistory>,
    pub previous: Option<(&'a Path, &'a [FileReport])>,
    pub project: ProjectSummary,
    pub now: TimeValue,
}

struct T {
    secs: i64,
    exact: bool,
}
fn t(tv: &TimeValue) -> Option<T> {
    Some(T {
        secs: tv.sort_key()?,
        exact: tv.offset_known,
    })
}
fn tol(a_exact: bool, b_exact: bool, th: &Thresholds) -> i64 {
    if a_exact && b_exact {
        120
    } else {
        th.tolerance_hours * 3600
    }
}
fn conf_obs(a_exact: bool, b_exact: bool) -> Confidence {
    if a_exact && b_exact {
        Confidence::High
    } else {
        Confidence::Medium
    }
}
fn claim_exact(r: &DayRange) -> bool {
    r.start == r.end
}

const MAX_EVIDENCE: usize = 25;

struct Ctx<'a> {
    i: &'a Input<'a>,
    out: Vec<Finding>,
}

struct Draft {
    rule: &'static str,
    title: String,
    severity: Severity,
    priority: u8,
    evidence: Vec<Evidence>,
    comparison_rule: String,
    explanation: String,
    benign: Vec<&'static str>,
    obs: Confidence,
    interp: Confidence,
    follow_up: &'static str,
}

impl Ctx<'_> {
    fn push(&mut self, d: Draft) {
        let mut keys: Vec<String> = d
            .evidence
            .iter()
            .map(|e| format!("{}|{}|{}", e.file, e.field, e.value))
            .collect();
        keys.sort();
        let id = format!(
            "{}-{}",
            d.rule,
            &sha256_bytes(format!("{}\n{}", d.rule, keys.join("\n")).as_bytes())[..8]
        );
        let mut files: Vec<String> = d.evidence.iter().map(|e| e.file.clone()).collect();
        files.sort();
        files.dedup();
        let omitted = d.evidence.len().saturating_sub(MAX_EVIDENCE);
        let mut evidence = d.evidence;
        evidence.truncate(MAX_EVIDENCE);
        self.out.push(Finding {
            id,
            rule: d.rule.into(),
            title: d.title,
            severity: d.severity,
            review_priority: d.priority,
            files,
            evidence,
            evidence_omitted: omitted,
            comparison_rule: d.comparison_rule,
            explanation: d.explanation,
            benign_explanations: d.benign.into_iter().map(String::from).collect(),
            observation_confidence: d.obs,
            interpretation_confidence: d.interp,
            follow_up: d.follow_up.into(),
        });
    }
}

fn ev(e: &Event) -> Evidence {
    Evidence {
        file: e.file.clone(),
        field: e.field.clone(),
        value: e.value.clone(),
        source: e.source,
    }
}

fn cat_of<'a>(reports: &'a [FileReport], root: &Path, file: &str) -> Option<&'a FileReport> {
    reports.iter().find(|r| rel(root, &r.path) == file)
}

const STD_BENIGN_TIME: [&str; 3] = [
    "The timezone is not recorded, so the comparison allows a tolerance for local-time ambiguity.",
    "Files copied, converted, re-saved or created from a template can retain timestamps from earlier work.",
    "The creating device's clock may have been wrong.",
];

pub fn assess(input: &Input) -> Assessment {
    let mut c = Ctx {
        i: input,
        out: vec![],
    };
    let root = input.root;
    let events = timeline::file_events(root, input.reports);
    let embedded: Vec<&Event> = events
        .iter()
        .filter(|e| e.source == Source::Embedded)
        .collect();
    let claims = input.claims;
    let th = input.thresholds;
    let start = claims.start.as_deref().and_then(|s| parse_claim(s).ok());
    let deadline = claims.deadline.as_deref().and_then(|s| parse_claim(s).ok());
    let completion = claims
        .completion
        .as_deref()
        .and_then(|s| parse_claim(s).ok());
    let now = t(&input.now);

    rule_future(&mut c, &embedded, &now);
    if let Some(s) = &start {
        rule_before_start(&mut c, &embedded, s);
    }
    for (label, d) in [("deadline", &deadline), ("claimed completion", &completion)] {
        if let Some(d) = d {
            rule_after(&mut c, &embedded, d, label, &now);
        }
    }
    rule_modified_before_created(&mut c, &embedded);
    for m in &claims.milestones {
        if let Ok(r) = parse_claim(&m.date) {
            rule_milestone(&mut c, &embedded, &m.name, &r, m.path_contains.as_deref());
        }
    }
    rule_uniform(&mut c, &events);
    rule_ext_mismatch(&mut c);
    rule_duplicates(&mut c);
    rule_missing_metadata(&mut c);
    rule_incremental(&mut c, &start, &deadline, &completion);
    if let Some(g) = input.git {
        rule_git(&mut c, g, &start, &deadline, &now);
    }
    rule_gaps(&mut c, &embedded, &start, &deadline.or(completion));
    if let Some((proot, prev)) = input.previous {
        rule_previous(&mut c, proot, prev);
    }

    let mut findings = c.out;
    findings.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then(b.review_priority.cmp(&a.review_priority))
            .then(a.id.cmp(&b.id))
    });
    findings.dedup_by(|a, b| a.id == b.id);

    let inventory = input
        .reports
        .iter()
        .map(|r| InventoryItem {
            path: rel(root, &r.path),
            size: r.size,
            sha256: r.sha256.clone(),
            format: r.format.id.clone(),
            extension_match: r.format.extension_match,
            fs_created: r.filesystem.created.clone(),
            fs_modified: r.filesystem.modified.clone(),
            fields: r.fields.len(),
            warnings: r
                .warnings
                .iter()
                .map(|w| format!("[{}] {}", w.backend, w.message))
                .collect(),
            backends: r
                .backends
                .iter()
                .map(|b| format!("{} {}", b.name, b.version))
                .collect(),
            error: r.error.clone(),
        })
        .collect();
    Assessment {
        schema: ASSESSMENT_SCHEMA.into(),
        tool: format!("metadata {}", env!("CARGO_PKG_VERSION")),
        case_id: input.case_id.into(),
        assessed_at: input.now.clone(),
        root: root.display().to_string(),
        claims: claims.clone(),
        thresholds: th.clone(),
        inventory,
        project: input.project.clone(),
        git_commits: input.git.map(|g| g.commits.len()),
        findings,
        integrity: None,
        checks_not_implemented: vec![
            "Consistency between source code, documentation, screenshots and project structure (planned; requires content analysis)".into(),
        ],
        limitations: vec![
            "Timestamps embedded in files come from the clock of whatever device or application last wrote them and can be edited.".into(),
            "Filesystem timestamps reflect copying, extraction or synchronization as much as authorship, and are weak evidence on their own.".into(),
            "Git commit dates can be rewritten (amend, rebase, environment overrides) and are not independently verified.".into(),
            "Timestamps without a timezone are compared with a tolerance; observations near a boundary are marked with lower confidence.".into(),
            "Hashes establish content equality with the recorded copy only; they do not establish authorship or creation time.".into(),
            "Missing metadata is common (exports, messaging apps, privacy settings) and is not by itself suspicious.".into(),
        ],
        disclaimer: DISCLAIMER.into(),
    }
}

/// Re-hash inputs after analysis to show the analysis did not alter the evidence.
pub fn verify_unchanged(root: &Path, reports: &[FileReport]) -> Integrity {
    let mut changed = vec![];
    let mut n = 0;
    for r in reports {
        if let Some(before) = &r.sha256 {
            n += 1;
            match sha256_file(Path::new(&r.path)) {
                Ok(after) if &after == before => {}
                _ => changed.push(rel(root, &r.path)),
            }
        }
    }
    Integrity {
        files_rehashed: n,
        unchanged: changed.is_empty(),
        changed,
        note: "SHA-256 computed before analysis and again after; equality shows this run did not modify the files.".into(),
    }
}

/// Mask personal identifiers in evidence values (author names etc.).
pub fn redact(a: &mut Assessment) {
    for f in &mut a.findings {
        for e in &mut f.evidence {
            let name = e.field.rsplit(':').next().unwrap_or("");
            if metadata_core::is_sensitive_name(name) {
                e.value = "[redacted]".into();
            }
        }
    }
}

// ---------------- rules ----------------

fn rule_future(c: &mut Ctx, emb: &[&Event], now: &Option<T>) {
    let Some(now) = now else { return };
    for e in emb {
        let Some(x) = t(&e.time) else { continue };
        if x.secs > now.secs + tol(x.exact, true, c.i.thresholds) {
            c.push(Draft {
                rule: "TS-FUTURE",
                title: "Embedded timestamp is later than the time of analysis".into(),
                severity: Severity::HighPriority,
                priority: 5,
                evidence: vec![ev(e)],
                comparison_rule: format!("embedded timestamp > assessment time ({}) + tolerance", c.i.now.iso),
                explanation: "A timestamp stored in the file lies in the future relative to the analysis clock, which a genuine save normally cannot produce.".into(),
                benign: vec!["The creating device's clock was set incorrectly.", "The analysis machine's clock is wrong.", "Timezone handling by the writing application."],
                obs: conf_obs(x.exact, true),
                interp: Confidence::Medium,
                follow_up: "Check the analysis machine's clock, then ask about the originating device or tool.",
            });
        }
    }
}

fn rule_before_start(c: &mut Ctx, emb: &[&Event], s: &DayRange) {
    for e in emb.iter().filter(|e| e.role == Role::Created) {
        let Some(x) = t(&e.time) else { continue };
        if x.secs + tol(x.exact, claim_exact(s), c.i.thresholds) < s.start {
            let image = cat_of(c.i.reports, c.i.root, &e.file)
                .is_some_and(|r| r.format.category == Category::Image);
            c.push(Draft {
                rule: "TS-BEFORE-START",
                title: "Creation timestamp is earlier than the claimed project start".into(),
                severity: Severity::Review,
                priority: if image { 2 } else { 3 },
                evidence: vec![ev(e)],
                comparison_rule: format!("embedded creation time + {}h tolerance < claimed start ({})", c.i.thresholds.tolerance_hours, c.i.claims.start.clone().unwrap_or_default()),
                explanation: "The file's own metadata says it was created before the student said the project began.".into(),
                benign: [&STD_BENIGN_TIME[..], &["Reference material, stock assets or templates from earlier coursework are often included."]].concat(),
                obs: conf_obs(x.exact, claim_exact(s)),
                interp: Confidence::Low,
                follow_up: "Ask what the file is and where it came from; compare with its content and any earlier milestone submission.",
            });
        }
    }
}

fn rule_after(c: &mut Ctx, emb: &[&Event], d: &DayRange, label: &str, now: &Option<T>) {
    for e in emb {
        let Some(x) = t(&e.time) else { continue };
        if now
            .as_ref()
            .is_some_and(|n| x.secs > n.secs + tol(x.exact, true, c.i.thresholds))
        {
            continue; // reported as TS-FUTURE
        }
        if x.secs - tol(x.exact, claim_exact(d), c.i.thresholds) > d.end {
            let media = e.field.contains("creation_time") || e.field.contains("Tags");
            let (rule, title) = if media {
                (
                    "TS-MEDIA-AFTER",
                    format!("Media container timestamp is later than the {label}"),
                )
            } else {
                (
                    "TS-AFTER",
                    format!("Embedded timestamp is later than the {label}"),
                )
            };
            c.push(Draft {
                rule,
                title,
                severity: Severity::Review,
                priority: if label == "deadline" { 4 } else { 3 },
                evidence: vec![ev(e)],
                comparison_rule: format!("embedded timestamp - {}h tolerance > {label}", c.i.thresholds.tolerance_hours),
                explanation: format!("The file's metadata records a time after the {label}."),
                benign: [&STD_BENIGN_TIME[..], &["The file may have been opened, re-exported or copied after submission preparation.", "Container timestamps record encoding time, not necessarily the time of the recorded event."]].concat(),
                obs: conf_obs(x.exact, claim_exact(d)),
                interp: Confidence::Low,
                follow_up: "Compare with the submission receipt/intake time and ask how the file was exported.",
            });
        }
    }
}

fn rule_modified_before_created(c: &mut Ctx, emb: &[&Event]) {
    let mut by: BTreeMap<(&str, &str), (Vec<&Event>, Vec<&Event>)> = BTreeMap::new();
    for e in emb {
        let group = e.field.split(':').next().unwrap_or("");
        let slot = by.entry((e.file.as_str(), group)).or_default();
        match e.role {
            Role::Created => slot.0.push(e),
            Role::Modified => slot.1.push(e),
            Role::Other => {}
        }
    }
    for ((_, _), (cr, md)) in by {
        let (Some(created), Some(modified)) = (
            cr.iter().min_by_key(|e| e.time.sort_key()),
            md.iter().max_by_key(|e| e.time.sort_key()),
        ) else {
            continue;
        };
        let (Some(a), Some(b)) = (t(&created.time), t(&modified.time)) else {
            continue;
        };
        if b.secs + tol(a.exact, b.exact, c.i.thresholds) < a.secs {
            c.push(Draft {
                rule: "TS-MOD-BEFORE-CREATE",
                title: "Modification time is earlier than creation time within the same file".into(),
                severity: Severity::Review,
                priority: 2,
                evidence: vec![ev(created), ev(modified)],
                comparison_rule: "max(modified) + tolerance < min(created) within one metadata group".into(),
                explanation: "A document normally cannot be last modified before it was created.".into(),
                benign: vec!["Clock changes between saves.", "Metadata copied from another file or template.", "Timezone differences between fields."],
                obs: conf_obs(a.exact, b.exact),
                interp: Confidence::Low,
                follow_up: "Inspect the whole metadata block; check whether the file was produced by a conversion tool.",
            });
        }
    }
}

fn rule_milestone(c: &mut Ctx, emb: &[&Event], name: &str, m: &DayRange, filt: Option<&str>) {
    for e in emb.iter().filter(|e| e.role == Role::Created) {
        if filt.is_some_and(|f| !e.file.to_lowercase().contains(&f.to_lowercase())) {
            continue;
        }
        let Some(x) = t(&e.time) else { continue };
        if x.secs - tol(x.exact, claim_exact(m), c.i.thresholds) > m.end {
            c.push(Draft {
                rule: "MS-LATE-CREATION",
                title: format!("File created after milestone “{name}”"),
                severity: Severity::Review,
                priority: 3,
                evidence: vec![ev(e)],
                comparison_rule: format!("embedded creation time - tolerance > milestone “{name}” date"),
                explanation: "The file is expected to belong to this milestone, but its embedded creation time is later.".into(),
                benign: [&STD_BENIGN_TIME[..], &["The file may be a later re-creation (e.g. re-exported or rebuilt) of earlier work."]].concat(),
                obs: conf_obs(x.exact, claim_exact(m)),
                interp: Confidence::Low,
                follow_up: "Compare with the milestone submission, if one exists, and ask about the file's history.",
            });
        }
    }
}

fn rule_uniform(c: &mut Ctx, events: &[Event]) {
    let min = c.i.thresholds.uniform_min_files;
    let hash_of = |file: &str| {
        cat_of(c.i.reports, c.i.root, file)
            .and_then(|r| r.sha256.clone())
            .unwrap_or_default()
    };
    for (source, rule, role, title, sev, prio) in [
        (
            Source::Filesystem,
            "UNI-FS",
            Role::Modified,
            "Many different files share one filesystem modification time",
            Severity::Info,
            1,
        ),
        (
            Source::Embedded,
            "UNI-EMBEDDED",
            Role::Created,
            "Many different files share one embedded creation time",
            Severity::Review,
            2,
        ),
    ] {
        let mut groups: BTreeMap<&str, Vec<&Event>> = BTreeMap::new();
        for e in events
            .iter()
            .filter(|e| e.source == source && e.role == role)
        {
            groups.entry(e.time.iso.as_str()).or_default().push(e);
        }
        for (iso, evs) in groups {
            let mut hashes: Vec<String> = evs.iter().map(|e| hash_of(&e.file)).collect();
            hashes.sort();
            hashes.dedup();
            let mut files: Vec<&str> = evs.iter().map(|e| e.file.as_str()).collect();
            files.sort();
            files.dedup();
            if hashes.len() >= min && files.len() >= min {
                c.push(Draft {
                    rule,
                    title: title.into(),
                    severity: sev,
                    priority: prio,
                    evidence: evs.iter().map(|e| ev(e)).collect(),
                    comparison_rule: format!("≥{min} files with different content and identical timestamp ({iso})"),
                    explanation: format!("{} distinct files carry exactly the same timestamp, so it describes a bulk operation rather than separate authoring events.", files.len()),
                    benign: vec!["Extracting an archive, cloning a repository, copying a folder or syncing cloud storage assigns one time to many files.", "Documents generated from one template or export share a creation time."],
                    obs: Confidence::High,
                    interp: Confidence::Low,
                    follow_up: "Treat the timestamp as the time of that bulk operation and look at embedded or git evidence instead.",
                });
            }
        }
    }
}

fn rule_ext_mismatch(c: &mut Ctx) {
    for r in
        c.i.reports
            .iter()
            .filter(|r| r.format.extension_match == ExtensionMatch::Mismatch)
    {
        let file = rel(c.i.root, &r.path);
        c.push(Draft {
            rule: "FMT-EXT-MISMATCH",
            title: "File extension does not match detected content".into(),
            severity: Severity::Review,
            priority: 3,
            evidence: vec![Evidence { file, field: "format".into(), value: format!("detected {} ({})", r.format.id, r.format.mime), source: Source::Embedded }],
            comparison_rule: "content signature vs file extension".into(),
            explanation: "The leading bytes identify a different format than the extension suggests.".into(),
            benign: vec!["Renamed files, mis-saved exports, or archives/containers with custom extensions (e.g. .jar, .apk) are common."],
            obs: Confidence::High,
            interp: Confidence::Low,
            follow_up: "Open the file with the tool matching its real format.",
        });
    }
}

fn skip_dup(p: &str) -> bool {
    p.split('/')
        .any(|c| matches!(c, ".git" | "node_modules" | "target" | "__pycache__"))
}

fn rule_duplicates(c: &mut Ctx) {
    let mut by: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for r in c.i.reports.iter().filter(|r| r.size > 0) {
        let rp = rel(c.i.root, &r.path);
        if skip_dup(&rp) {
            continue;
        }
        if let Some(h) = &r.sha256 {
            by.entry(h.as_str()).or_default().push(rp);
        }
    }
    for (h, files) in by.into_iter().filter(|(_, f)| f.len() > 1) {
        c.push(Draft {
            rule: "DUP-CONTENT",
            title: format!("{} files have identical content", files.len()),
            severity: Severity::Info,
            priority: 1,
            evidence: files
                .iter()
                .map(|f| Evidence {
                    file: f.clone(),
                    field: "sha256".into(),
                    value: h.into(),
                    source: Source::Embedded,
                })
                .collect(),
            comparison_rule: "equal SHA-256".into(),
            explanation: "These paths contain byte-identical content.".into(),
            benign: vec!["Intentional copies, backups, shared assets or build outputs."],
            obs: Confidence::High,
            interp: Confidence::Low,
            follow_up: "No action unless duplicates are unexpected for this project.",
        });
    }
}

fn rule_missing_metadata(c: &mut Ctx) {
    let mut docs = vec![];
    let mut photos = vec![];
    let mut pdfs = vec![];
    for r in c.i.reports.iter().filter(|r| r.error.is_none()) {
        let rp = rel(c.i.root, &r.path);
        let has = |names: &[&str]| names.iter().any(|n| !r.find(n).is_empty());
        match r.format.id.as_str() {
            "docx" | "xlsx" | "pptx" | "odt" | "ods" | "odp"
                if !has(&["Created", "CreationDate"]) =>
            {
                docs.push(rp)
            }
            "pdf" if !has(&["CreationDate"]) => pdfs.push(rp),
            "jpeg" | "tiff" | "heic"
                if !has(&["DateTimeOriginal", "DateTime", "DateTimeDigitized"]) =>
            {
                photos.push(rp)
            }
            _ => {}
        }
    }
    for (label, files, field) in [
        ("office/ODF documents", docs, "creation date"),
        ("PDF files", pdfs, "CreationDate"),
        ("photos", photos, "capture/modify date"),
    ] {
        if files.is_empty() {
            continue;
        }
        c.push(Draft {
            rule: "MISS-METADATA",
            title: format!("{} {label} have no {field} metadata", files.len()),
            severity: Severity::Info,
            priority: 1,
            evidence: files.iter().map(|f| Evidence { file: f.clone(), field: field.into(), value: "absent".into(), source: Source::Embedded }).collect(),
            comparison_rule: "format normally exposes the field but it is absent".into(),
            explanation: "Absence of metadata limits what can be said about these files; it is not evidence of anything by itself.".into(),
            benign: vec!["Messaging apps, web uploads, privacy settings and some exporters remove metadata.", "Some tools simply never write it."],
            obs: Confidence::High,
            interp: Confidence::Low,
            follow_up: "None required; consider other evidence sources.",
        });
    }
}

fn rule_incremental(
    c: &mut Ctx,
    start: &Option<DayRange>,
    deadline: &Option<DayRange>,
    completion: &Option<DayRange>,
) {
    let mut ev_list = vec![];
    for r in c.i.reports {
        let f = rel(c.i.root, &r.path);
        let num = |n: &str| {
            r.find(n)
                .first()
                .and_then(|x| x.raw.parse::<u64>().ok())
                .unwrap_or(0)
        };
        let (rev, tracked) = (
            num("Revision"),
            num("TrackedInsertions") + num("TrackedDeletions"),
        );
        let pdf_rev = if r.format.id == "pdf" {
            num("Revisions")
        } else {
            0
        };
        if rev > 1 {
            ev_list.push(Evidence {
                file: f.clone(),
                field: "OOXML-Core:Revision".into(),
                value: rev.to_string(),
                source: Source::Embedded,
            });
        }
        if tracked > 0 {
            ev_list.push(Evidence {
                file: f.clone(),
                field: "Document:TrackedChanges".into(),
                value: tracked.to_string(),
                source: Source::Embedded,
            });
        }
        if pdf_rev > 1 {
            ev_list.push(Evidence {
                file: f,
                field: "PDF:Revisions".into(),
                value: pdf_rev.to_string(),
                source: Source::Embedded,
            });
        }
    }
    let git_n = c.i.git.map(|g| g.commits.len()).unwrap_or(0);
    if git_n > 1 {
        ev_list.push(Evidence {
            file: "(git)".into(),
            field: "commits".into(),
            value: git_n.to_string(),
            source: Source::Git,
        });
    }
    if !ev_list.is_empty() {
        c.push(Draft {
            rule: "INC-PRESENT",
            title: "Evidence of incremental changes is present".into(),
            severity: Severity::Info,
            priority: 1,
            evidence: ev_list,
            comparison_rule: "revision counters, tracked changes, PDF incremental updates, git commits".into(),
            explanation: "These indicators show files were saved or updated more than once. They say nothing about the duration or amount of work.".into(),
            benign: vec!["Revision counters are application-specific and not an editing duration."],
            obs: Confidence::High,
            interp: Confidence::Low,
            follow_up: "None required.",
        });
    } else if let (Some(s), Some(e)) = (start, deadline.as_ref().or(completion.as_ref())) {
        if (e.end - s.start) / 86_400 >= 30 && !c.i.reports.is_empty() {
            c.push(Draft {
                rule: "INC-ABSENT",
                title: "No incremental-change evidence found for a long claimed work period".into(),
                severity: Severity::Info,
                priority: 2,
                evidence: vec![Evidence { file: "(project)".into(), field: "claimed period".into(), value: format!("{} days", (e.end - s.start) / 86_400), source: Source::Declared }],
                comparison_rule: "claimed period ≥ 30 days and no revisions, tracked changes, PDF updates or git history".into(),
                explanation: "Nothing in the submitted files shows intermediate saves.".into(),
                benign: vec!["Work may have been done in tools that do not record history, or only final exports were submitted."],
                obs: Confidence::Medium,
                interp: Confidence::Low,
                follow_up: "Ask for working files or earlier milestone submissions.",
            });
        }
    }
}

fn rule_git(
    c: &mut Ctx,
    g: &GitHistory,
    start: &Option<DayRange>,
    deadline: &Option<DayRange>,
    now: &Option<T>,
) {
    if g.commits.is_empty() {
        return;
    }
    let th = c.i.thresholds;
    let secs = |tv: &TimeValue| tv.sort_key().unwrap_or(0);
    let first = g
        .commits
        .iter()
        .min_by_key(|c| secs(&c.author_date))
        .unwrap();
    let last = g
        .commits
        .iter()
        .max_by_key(|c| secs(&c.author_date))
        .unwrap();
    let gev = |cm: &crate::git::Commit, f: &str| Evidence {
        file: format!("git:{}", &cm.hash[..10.min(cm.hash.len())]),
        field: f.into(),
        value: cm.author_date.iso.clone(),
        source: Source::Git,
    };
    if let Some(s) = start {
        let x = secs(&first.author_date);
        if x + tol(true, claim_exact(s), th) < s.start {
            c.push(Draft {
                rule: "GIT-BEFORE-START",
                title: "Git history begins before the claimed project start".into(),
                severity: Severity::Review,
                priority: 3,
                evidence: vec![gev(first, "first commit author date")],
                comparison_rule: "first commit - tolerance < claimed start".into(),
                explanation: "The repository contains commits older than the claimed start date."
                    .into(),
                benign: vec![
                    "Repository initialised from a template or earlier project.",
                    "Cloned or forked starter code.",
                    "The claimed start date may be approximate.",
                ],
                obs: Confidence::High,
                interp: Confidence::Low,
                follow_up: "Ask which commits predate the project and where they came from.",
            });
        } else if x - s.end > th.gap_days * 86_400 {
            c.push(Draft {
                rule: "GIT-LATE-START",
                title: "Available git history starts long after the claimed project start".into(),
                severity: Severity::Info,
                priority: 2,
                evidence: vec![gev(first, "first commit author date")],
                comparison_rule: format!("first commit - claimed start > {} days", th.gap_days),
                explanation: "There is no repository history for the first part of the claimed period.".into(),
                benign: vec!["Version control may have been adopted later, or history squashed/re-initialised."],
                obs: Confidence::High,
                interp: Confidence::Low,
                follow_up: "Ask how the early work was tracked.",
            });
        }
    }
    if let Some(d) = deadline {
        let late: Vec<_> = g
            .commits
            .iter()
            .filter(|cm| secs(&cm.author_date) - tol(true, claim_exact(d), th) > d.end)
            .collect();
        if !late.is_empty() {
            c.push(Draft {
                rule: "GIT-AFTER-DEADLINE",
                title: format!("{} commit(s) dated after the deadline", late.len()),
                severity: Severity::Review,
                priority: 4,
                evidence: late.iter().map(|cm| gev(cm, "author date")).collect(),
                comparison_rule: "commit author date - tolerance > deadline".into(),
                explanation: "The repository contains work dated after the submission deadline.".into(),
                benign: vec!["Post-submission cleanup in the same repository; the submitted snapshot may predate these commits."],
                obs: Confidence::High,
                interp: Confidence::Low,
                follow_up: "Check which commit corresponds to the submitted snapshot.",
            });
        }
    }
    if let Some(n) = now {
        let fut: Vec<_> = g
            .commits
            .iter()
            .filter(|cm| {
                secs(&cm.author_date) > n.secs + th.tolerance_hours * 3600
                    || secs(&cm.commit_date) > n.secs + th.tolerance_hours * 3600
            })
            .collect();
        if !fut.is_empty() {
            c.push(Draft {
                rule: "GIT-FUTURE",
                title: format!("{} commit(s) dated in the future", fut.len()),
                severity: Severity::HighPriority,
                priority: 5,
                evidence: fut.iter().map(|cm| gev(cm, "date")).collect(),
                comparison_rule: "commit date > assessment time + tolerance".into(),
                explanation: "Commit dates after the analysis time cannot be genuine unless a clock was wrong or dates were set manually.".into(),
                benign: vec!["Wrong system clock on the committing machine.", "The analysis machine's clock is wrong."],
                obs: Confidence::High,
                interp: Confidence::Medium,
                follow_up: "Verify the analysis clock; then ask about the committing environment.",
            });
        }
    }
    // child dated before its parent
    let by_hash: std::collections::HashMap<&str, &crate::git::Commit> =
        g.commits.iter().map(|cm| (cm.hash.as_str(), cm)).collect();
    let mut back = vec![];
    for cm in &g.commits {
        for p in &cm.parents {
            if let Some(pc) = by_hash.get(p.as_str()) {
                if secs(&cm.commit_date) + 120 < secs(&pc.commit_date) {
                    back.push((cm, *pc));
                }
            }
        }
    }
    if !back.is_empty() {
        c.push(Draft {
            rule: "GIT-NONMONOTONIC",
            title: format!(
                "{} commit(s) are dated earlier than their parent",
                back.len()
            ),
            severity: Severity::Review,
            priority: 3,
            evidence: back
                .iter()
                .map(|(cm, pc)| Evidence {
                    file: format!("git:{}", &cm.hash[..10.min(cm.hash.len())]),
                    field: "commit date vs parent".into(),
                    value: format!("{} < parent {}", cm.commit_date.iso, pc.commit_date.iso),
                    source: Source::Git,
                })
                .collect(),
            comparison_rule: "child committer date + 2 min < parent committer date".into(),
            explanation: "History order and recorded dates disagree.".into(),
            benign: vec![
                "Clock changes or skew between machines.",
                "Rebases, cherry-picks or merges of work done elsewhere.",
            ],
            obs: Confidence::High,
            interp: Confidence::Low,
            follow_up: "Inspect these commits' author/committer dates and the branch structure.",
        });
    }
    let skewed: Vec<_> = g
        .commits
        .iter()
        .filter(|cm| (secs(&cm.author_date) - secs(&cm.commit_date)).abs() > 86_400)
        .collect();
    if !skewed.is_empty() {
        c.push(Draft {
            rule: "GIT-AUTHOR-COMMIT-SKEW",
            title: format!(
                "{} commit(s) have author and committer dates more than a day apart",
                skewed.len()
            ),
            severity: Severity::Info,
            priority: 1,
            evidence: skewed
                .iter()
                .map(|cm| Evidence {
                    file: format!("git:{}", &cm.hash[..10.min(cm.hash.len())]),
                    field: "author vs commit date".into(),
                    value: format!("{} / {}", cm.author_date.iso, cm.commit_date.iso),
                    source: Source::Git,
                })
                .collect(),
            comparison_rule: "|author date - committer date| > 24h".into(),
            explanation: "Typical of rebased, amended or cherry-picked commits.".into(),
            benign: vec!["Ordinary rebase or amend workflows."],
            obs: Confidence::High,
            interp: Confidence::Low,
            follow_up: "None unless many commits are affected.",
        });
    }
    let _ = last;
}

fn rule_gaps(c: &mut Ctx, emb: &[&Event], start: &Option<DayRange>, end: &Option<DayRange>) {
    let mut times: Vec<(i64, String)> = emb
        .iter()
        .filter_map(|e| {
            e.time
                .sort_key()
                .map(|k| (k, format!("{} {}", e.file, e.field)))
        })
        .collect();
    if let Some(g) = c.i.git {
        times.extend(g.commits.iter().filter_map(|cm| {
            cm.author_date
                .sort_key()
                .map(|k| (k, format!("git:{}", &cm.hash[..10.min(cm.hash.len())])))
        }));
    }
    if let Some(s) = start {
        times.retain(|(k, _)| *k >= s.start);
    }
    if let Some(e) = end {
        times.retain(|(k, _)| *k <= e.end);
    }
    times.sort();
    times.dedup_by_key(|(k, _)| *k / 3600);
    if times.len() < 3 {
        return;
    }
    let gap = c.i.thresholds.gap_days * 86_400;
    let evs: Vec<Evidence> = times
        .windows(2)
        .filter(|w| w[1].0 - w[0].0 > gap)
        .map(|w| Evidence {
            file: "(timeline)".into(),
            field: "gap".into(),
            value: format!(
                "{} days between {} and {}",
                (w[1].0 - w[0].0) / 86_400,
                w[0].1,
                w[1].1
            ),
            source: Source::Embedded,
        })
        .collect();
    if !evs.is_empty() {
        c.push(Draft {
            rule: "GAP-HISTORY",
            title: format!(
                "{} gap(s) of more than {} days in the available history",
                evs.len(),
                c.i.thresholds.gap_days
            ),
            severity: Severity::Info,
            priority: 1,
            evidence: evs,
            comparison_rule:
                "consecutive embedded/git timestamps further apart than the gap threshold".into(),
            explanation: "No dated artefacts exist for these periods in what was submitted.".into(),
            benign: vec![
                "Work in tools that leave no dated artefacts.",
                "Breaks in the project, holidays or exams.",
            ],
            obs: Confidence::Medium,
            interp: Confidence::Low,
            follow_up: "Ask what happened during the gap if the milestone plan expected activity.",
        });
    }
}

fn rule_previous(c: &mut Ctx, proot: &Path, prev: &[FileReport]) {
    fn key<'a>(root: &Path, r: &'a FileReport) -> (String, &'a FileReport) {
        (rel(root, &r.path), r)
    }
    let old: BTreeMap<String, &FileReport> = prev.iter().map(|r| key(proot, r)).collect();
    let new: BTreeMap<String, &FileReport> = c.i.reports.iter().map(|r| key(c.i.root, r)).collect();
    let old_hashes: std::collections::HashSet<&str> =
        prev.iter().filter_map(|r| r.sha256.as_deref()).collect();
    let (mut added, mut removed, mut changed, mut same) = (vec![], vec![], vec![], 0usize);
    for (p, n) in &new {
        match old.get(p) {
            None => added.push(p.clone()),
            Some(o) if o.sha256 == n.sha256 => same += 1,
            Some(_) => changed.push(p.clone()),
        }
    }
    for p in old.keys() {
        if !new.contains_key(p) {
            removed.push(p.clone());
        }
    }
    let mut evid = vec![];
    for (label, list) in [("added", &added), ("removed", &removed), ("changed", &changed)] {
        for p in list {
            evid.push(Evidence { file: p.clone(), field: "vs previous submission".into(), value: label.into(), source: Source::Embedded });
        }
    }
    c.push(Draft {
        rule: "SUB-DIFF",
        title: format!("Compared with previous submission: {} added, {} removed, {} changed, {} identical", added.len(), removed.len(), changed.len(), same),
        severity: Severity::Info,
        priority: 1,
        evidence: evid,
        comparison_rule: "path-wise SHA-256 comparison".into(),
        explanation: "Summarises how the submission evolved between the two snapshots.".into(),
        benign: vec!["Normal project progress."],
        obs: Confidence::High,
        interp: Confidence::Low,
        follow_up: "Review large rewrites against the milestone plan.",
    });
    // repeated assets: identical content at a different path
    let moved: Vec<Evidence> = new
        .iter()
        .filter(|(p, n)| !old.contains_key(*p) && n.sha256.as_deref().is_some_and(|h| old_hashes.contains(h)))
        .map(|(p, _)| Evidence { file: p.clone(), field: "sha256".into(), value: "identical content existed at another path previously".into(), source: Source::Embedded })
        .collect();
    if !moved.is_empty() {
        c.push(Draft {
            rule: "DUP-PREVIOUS",
            title: format!("{} file(s) repeat content from the previous submission under new paths", moved.len()),
            severity: Severity::Info,
            priority: 1,
            evidence: moved,
            comparison_rule: "equal SHA-256 at a different relative path".into(),
            explanation: "Assets were renamed or relocated between submissions.".into(),
            benign: vec!["Restructuring or renaming."],
            obs: Confidence::High,
            interp: Confidence::Low,
            follow_up: "None unless unexpected.",
        });
    }
    // timestamp evolution for same paths
    for (p, n) in &new {
        let Some(o) = old.get(p) else { continue };
        if o.sha256 == n.sha256 {
            continue;
        }
        let first = |r: &FileReport, role: Role| r.fields.iter().filter(|f| f.provenance == metadata_core::Provenance::Stored && f.time.is_some() && timeline::role_of(&f.name) == role).map(|f| (f.key(), f.time.clone().unwrap())).collect::<Vec<_>>();
        let (oc, nc) = (first(o, Role::Created), first(n, Role::Created));
        for (k, nt) in &nc {
            if let Some((_, ot)) = oc.iter().find(|(ok, _)| ok == k) {
                if ot.iso != nt.iso {
                    c.push(Draft {
                        rule: "SUB-CREATED-CHANGED",
                        title: "Embedded creation time differs from the previous submission of the same file".into(),
                        severity: Severity::Review,
                        priority: 3,
                        evidence: vec![Evidence { file: p.clone(), field: k.clone(), value: format!("{} → {}", ot.iso, nt.iso), source: Source::Embedded }],
                        comparison_rule: "same path, changed content, different embedded creation time".into(),
                        explanation: "A creation time normally stays constant across saves of one document.".into(),
                        benign: vec!["The file was re-created via 'Save As', export, conversion or copy-paste into a new document."],
                        obs: Confidence::High,
                        interp: Confidence::Low,
                        follow_up: "Ask whether the document was rebuilt between submissions.",
                    });
                }
            }
        }
        let (om, nm) = (first(o, Role::Modified), first(n, Role::Modified));
        for (k, nt) in &nm {
            if let Some((_, ot)) = om.iter().find(|(ok, _)| ok == k) {
                if let (Some(a), Some(b)) = (t(ot), t(nt)) {
                    if b.secs + tol(a.exact, b.exact, c.i.thresholds) < a.secs {
                        c.push(Draft {
                            rule: "SUB-MODIFIED-BACKWARDS",
                            title: "Content changed but the embedded modification time is earlier than before".into(),
                            severity: Severity::Review,
                            priority: 3,
                            evidence: vec![Evidence { file: p.clone(), field: k.clone(), value: format!("{} → {}", ot.iso, nt.iso), source: Source::Embedded }],
                            comparison_rule: "same path, different hash, modification time decreased beyond tolerance".into(),
                            explanation: "Newer content normally carries a later modification time.".into(),
                            benign: vec!["Clock changes; content produced by a different tool or machine."],
                            obs: conf_obs(a.exact, b.exact),
                            interp: Confidence::Low,
                            follow_up: "Compare the two versions' content and producing applications.",
                        });
                    }
                }
            }
        }
    }
}
