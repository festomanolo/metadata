//! Field-level comparison of two file reports (or two report sets matched by relative path).
use metadata_core::FileReport;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiffStatus {
    Added,
    Removed,
    Changed,
    Identical,
    /// A backend needed to read this family on one side was unavailable.
    Unavailable,
}

#[derive(Debug, Clone, Serialize)]
pub struct FieldDiff {
    pub key: String,
    pub status: DiffStatus,
    pub left: Option<String>,
    pub right: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Comparison {
    pub left: String,
    pub right: String,
    pub content_identical: Option<bool>,
    pub format_left: String,
    pub format_right: String,
    pub fields: Vec<FieldDiff>,
    /// Notes where a comparison is incomplete (missing backends, errors).
    pub caveats: Vec<String>,
}

impl Comparison {
    pub fn count(&self, s: DiffStatus) -> usize {
        self.fields.iter().filter(|f| f.status == s).count()
    }
}

fn index(r: &FileReport) -> BTreeMap<String, String> {
    let mut m: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for f in &r.fields {
        // Archive entries are listed per path; keep each distinct.
        let key = if f.name == "Entry" {
            format!("{}:{}", f.group, f.raw)
        } else {
            f.key()
        };
        m.entry(key).or_default().push(f.raw.clone());
    }
    m.into_iter().map(|(k, v)| (k, v.join(" | "))).collect()
}

pub fn compare_reports(a: &FileReport, b: &FileReport) -> Comparison {
    let (ia, ib) = (index(a), index(b));
    let keys: BTreeSet<&String> = ia.keys().chain(ib.keys()).collect();
    let mut fields = vec![];
    for k in keys {
        let (l, r) = (ia.get(k), ib.get(k));
        let status = match (l, r) {
            (Some(x), Some(y)) if x == y => DiffStatus::Identical,
            (Some(_), Some(_)) => DiffStatus::Changed,
            (Some(_), None) => DiffStatus::Removed,
            (None, Some(_)) => DiffStatus::Added,
            (None, None) => unreachable!(),
        };
        fields.push(FieldDiff {
            key: k.clone(),
            status,
            left: l.cloned(),
            right: r.cloned(),
        });
    }
    let mut caveats = vec![];
    for (side, r) in [("left", a), ("right", b)] {
        if let Some(e) = &r.error {
            caveats.push(format!("{side}: extraction error: {e}"));
        }
        for u in &r.unavailable {
            caveats.push(format!(
                "{side}: {u}; fields may be missing from the comparison"
            ));
        }
    }
    if a.format.id != b.format.id {
        caveats.push(format!(
            "formats differ ({} vs {}); field names are not directly comparable",
            a.format.id, b.format.id
        ));
    }
    Comparison {
        left: a.path.clone(),
        right: b.path.clone(),
        content_identical: a
            .sha256
            .as_ref()
            .zip(b.sha256.as_ref())
            .map(|(x, y)| x == y),
        format_left: a.format.id.clone(),
        format_right: b.format.id.clone(),
        fields,
        caveats,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SetComparison {
    pub pairs: Vec<Comparison>,
    pub only_left: Vec<String>,
    pub only_right: Vec<String>,
}

/// Pair reports by path relative to their roots.
pub fn compare_sets(
    left_root: &std::path::Path,
    left: &[FileReport],
    right_root: &std::path::Path,
    right: &[FileReport],
) -> SetComparison {
    let rel = |root: &std::path::Path, r: &FileReport| {
        std::path::Path::new(&r.path)
            .strip_prefix(root)
            .unwrap_or(std::path::Path::new(&r.path))
            .display()
            .to_string()
    };
    let lm: BTreeMap<String, &FileReport> = left.iter().map(|r| (rel(left_root, r), r)).collect();
    let rm: BTreeMap<String, &FileReport> = right.iter().map(|r| (rel(right_root, r), r)).collect();
    let mut pairs = vec![];
    let (mut only_left, mut only_right) = (vec![], vec![]);
    for (k, l) in &lm {
        match rm.get(k) {
            Some(r) => pairs.push(compare_reports(l, r)),
            None => only_left.push(k.clone()),
        }
    }
    for k in rm.keys() {
        if !lm.contains_key(k) {
            only_right.push(k.clone());
        }
    }
    SetComparison {
        pairs,
        only_left,
        only_right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use metadata_core::*;
    fn report(path: &str, fields: &[(&str, &str, &str)]) -> FileReport {
        FileReport {
            schema_version: SCHEMA_VERSION.into(),
            path: path.into(),
            size: 1,
            sha256: Some(path.into()),
            format: FormatInfo {
                id: "docx".into(),
                description: String::new(),
                mime: String::new(),
                category: Category::Document,
                extension_match: ExtensionMatch::Match,
                signature_hex: String::new(),
            },
            filesystem: FsInfo::default(),
            fields: fields
                .iter()
                .map(|(g, n, v)| Field::new(g, n, *v, "t", Provenance::Stored))
                .collect(),
            warnings: vec![],
            backends: vec![],
            unavailable: vec![],
            error: None,
        }
    }
    #[test]
    fn classifies_all_states() {
        let a = report(
            "a",
            &[
                ("C", "Creator", "Ann"),
                ("C", "Title", "T"),
                ("C", "Gone", "x"),
            ],
        );
        let b = report(
            "b",
            &[
                ("C", "Creator", "Bob"),
                ("C", "Title", "T"),
                ("C", "New", "y"),
            ],
        );
        let c = compare_reports(&a, &b);
        assert_eq!(c.count(DiffStatus::Changed), 1);
        assert_eq!(c.count(DiffStatus::Identical), 1);
        assert_eq!(c.count(DiffStatus::Removed), 1);
        assert_eq!(c.count(DiffStatus::Added), 1);
        assert_eq!(c.content_identical, Some(false));
    }
    #[test]
    fn caveat_on_unavailable() {
        let mut a = report("a", &[]);
        a.unavailable.push("needs exiftool".into());
        assert!(!compare_reports(&a, &report("b", &[])).caveats.is_empty());
    }
}
