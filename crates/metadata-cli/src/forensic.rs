//! `timeline` and `assess` command handlers.
use crate::args::{Common, Fmt};
use crate::render::Style;
use crate::{emit, engine_for, pick_format, scan_opts, Ctx};
use anyhow::{bail, Context, Result};
use metadata_core::{time, FileReport};
use metadata_engine::collect_files;
use metadata_forensics::assess::{self, Severity};
use metadata_forensics::claims::{parse_claim, Claims, Milestone, Thresholds};
use metadata_forensics::timeline::{self, Source};
use metadata_forensics::{git, manifest, project};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

const BUILD_DIRS: [&str; 5] = ["node_modules", "target", "__pycache__", ".venv", "venv"];

#[derive(Deserialize, Default)]
#[serde(default)]
struct ClaimsFile {
    #[serde(flatten)]
    claims: Claims,
    thresholds: Option<Thresholds>,
}

fn load_claims(p: &Path) -> Result<ClaimsFile> {
    let s = std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
    let f: ClaimsFile = if p.extension().is_some_and(|e| e == "json") {
        serde_json::from_str(&s)?
    } else {
        toml::from_str(&s)?
    };
    f.claims.validate()?;
    Ok(f)
}

fn parse_milestone(s: &str) -> Result<Milestone> {
    let (name, rest) = s
        .split_once('=')
        .context("milestone must look like Name=YYYY-MM-DD[:path-substring]")?;
    let (date, filt) = match rest.split_once(':') {
        // a colon inside an RFC 3339 time must not be mistaken for the path separator
        Some((d, f))
            if parse_claim(d).is_ok() && !f.chars().next().is_some_and(|c| c.is_ascii_digit()) =>
        {
            (d, Some(f.to_string()))
        }
        _ => (rest, None),
    };
    parse_claim(date)?;
    Ok(Milestone {
        name: name.trim().into(),
        date: date.trim().into(),
        path_contains: filt,
    })
}

fn root_of(p: &Path) -> PathBuf {
    if p.is_dir() {
        p.to_path_buf()
    } else {
        p.parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

fn scan_project(ctx: &Ctx, c: &Common, root: &Path) -> Result<(Vec<FileReport>, Vec<PathBuf>)> {
    let mut cc = c.clone();
    cc.recursive = true;
    let mut so = scan_opts(ctx, &cc);
    if !c.include_vcs {
        so.exclude_dirs
            .extend(BUILD_DIRS.iter().map(|s| s.to_string()));
    }
    let col = collect_files(&[root.to_path_buf()], &so);
    for (p, r) in &col.skipped {
        eprintln!("skipped {}: {r}", p.display());
    }
    let engine = engine_for(ctx, &cc);
    let reports = if c.cache {
        crate::cache::inspect_many_cached(&engine, &col.files, so.jobs, &|_, _| {})
    } else {
        engine.inspect_many(&col.files, so.jobs, &|_, _| {})
    };
    Ok((reports, col.files))
}

fn read_git(root: &Path, no_git: bool, th: &Thresholds) -> Option<git::GitHistory> {
    if no_git {
        return None;
    }
    match git::read_history(root, th.max_commits, Duration::from_secs(60)) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("warning: git history unavailable: {e}");
            None
        }
    }
}

pub fn cmd_timeline(
    ctx: &Ctx,
    paths: &[PathBuf],
    sources: &[String],
    no_git: bool,
    manifest_path: Option<PathBuf>,
    claims: Option<PathBuf>,
    c: &Common,
) -> Result<i32> {
    let root = if paths.len() == 1 {
        root_of(&paths[0])
    } else {
        PathBuf::from(".")
    };
    let (reports, _) = if paths.len() == 1 && paths[0].is_dir() {
        scan_project(ctx, c, &root)?
    } else {
        let engine = engine_for(ctx, c);
        let col = collect_files(paths, &scan_opts(ctx, c));
        (engine.inspect_many(&col.files, 4, &|_, _| {}), col.files)
    };
    let cf = claims.as_deref().map(load_claims).transpose()?;
    let th = cf
        .as_ref()
        .and_then(|f| f.thresholds.clone())
        .unwrap_or_default();
    let g = if paths.len() == 1 && paths[0].is_dir() {
        read_git(&root, no_git, &th)
    } else {
        None
    };
    let intake = manifest_path
        .as_deref()
        .map(manifest::load)
        .transpose()?
        .and_then(|m| time::parse_iso(&m.recorded_at));
    let mut ev = timeline::build(
        &root,
        &reports,
        g.as_ref(),
        intake.as_ref().map(|t| ("manifest recorded_at", t)),
        cf.as_ref().map(|f| &f.claims),
    );
    if !sources.is_empty() {
        let want: Vec<String> = sources.iter().map(|s| s.to_ascii_lowercase()).collect();
        for w in &want {
            if ![
                "declared",
                "embedded",
                "archive",
                "filesystem",
                "git",
                "intake",
            ]
            .contains(&w.as_str())
            {
                bail!("unknown source '{w}'");
            }
        }
        ev.retain(|e| want.iter().any(|w| w == e.source.label()));
    }
    let redact = c.redact || ctx.cfg.redact.unwrap_or(false);
    if redact {
        for e in &mut ev {
            if metadata_core::is_sensitive_name(e.field.rsplit(':').next().unwrap_or("")) {
                e.value = crate::render::REDACTED.into();
            }
        }
    }
    let text = match pick_format(ctx, c.format, Fmt::Human) {
        Fmt::Json => {
            let rel: serde_json::Map<String, serde_json::Value> = [
                Source::Declared,
                Source::Embedded,
                Source::Archive,
                Source::Filesystem,
                Source::Git,
                Source::Intake,
            ]
            .iter()
            .map(|s| (s.label().to_string(), s.reliability().into()))
            .collect();
            serde_json::to_string_pretty(
                &serde_json::json!({"schema_version": metadata_core::SCHEMA_VERSION, "tool": crate::render::tool(), "source_reliability": rel, "events": ev}),
            )? + "\n"
        }
        Fmt::Csv => {
            let mut o = String::from("time,time_utc,offset_known,source,role,file,field,value\n");
            for e in &ev {
                o.push_str(&format!(
                    "{},{},{},{},{:?},{},{},{}\n",
                    e.time.iso,
                    e.time.utc.clone().unwrap_or_default(),
                    e.time.offset_known,
                    e.source.label(),
                    e.role,
                    csv(&e.file),
                    csv(&e.field),
                    csv(&e.value)
                ));
            }
            o
        }
        Fmt::Human => {
            let st = if c.output.is_some() {
                Style(false)
            } else {
                ctx.style
            };
            let mut o = String::new();
            o.push_str(&st.dim("Sources are shown separately; none is independent proof of when work happened. `?` = no timezone in source.\n"));
            for s in [
                Source::Declared,
                Source::Embedded,
                Source::Archive,
                Source::Filesystem,
                Source::Git,
                Source::Intake,
            ] {
                if ev.iter().any(|e| e.source == s) {
                    o.push_str(&st.dim(&format!("  {:<11} {}\n", s.label(), s.reliability())));
                }
            }
            o.push('\n');
            for e in &ev {
                let mark = if e.time.offset_known { " " } else { "?" };
                o.push_str(&format!(
                    "{}{mark} {} {:<28} {}  {}\n",
                    e.time.iso,
                    st.accent(&format!("{:<10}", e.source.label())),
                    e.file,
                    st.dim(&e.field),
                    e.value.chars().take(60).collect::<String>()
                ));
            }
            o.push_str(&format!("\n{} events\n", ev.len()));
            o
        }
    };
    emit(c, &text)?;
    Ok(0)
}

fn csv(s: &str) -> String {
    let g = if s.starts_with(['=', '+', '-', '@']) {
        format!("'{s}")
    } else {
        s.to_string()
    };
    if g.contains([',', '"', '\n']) {
        format!("\"{}\"", g.replace('"', "\"\""))
    } else {
        g
    }
}

pub struct AssessArgs {
    pub html: Option<PathBuf>,
    pub path: PathBuf,
    pub case: String,
    pub student: Option<String>,
    pub start: Option<String>,
    pub completion: Option<String>,
    pub deadline: Option<String>,
    pub milestone: Vec<String>,
    pub claims: Option<PathBuf>,
    pub previous: Option<PathBuf>,
    pub no_git: bool,
    pub include_personal: bool,
    pub min_severity: String,
}

pub fn cmd_assess(ctx: &Ctx, a: AssessArgs, c: &Common) -> Result<i32> {
    if !a.path.exists() {
        bail!("{} does not exist", a.path.display());
    }
    let mut cf = a
        .claims
        .as_deref()
        .map(load_claims)
        .transpose()?
        .unwrap_or_default();
    if a.student.is_some() {
        cf.claims.student = a.student.clone();
    }
    if a.start.is_some() {
        cf.claims.start = a.start.clone();
    }
    if a.completion.is_some() {
        cf.claims.completion = a.completion.clone();
    }
    if a.deadline.is_some() {
        cf.claims.deadline = a.deadline.clone();
    }
    for m in &a.milestone {
        cf.claims.milestones.push(parse_milestone(m)?);
    }
    cf.claims.validate()?;
    let th = cf.thresholds.clone().unwrap_or_default();
    let prev = match &a.previous {
        Some(p) => Some((p.clone(), scan_project(ctx, c, p)?.0)),
        None => None,
    };
    let done = run_assessment(
        ctx,
        c,
        &a.case,
        &a.path,
        &cf.claims,
        &th,
        prev,
        a.no_git,
        a.include_personal,
    )?;
    let mut result = done.assessment;
    if let Some(h) = &a.html {
        let html = metadata_report::html(&metadata_report::ReportInput {
            assessment: &result,
            events: &done.events,
            states: &Default::default(),
            notes: &[],
            title: None,
            include_timeline: true,
        });
        crate::write_output(h, &html, c.force)?;
    }
    let min = match a.min_severity.as_str() {
        "high" => Severity::HighPriority,
        "review" => Severity::Review,
        _ => Severity::Info,
    };
    result.findings.retain(|f| f.severity >= min);
    let text = match pick_format(ctx, c.format, Fmt::Human) {
        Fmt::Json | Fmt::Csv => serde_json::to_string_pretty(&result)? + "\n",
        Fmt::Human => human_assessment(
            &result,
            if c.output.is_some() {
                Style(false)
            } else {
                ctx.style
            },
        ),
    };
    emit(c, &text)?;
    Ok(if result.integrity.as_ref().is_some_and(|i| !i.unchanged) {
        1
    } else {
        0
    })
}

fn human_assessment(a: &assess::Assessment, st: Style) -> String {
    let mut o = String::new();
    let n = |s: Severity| a.findings.iter().filter(|f| f.severity == s).count();
    o.push_str(&format!(
        "{}  case {}  {}\n",
        st.bold("ASSESSMENT"),
        a.case_id,
        st.dim(&a.root)
    ));
    o.push_str(&format!(
        "{} files · {} HIGH PRIORITY · {} REVIEW · {} INFO\n",
        a.inventory.len(),
        n(Severity::HighPriority),
        n(Severity::Review),
        n(Severity::Info)
    ));
    let cl = &a.claims;
    o.push_str(&st.dim(&format!(
        "claims: start {} · completion {} · deadline {} · {} milestone(s)\n",
        cl.start.as_deref().unwrap_or("-"),
        cl.completion.as_deref().unwrap_or("-"),
        cl.deadline.as_deref().unwrap_or("-"),
        cl.milestones.len()
    )));
    if let Some(i) = &a.integrity {
        o.push_str(&format!(
            "integrity: {} ({} files re-hashed)\n",
            if i.unchanged {
                st.ok("inputs unchanged by analysis")
            } else {
                st.err("INPUTS CHANGED DURING ANALYSIS")
            },
            i.files_rehashed
        ));
    }
    o.push('\n');
    for f in &a.findings {
        let tag = match f.severity {
            Severity::HighPriority => st.err("[HIGH PRIORITY]"),
            Severity::Review => st.warn("[REVIEW]"),
            Severity::Info => st.dim("[INFO]"),
        };
        o.push_str(&format!(
            "{tag} {}  {}  {}\n",
            st.bold(&f.id),
            f.title,
            st.dim(&format!("(priority {})", f.review_priority))
        ));
        o.push_str("  evidence:\n");
        for e in &f.evidence {
            o.push_str(&format!(
                "    {}  {} = {}  {}\n",
                e.file,
                e.field,
                e.value,
                st.dim(&format!("[{}]", e.source.label()))
            ));
        }
        if f.evidence_omitted > 0 {
            o.push_str(&format!(
                "    … {} more in JSON output\n",
                f.evidence_omitted
            ));
        }
        o.push_str(&format!(
            "  rule: {}\n  why: {}\n  could also be:\n",
            f.comparison_rule, f.explanation
        ));
        for b in &f.benign_explanations {
            o.push_str(&format!("    - {b}\n"));
        }
        o.push_str(&format!(
            "  confidence: observation {:?} · interpretation {:?}\n  follow-up: {}\n\n",
            f.observation_confidence, f.interpretation_confidence, f.follow_up
        ));
    }
    if a.findings.is_empty() {
        o.push_str("No findings at or above the selected level.\n\n");
    }
    o.push_str(&st.bold("Limitations\n"));
    for l in &a.limitations {
        o.push_str(&format!("  - {l}\n"));
    }
    for l in &a.checks_not_implemented {
        o.push_str(&format!("  - not implemented: {l}\n"));
    }
    o.push_str(&format!("\n{}\n", st.dim(&a.disclaimer)));
    o
}

pub struct Assessed {
    pub assessment: assess::Assessment,
    pub reports: Vec<FileReport>,
    pub events: Vec<timeline::Event>,
}

/// Scan a project folder and produce the full assessment (shared by `assess` and `case add`).
#[allow(clippy::too_many_arguments)]
pub fn run_assessment(
    ctx: &Ctx,
    c: &Common,
    case: &str,
    path: &Path,
    claims: &Claims,
    th: &Thresholds,
    previous: Option<(PathBuf, Vec<FileReport>)>,
    no_git: bool,
    include_personal: bool,
) -> Result<Assessed> {
    if !path.exists() {
        bail!("{} does not exist", path.display());
    }
    let root = root_of(path);
    let (reports, files) = scan_project(ctx, c, &root)?;
    if ctx.cancel.load(Ordering::Relaxed) {
        bail!("cancelled");
    }
    let g = read_git(&root, no_git, th);
    let now = time::TimeValue::from_system(std::time::SystemTime::now());
    let mut assessment = assess::assess(&assess::Input {
        case_id: case,
        root: &root,
        reports: &reports,
        claims,
        thresholds: th,
        git: g.as_ref(),
        previous: previous.as_ref().map(|(p, r)| (p.as_path(), r.as_slice())),
        project: project::summarize(&root, &files),
        now,
    });
    assessment.integrity = Some(assess::verify_unchanged(&root, &reports));
    if !include_personal {
        assess::redact(&mut assessment);
    }
    let mut events = timeline::build(&root, &reports, g.as_ref(), None, Some(claims));
    if !include_personal {
        for e in &mut events {
            if metadata_core::is_sensitive_name(e.field.rsplit(':').next().unwrap_or("")) {
                e.value = crate::render::REDACTED.into();
            }
        }
    }
    Ok(Assessed {
        assessment,
        reports,
        events,
    })
}

pub fn build_claims(
    c: Option<&Path>,
    student: &Option<String>,
    start: &Option<String>,
    completion: &Option<String>,
    deadline: &Option<String>,
    milestones: &[String],
) -> Result<(Claims, Thresholds)> {
    let mut cf = c.map(load_claims).transpose()?.unwrap_or_default();
    if student.is_some() {
        cf.claims.student = student.clone();
    }
    if start.is_some() {
        cf.claims.start = start.clone();
    }
    if completion.is_some() {
        cf.claims.completion = completion.clone();
    }
    if deadline.is_some() {
        cf.claims.deadline = deadline.clone();
    }
    for m in milestones {
        cf.claims.milestones.push(parse_milestone(m)?);
    }
    cf.claims.validate()?;
    Ok((cf.claims, cf.thresholds.unwrap_or_default()))
}
