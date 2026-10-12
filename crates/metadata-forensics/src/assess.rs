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
    Some(T { secs: tv.sort_key()?, exact: tv.offset_known })
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
        let mut keys: Vec<String> = d.evidence.iter().map(|e| format!("{}|{}|{}", e.file, e.field, e.value)).collect();
        keys.sort();
        let id = format!("{}-{}", d.rule, &sha256_bytes(format!("{}\n{}", d.rule, keys.join("\n")).as_bytes())[..8]);
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
    Evidence { file: e.file.clone(), field: e.field.clone(), value: e.value.clone(), source: e.source }
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
    let mut c = Ctx { i: input, out: vec![] };
    let root = input.root;
    let events = timeline::file_events(root, input.reports);
    let embedded: Vec<&Event> = events.iter().filter(|e| e.source == Source::Embedded).collect();
    let claims = input.claims;
    let th = input.thresholds;
    let start = claims.start.as_deref().and_then(|s| parse_claim(s).ok());
    let deadline = claims.deadline.as_deref().and_then(|s| parse_claim(s).ok());
    let completion = claims.completion.as_deref().and_then(|s| parse_claim(s).ok());
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
    findings.sort_by(|a, b| b.severity.cmp(&a.severity).then(b.review_priority.cmp(&a.review_priority)).then(a.id.cmp(&b.id)));
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
            warnings: r.warnings.iter().map(|w| format!("[{}] {}", w.backend, w.message)).collect(),
            backends: r.backends.iter().map(|b| format!("{} {}", b.name, b.version)).collect(),
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
            let image = cat_of(c.i.reports, c.i.root, &e.file).is_some_and(|r| r.format.category == Category::Image);
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
        if now.as_ref().is_some_and(|n| x.secs > n.secs + tol(x.exact, true, c.i.thresholds)) {
            continue; // reported as TS-FUTURE
        }
        if x.secs - tol(x.exact, claim_exact(d), c.i.thresholds) > d.end {
            let media = e.field.contains("creation_time") || e.field.contains("Tags");
            let (rule, title) = if media {
                ("TS-MEDIA-AFTER", format!("Media container timestamp is later than the {label}"))
            } else {
                ("TS-AFTER", format!("Embedded timestamp is later than the {label}"))
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
        let (Some(created), Some(modified)) = (cr.iter().min_by_key(|e| e.time.sort_key()), md.iter().max_by_key(|e| e.time.sort_key())) else { continue };
        let (Some(a), Some(b)) = (t(&created.time), t(&modified.time)) else { continue };
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
    let hash_of = |file: &str| cat_of(c.i.reports, c.i.root, file).and_then(|r| r.sha256.clone()).unwrap_or_default();
    for (source, rule, role, title, sev, prio) in [
        (Source::Filesystem, "UNI-FS", Role::Modified, "Many different files share one filesystem modification time", Severity::Info, 1),
        (Source::Embedded, "UNI-EMBEDDED", Role::Created, "Many different files share one embedded creation time", Severity::Review, 2),
    ] {
        let mut groups: BTreeMap<&str, Vec<&Event>> = BTreeMap::new();
        for e in events.iter().filter(|e| e.source == source && e.role == role) {
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
    for r in c.i.reports.iter().filter(|r| r.format.extension_match == ExtensionMatch::Mismatch) {
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

