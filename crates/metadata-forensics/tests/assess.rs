use metadata_core::{FileReport, TimeValue};
use metadata_engine::{collect_files, Engine, Options, ScanOptions};
use metadata_forensics::assess::{assess, verify_unchanged, Assessment, Input, Severity};
use metadata_forensics::claims::{Claims, Milestone, Thresholds};
use metadata_forensics::project::ProjectSummary;
use metadata_testkit as kit;
use std::path::Path;

fn put(dir: &Path, name: &str, data: &[u8]) {
    let p = dir.join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, data).unwrap();
}
fn scan(root: &Path) -> Vec<FileReport> {
    let e = Engine::new(Options {
        use_external: false,
        ..Default::default()
    });
    let files = collect_files(
        &[root.to_path_buf()],
        &ScanOptions {
            recursive: true,
            ..Default::default()
        },
    )
    .files;
    e.inspect_many(&files, 2, &|_, _| {})
}
fn docx(created: &str, modified: &str) -> Vec<u8> {
    kit::docx(&kit::DocxSpec {
        creator: "Ann",
        last_modified_by: "Ann",
        created,
        modified,
        revision: 1,
        tracked_insertions: 0,
        application: "W",
    })
}
fn run(root: &Path, reports: &[FileReport], claims: &Claims, now: &str) -> Assessment {
    run_with(root, reports, claims, now, None, None)
}
fn run_with(
    root: &Path,
    reports: &[FileReport],
    claims: &Claims,
    now: &str,
    git: Option<&metadata_forensics::git::GitHistory>,
    prev: Option<(&Path, &[FileReport])>,
) -> Assessment {
    assess(&Input {
        case_id: "t1",
        root,
        reports,
        claims,
        thresholds: &Thresholds::default(),
        git,
        previous: prev,
        project: ProjectSummary::default(),
        now: metadata_core::time::parse_iso(now).unwrap(),
    })
}
fn rules(a: &Assessment) -> Vec<&str> {
    a.findings.iter().map(|f| f.rule.as_str()).collect()
}
fn claims(start: &str, deadline: &str) -> Claims {
    Claims {
        start: Some(start.into()),
        deadline: Some(deadline.into()),
        ..Default::default()
    }
}

#[test]
fn created_before_start_flags_review_with_full_explanation() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "old.docx",
        &docx("2023-01-01T09:00:00Z", "2024-02-01T09:00:00Z"),
    );
    let r = scan(d.path());
    let a = run(
        d.path(),
        &r,
        &claims("2024-01-10", "2024-06-01"),
        "2024-06-02T00:00:00Z",
    );
    let f = a
        .findings
        .iter()
        .find(|f| f.rule == "TS-BEFORE-START")
        .expect("finding");
    assert_eq!(f.severity, Severity::Review);
    assert_eq!(f.files, vec!["old.docx"]);
    assert!(f.evidence[0].field.contains("Created") && f.evidence[0].value.contains("2023-01-01"));
    assert!(
        !f.benign_explanations.is_empty()
            && !f.explanation.is_empty()
            && !f.follow_up.is_empty()
            && !f.comparison_rule.is_empty()
    );
    assert!(f.id.starts_with("TS-BEFORE-START-"));
    let json = serde_json::to_string(&a).unwrap();
    assert!(json.contains("\"REVIEW\""));
    assert!(!json.to_lowercase().contains("guilty") && !json.to_lowercase().contains("cheat"));
}

#[test]
fn within_tolerance_is_not_flagged() {
    let d = tempfile::tempdir().unwrap();
    // created 8 hours before the start of the claimed day: inside the 24h tolerance
    put(
        d.path(),
        "a.docx",
        &docx("2024-01-09T16:00:00Z", "2024-02-01T09:00:00Z"),
    );
    let a = run(
        d.path(),
        &scan(d.path()),
        &claims("2024-01-10", "2024-06-01"),
        "2024-06-02T00:00:00Z",
    );
    assert!(!rules(&a).contains(&"TS-BEFORE-START"), "{:?}", rules(&a));
}

#[test]
fn future_timestamp_is_high_priority_and_sorted_first() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "f.docx",
        &docx("2024-01-12T09:00:00Z", "2031-01-01T00:00:00Z"),
    );
    put(d.path(), "m.jpg", &kit::jpeg(None, None, 2, 2));
    let a = run(
        d.path(),
        &scan(d.path()),
        &claims("2024-01-10", "2024-06-01"),
        "2024-06-02T00:00:00Z",
    );
    assert_eq!(a.findings[0].severity, Severity::HighPriority);
    assert_eq!(a.findings[0].rule, "TS-FUTURE");
    assert!(
        !rules(&a).contains(&"TS-AFTER"),
        "future events must not double-report"
    );
}

#[test]
fn after_deadline_and_modified_before_created() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "late.docx",
        &docx("2024-02-01T09:00:00Z", "2024-07-15T09:00:00Z"),
    );
    put(
        d.path(),
        "odd.docx",
        &docx("2024-05-01T09:00:00Z", "2024-03-01T09:00:00Z"),
    );
    let a = run(
        d.path(),
        &scan(d.path()),
        &claims("2024-01-10", "2024-06-01"),
        "2024-08-01T00:00:00Z",
    );
    assert!(a
        .findings
        .iter()
        .any(|f| f.rule == "TS-AFTER" && f.files == vec!["late.docx"]));
    assert!(a
        .findings
        .iter()
        .any(|f| f.rule == "TS-MOD-BEFORE-CREATE" && f.files == vec!["odd.docx"]));
}

#[test]
fn milestone_rule_respects_path_filter() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "proposal/p.docx",
        &docx("2024-03-01T09:00:00Z", "2024-03-02T09:00:00Z"),
    );
    put(
        d.path(),
        "final/f.docx",
        &docx("2024-03-01T09:00:00Z", "2024-03-02T09:00:00Z"),
    );
    let mut c = claims("2024-01-10", "2024-06-01");
    c.milestones.push(Milestone {
        name: "Proposal".into(),
        date: "2024-02-01".into(),
        path_contains: Some("proposal".into()),
    });
    let a = run(d.path(), &scan(d.path()), &c, "2024-08-01T00:00:00Z");
    let hits: Vec<_> = a
        .findings
        .iter()
        .filter(|f| f.rule == "MS-LATE-CREATION")
        .collect();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].files, vec!["proposal/p.docx"]);
}

#[test]
fn ids_are_stable_and_path_relative() {
    let mk = || {
        let d = tempfile::tempdir().unwrap();
        put(
            d.path(),
            "old.docx",
            &docx("2023-01-01T09:00:00Z", "2024-02-01T09:00:00Z"),
        );
        let r = scan(d.path());
        let a = run(
            d.path(),
            &r,
            &claims("2024-01-10", "2024-06-01"),
            "2024-06-02T00:00:00Z",
        );
        a.findings.iter().map(|f| f.id.clone()).collect::<Vec<_>>()
    };
    assert_eq!(mk(), mk(), "ids must not depend on the temp directory");
}

#[test]
fn missing_metadata_is_info_only() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "bare.jpg", &kit::jpeg(None, None, 2, 2));
    let a = run(
        d.path(),
        &scan(d.path()),
        &Claims::default(),
        "2024-06-02T00:00:00Z",
    );
    let f = a
        .findings
        .iter()
        .find(|f| f.rule == "MISS-METADATA")
        .expect("missing metadata finding");
    assert_eq!(f.severity, Severity::Info);
    assert!(f.explanation.contains("not evidence"));
}

#[test]
fn uniform_filesystem_times_and_duplicates() {
    let d = tempfile::tempdir().unwrap();
    for i in 0..6 {
        put(
            d.path(),
            &format!("f{i}.txt"),
            format!("content {i}").as_bytes(),
        );
    }
    put(d.path(), "a/same.txt", b"dup");
    put(d.path(), "b/same.txt", b"dup");
    let mut r = scan(d.path());
    let t = TimeValue::naive(
        chrono::NaiveDate::from_ymd_opt(2024, 3, 3)
            .unwrap()
            .and_hms_opt(1, 2, 3)
            .unwrap(),
    );
    for x in &mut r {
        x.filesystem.modified = Some(t.clone());
    }
    let a = run(d.path(), &r, &Claims::default(), "2024-06-02T00:00:00Z");
    let uni = a
        .findings
        .iter()
        .find(|f| f.rule == "UNI-FS")
        .expect("uniform");
    assert_eq!(uni.severity, Severity::Info);
    assert!(a
        .findings
        .iter()
        .any(|f| f.rule == "DUP-CONTENT" && f.files.len() == 2));
}

#[test]
fn extension_mismatch_finding() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "report.pdf", &kit::jpeg(None, None, 2, 2));
    let a = run(
        d.path(),
        &scan(d.path()),
        &Claims::default(),
        "2024-06-02T00:00:00Z",
    );
    assert!(rules(&a).contains(&"FMT-EXT-MISMATCH"));
}

#[test]
fn previous_submission_comparison() {
    let old = tempfile::tempdir().unwrap();
    let new = tempfile::tempdir().unwrap();
    put(old.path(), "keep.txt", b"same");
    put(
        old.path(),
        "chg.docx",
        &docx("2024-01-12T09:00:00Z", "2024-02-01T09:00:00Z"),
    );
    put(new.path(), "keep.txt", b"same");
    // same path, new content, different created, modified went backwards
    put(
        new.path(),
        "chg.docx",
        &docx("2024-03-01T09:00:00Z", "2024-01-15T09:00:00Z"),
    );
    put(new.path(), "added.txt", b"new");
    let (ro, rn) = (scan(old.path()), scan(new.path()));
    let a = run_with(
        new.path(),
        &rn,
        &Claims::default(),
        "2024-06-02T00:00:00Z",
        None,
        Some((old.path(), &ro)),
    );
    let diff = a.findings.iter().find(|f| f.rule == "SUB-DIFF").unwrap();
    assert!(
        diff.title.contains("1 added")
            && diff.title.contains("1 changed")
            && diff.title.contains("1 identical"),
        "{}",
        diff.title
    );
    assert!(rules(&a).contains(&"SUB-CREATED-CHANGED"));
    assert!(rules(&a).contains(&"SUB-MODIFIED-BACKWARDS"));
}

#[test]
fn integrity_check_detects_modification() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "a.txt", b"one");
    let r = scan(d.path());
    assert!(verify_unchanged(d.path(), &r).unchanged);
    std::fs::write(d.path().join("a.txt"), b"two").unwrap();
    let i = verify_unchanged(d.path(), &r);
    assert!(!i.unchanged);
    assert_eq!(i.changed, vec!["a.txt"]);
}

#[test]
fn assessment_never_modifies_inputs() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "a.docx",
        &docx("2023-01-01T09:00:00Z", "2024-02-01T09:00:00Z"),
    );
    let r = scan(d.path());
    let _ = run(
        d.path(),
        &r,
        &claims("2024-01-10", "2024-06-01"),
        "2024-06-02T00:00:00Z",
    );
    assert!(verify_unchanged(d.path(), &r).unchanged);
}

fn git(dir: &Path, args: &[&str], date: Option<&str>) -> bool {
    let mut c = std::process::Command::new("git");
    c.args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1");
    if let Some(d) = date {
        c.env("GIT_AUTHOR_DATE", d).env("GIT_COMMITTER_DATE", d);
    }
    c.output().map(|o| o.status.success()).unwrap_or(false)
}

#[test]
fn git_history_rules() {
    if std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("git missing; skipping");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let p = d.path();
    assert!(git(p, &["init", "-q"], None));
    for (i, date) in [
        "2023-12-01T10:00:00+00:00",
        "2024-02-01T10:00:00+00:00",
        "2024-07-01T10:00:00+00:00",
    ]
    .iter()
    .enumerate()
    {
        put(p, "main.rs", format!("fn main(){{ /*{i}*/ }}").as_bytes());
        assert!(git(p, &["add", "."], None));
        assert!(git(
            p,
            &[
                "-c",
                "user.name=A",
                "-c",
                "user.email=a@x",
                "commit",
                "-q",
                "-m",
                &format!("c{i}")
            ],
            Some(date)
        ));
    }
    let h = metadata_forensics::git::read_history(p, 100, std::time::Duration::from_secs(20))
        .unwrap()
        .expect("repo");
    assert_eq!(h.commits.len(), 3);
    let r = scan(p)
        .into_iter()
        .filter(|r| !r.path.contains("/.git/"))
        .collect::<Vec<_>>();
    let a = run_with(
        p,
        &r,
        &claims("2024-01-10", "2024-06-01"),
        "2024-08-01T00:00:00Z",
        Some(&h),
        None,
    );
    assert!(rules(&a).contains(&"GIT-BEFORE-START"), "{:?}", rules(&a));
    assert!(rules(&a).contains(&"GIT-AFTER-DEADLINE"));
    assert!(rules(&a).contains(&"GAP-HISTORY") || rules(&a).contains(&"INC-PRESENT"));
}
