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
