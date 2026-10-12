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
