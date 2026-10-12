mod args;
mod config;
mod forensic;
mod render;

use anyhow::{bail, Context, Result};
use args::{Cli, Cmd, Common, Fmt};
use clap::Parser;
use metadata_core::hash::{digest_file, Algorithm};
use metadata_core::FileReport;
use metadata_engine::search::{search, Presence, Query};
use metadata_engine::{collect_files, Engine, Limits, Options, ScanOptions};
use metadata_forensics::{compare, manifest};
use render::Style;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Process exit codes (documented in --help).
const EXIT_FAIL: i32 = 1;
const EXIT_PARTIAL: i32 = 3;

pub(crate) struct Ctx {
    pub(crate) cfg: config::Config,
    style: Style,
    cancel: Arc<AtomicBool>,
}

fn main() {
    let cli = Cli::parse();
    init_tracing(cli.verbose);
    let code = match run(cli) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e:#}");
            EXIT_FAIL
        }
    };
    std::process::exit(code);
}

fn init_tracing(v: u8) {
    let lvl = match v {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    let filter = tracing_subscriber::EnvFilter::try_from_env("METADATA_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(lvl));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .without_time()
        .try_init();
}

fn run(cli: Cli) -> Result<i32> {
    let cfg = config::load()?;
    let color =
        !cli.no_color && std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal();
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let c = cancel.clone();
        let _ = ctrlc::set_handler(move || {
            if c.swap(true, Ordering::SeqCst) {
                std::process::exit(130);
            }
            eprintln!("\ncancelling… (press Ctrl+C again to force quit)");
        });
    }
    let ctx = Ctx {
        cfg,
        style: Style(color),
        cancel,
    };
    match cli.cmd {
        Cmd::Inspect { paths, c } => cmd_extract(&ctx, &paths, &c, Fmt::Human),
        Cmd::Extract { paths, c } => cmd_extract(&ctx, &paths, &c, Fmt::Json),
        Cmd::Batch { paths, c } => cmd_batch(&ctx, &paths, &c),
        Cmd::Search {
            paths,
            field,
            value,
            c,
        } => cmd_search(&ctx, &paths, field, value, &c),
        Cmd::Compare {
            left,
            right,
            show_identical,
            c,
        } => cmd_compare(&ctx, &left, &right, show_identical, &c),
        Cmd::Hash {
            paths,
            algorithm,
            manifest,
            c,
        } => cmd_hash(&ctx, &paths, &algorithm, manifest, &c),
        Cmd::VerifyManifest {
            manifest,
            root,
            check_extra,
            format,
        } => cmd_verify(&ctx, &manifest, root, check_extra, format),
        Cmd::Timeline { paths, source, no_git, manifest, claims, c } => forensic::cmd_timeline(&ctx, &paths, &source, no_git, manifest, claims, &c),
        Cmd::Assess { path, case, student, start, completion, deadline, milestone, claims, previous, no_git, include_personal, min_severity, c } => {
            forensic::cmd_assess(&ctx, forensic::AssessArgs { path, case, student, start, completion, deadline, milestone, claims, previous, no_git, include_personal, min_severity }, &c)
        }
        Cmd::Capabilities { format } => cmd_caps(&ctx, format),
        Cmd::Doctor { format } => cmd_doctor(&ctx, format),
        Cmd::Config { path, init } => cmd_config(&ctx, path, init),
    }
}

// ---------- shared plumbing ----------

pub(crate) fn pick_format(ctx: &Ctx, c: Option<Fmt>, default: Fmt) -> Fmt {
    c.or_else(|| ctx.cfg.default_format.as_deref().and_then(Fmt::parse))
        .unwrap_or(default)
}

fn engine_for(ctx: &Ctx, c: &Common) -> Engine {
    let mut l = Limits::default();
    let lc = &ctx.cfg.limits;
    if let Some(v) = lc.max_archive_entries {
        l.max_archive_entries = v;
    }
    if let Some(v) = lc.max_entry_mib {
        l.max_entry_bytes = v << 20;
    }
    if let Some(v) = lc.max_total_uncompressed_mib {
        l.max_total_uncompressed = v << 20;
    }
    if let Some(v) = lc.max_ratio {
        l.max_ratio = v;
    }
    if let Some(v) = lc.max_depth {
        l.max_depth = v;
    }
    if let Some(v) = lc.max_structural_mib {
        l.max_structural_bytes = v << 20;
    }
    Engine::new(Options {
        all: c.all,
        deep: c.deep,
        hash: !c.no_hash && ctx.cfg.hash.unwrap_or(true),
        use_external: !c.no_external && ctx.cfg.use_external.unwrap_or(true),
        timeout_secs: c.timeout.or(ctx.cfg.timeout_secs).unwrap_or(30),
        limits: l,
        cancel: Some(ctx.cancel.clone()),
    })
}

fn scan_opts(ctx: &Ctx, c: &Common) -> ScanOptions {
    let mut s = ScanOptions {
        recursive: c.recursive,
        ..Default::default()
    };
    if let Some(j) = c.jobs.or(ctx.cfg.jobs) {
        s.jobs = j.max(1);
    }
    s
}

struct Scanned {
    reports: Vec<FileReport>,
    skipped: Vec<(String, String)>,
}

fn scan(ctx: &Ctx, engine: &Engine, paths: &[PathBuf], c: &Common) -> Result<Scanned> {
    let so = scan_opts(ctx, c);
    let col = collect_files(paths, &so);
    let skipped: Vec<(String, String)> = col
        .skipped
        .iter()
        .map(|(p, r)| (p.display().to_string(), r.clone()))
        .collect();
    if col.files.is_empty() && skipped.is_empty() {
        bail!("no files found (directories need --recursive to descend)");
    }
    let show = std::io::stderr().is_terminal() && col.files.len() > 1;
    let pb = if show {
        indicatif::ProgressBar::new(col.files.len() as u64)
    } else {
        indicatif::ProgressBar::hidden()
    };
    pb.set_style(
        indicatif::ProgressStyle::with_template(
            "{spinner} {pos}/{len} files  {elapsed_precise}  {wide_bar:.cyan/blue}",
        )
        .unwrap(),
    );
    let mut reports =
        engine.inspect_many(&col.files, so.jobs, &|done, _| pb.set_position(done as u64));
    pb.finish_and_clear();
    if ctx.cancel.load(Ordering::Relaxed) {
        eprintln!(
            "cancelled: {} of {} files processed",
            reports.len(),
            col.files.len()
        );
    }
    if c.redact || ctx.cfg.redact.unwrap_or(false) {
        reports.iter_mut().for_each(render::redact);
    } else if render::has_sensitive(&reports) && std::io::stderr().is_terminal() {
        eprintln!("note: output contains location/personal identifiers (GPS, author, serials…); use --redact to mask them");
    }
    for (p, r) in &skipped {
        eprintln!("skipped {p}: {r}");
    }
    Ok(Scanned { reports, skipped })
}

fn emit(c: &Common, text: &str) -> Result<()> {
    match &c.output {
        Some(p) => write_output(p, text, c.force),
        None => {
            let mut out = std::io::stdout().lock();
            if let Err(e) = out.write_all(text.as_bytes()) {
                if e.kind() != std::io::ErrorKind::BrokenPipe {
                    return Err(e.into());
                }
            }
            Ok(())
        }
    }
}

fn write_output(p: &Path, text: &str, force: bool) -> Result<()> {
    if p.exists() && !force {
        bail!("{} already exists (use --force to replace)", p.display());
    }
    // Write beside the target then rename so a failure never leaves a half-written report.
    let tmp = p.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, p).with_context(|| format!("replacing {}", p.display()))?;
    eprintln!("wrote {}", p.display());
    Ok(())
}

fn exit_for(reports: &[FileReport], skipped: &[(String, String)]) -> i32 {
    if reports.iter().any(|r| r.error.is_some()) || !skipped.is_empty() {
        EXIT_PARTIAL
    } else {
        0
    }
}

// ---------- commands ----------

fn cmd_extract(ctx: &Ctx, paths: &[PathBuf], c: &Common, default: Fmt) -> Result<i32> {
    let engine = engine_for(ctx, c);
    let s = scan(ctx, &engine, paths, c)?;
    let fmt = pick_format(ctx, c.format, default);
    let style = if c.output.is_some() {
        Style(false)
    } else {
        ctx.style
    };
    let text = match fmt {
        Fmt::Json => render::json_envelope(&s.reports, &s.skipped),
        Fmt::Csv => render::csv_fields(&s.reports),
        Fmt::Human => s
            .reports
            .iter()
            .map(|r| render::human_report(r, style, &c.filter, true))
            .collect::<String>(),
    };
    emit(c, &text)?;
    Ok(exit_for(&s.reports, &s.skipped))
}

fn cmd_batch(ctx: &Ctx, paths: &[PathBuf], c: &Common) -> Result<i32> {
    let mut c = c.clone();
    c.recursive = true;
    let engine = engine_for(ctx, &c);
    let started = std::time::Instant::now();
    let s = scan(ctx, &engine, paths, &c)?;
    let failed: Vec<_> = s.reports.iter().filter(|r| r.error.is_some()).collect();
    let mut by_fmt = std::collections::BTreeMap::<String, usize>::new();
    for r in &s.reports {
        *by_fmt.entry(r.format.id.clone()).or_default() += 1;
    }
    let warnings: usize = s.reports.iter().map(|r| r.warnings.len()).sum();
    let fields: usize = s.reports.iter().map(|r| r.fields.len()).sum();
    let summary = serde_json::json!({
        "files_processed": s.reports.len(), "files_failed": failed.len(), "skipped": s.skipped.len(),
        "fields_extracted": fields, "warnings": warnings, "formats": by_fmt,
        "elapsed_ms": started.elapsed().as_millis() as u64, "cancelled": ctx.cancel.load(Ordering::Relaxed),
    });
    let fmt = pick_format(ctx, c.format, Fmt::Json);
    let text = match fmt {
        Fmt::Json => {
            let v = serde_json::json!({"schema_version": metadata_core::SCHEMA_VERSION, "tool": render::tool(), "summary": summary,
                "files": s.reports, "skipped": s.skipped.iter().map(|(p, r)| serde_json::json!({"path": p, "reason": r})).collect::<Vec<_>>()});
            serde_json::to_string_pretty(&v)? + "\n"
        }
        Fmt::Csv => render::csv_fields(&s.reports),
        Fmt::Human => {
            let mut o = format!(
                "Processed {} files in {} ms · {} failed · {} warnings · {} fields\n",
                s.reports.len(),
                started.elapsed().as_millis(),
                failed.len(),
                warnings,
                fields
            );
            for (f, n) in &by_fmt {
                o.push_str(&format!("  {f:<12}{n}\n"));
            }
            for r in &failed {
                o.push_str(&format!(
                    "  FAILED {}: {}\n",
                    r.path,
                    r.error.as_deref().unwrap_or("")
                ));
            }
            o
        }
    };
    emit(&c, &text)?;
    Ok(exit_for(&s.reports, &s.skipped))
}

fn cmd_search(
    ctx: &Ctx,
    paths: &[PathBuf],
    field: Option<String>,
    value: Option<String>,
    c: &Common,
) -> Result<i32> {
    if field.is_none() && value.is_none() {
        bail!("provide --field and/or --value");
    }
    let mut cc = c.clone();
    cc.no_hash = true;
    // Search must see real values; redaction applies to the printed result instead.
    let redact = c.redact || ctx.cfg.redact.unwrap_or(false);
    cc.redact = false;
    let engine = engine_for(ctx, &cc);
    let mut s = scan(ctx, &engine, paths, &cc)?;
    let q = Query { field, value };
    let mut results: Vec<_> = s.reports.iter().map(|r| search(r, &q)).collect();
    if redact {
        for r in &mut results {
            for m in &mut r.matches {
                if m.sensitive {
                    m.raw = render::REDACTED.into();
                }
            }
        }
    }
    s.reports.clear();
    let fmt = pick_format(ctx, c.format, Fmt::Human);
    let text = match fmt {
        Fmt::Json => {
            serde_json::to_string_pretty(
                &serde_json::json!({"schema_version": metadata_core::SCHEMA_VERSION, "tool": render::tool(), "results": results}),
            )? + "\n"
        }
        Fmt::Csv => {
            let mut o = String::from("path,presence,group,name,value,detail\n");
            for r in &results {
                if r.matches.is_empty() {
                    o.push_str(&format!(
                        "{},{:?},,,,{}\n",
                        r.path,
                        r.presence,
                        r.detail.clone().unwrap_or_default().replace(',', ";")
                    ));
                }
                for m in &r.matches {
                    o.push_str(&format!(
                        "{},found,{},{},\"{}\",\n",
                        r.path,
                        m.group,
                        m.name,
                        m.raw.replace('"', "\"\"")
                    ));
                }
            }
            o
        }
        Fmt::Human => {
            let st = ctx.style;
            let mut o = String::new();
            for r in &results {
                match r.presence {
                    Presence::Found => {
                        o.push_str(&format!("{}\n", st.bold(&r.path)));
                        for m in &r.matches {
                            o.push_str(&format!("  {}:{}  {}\n", m.group, m.name, m.raw));
                        }
                    }
                    other => {
                        let label = match other {
                            Presence::Absent => {
                                st.dim("absent (format supports it, file has none)")
                            }
                            Presence::Unsupported => st.dim("unsupported by this format"),
                            Presence::Unavailable => {
                                st.warn("unknown: needed backend not installed")
                            }
                            Presence::Error => st.err("error"),
                            Presence::Found => unreachable!(),
                        };
                        o.push_str(&format!(
                            "{}  {label}{}\n",
                            r.path,
                            r.detail
                                .as_ref()
                                .map(|d| format!(" — {d}"))
                                .unwrap_or_default()
                        ));
                    }
                }
            }
            let found = results
                .iter()
                .filter(|r| r.presence == Presence::Found)
                .count();
            o.push_str(&format!("\n{found} of {} files matched\n", results.len()));
            o
        }
    };
    emit(c, &text)?;
    Ok(0)
}

fn cmd_compare(
    ctx: &Ctx,
    left: &Path,
    right: &Path,
    show_identical: bool,
    c: &Common,
) -> Result<i32> {
    let engine = engine_for(ctx, c);
    let st = if c.output.is_some() {
        Style(false)
    } else {
        ctx.style
    };
    let (ld, rd) = (left.is_dir(), right.is_dir());
    if ld != rd {
        bail!("cannot compare a file with a directory");
    }
    let comps = if ld {
        let (l, r) = (
            scan(ctx, &engine, &[left.to_path_buf()], c)?,
            scan(ctx, &engine, &[right.to_path_buf()], c)?,
        );
        compare::compare_sets(left, &l.reports, right, &r.reports)
    } else {
        let (a, b) = (engine.inspect(left), engine.inspect(right));
        let mut pairs = vec![compare::compare_reports(&a, &b)];
        if c.redact {
            for p in &mut pairs {
                for f in &mut p.fields {
                    redact_diff(f);
                }
            }
        }
        compare::SetComparison {
            pairs,
            only_left: vec![],
            only_right: vec![],
        }
    };
    let fmt = pick_format(ctx, c.format, Fmt::Human);
    let text = match fmt {
        Fmt::Json | Fmt::Csv => {
            serde_json::to_string_pretty(
                &serde_json::json!({"schema_version": metadata_core::SCHEMA_VERSION, "tool": render::tool(),
            "comparisons": comps.pairs, "only_left": comps.only_left, "only_right": comps.only_right}),
            )? + "\n"
        }
        Fmt::Human => {
            use compare::DiffStatus::*;
            let mut o = String::new();
            for p in &comps.pairs {
                o.push_str(&format!(
                    "{} {} {}\n",
                    st.bold(&p.left),
                    st.dim("↔"),
                    st.bold(&p.right)
                ));
                if let Some(same) = p.content_identical {
                    o.push_str(&format!(
                        "  content: {}\n",
                        if same {
                            st.ok("identical (same SHA-256)")
                        } else {
                            "differs".to_string()
                        }
                    ));
                }
                for f in &p.fields {
                    let line = match f.status {
                        Changed => format!(
                            "  {} {}\n      - {}\n      + {}\n",
                            st.warn("~"),
                            f.key,
                            f.left.as_deref().unwrap_or(""),
                            f.right.as_deref().unwrap_or("")
                        ),
                        Added => format!(
                            "  {} {}  {}\n",
                            st.ok("+"),
                            f.key,
                            f.right.as_deref().unwrap_or("")
                        ),
                        Removed => format!(
                            "  {} {}  {}\n",
                            st.err("-"),
                            f.key,
                            f.left.as_deref().unwrap_or("")
                        ),
                        Identical if show_identical => format!(
                            "  = {}  {}\n",
                            f.key,
                            st.dim(f.left.as_deref().unwrap_or(""))
                        ),
                        _ => continue,
                    };
                    o.push_str(&line);
                }
                o.push_str(&format!(
                    "  {} changed · {} added · {} removed · {} identical\n",
                    p.count(Changed),
                    p.count(Added),
                    p.count(Removed),
                    p.count(Identical)
                ));
                for cv in &p.caveats {
                    o.push_str(&format!("  {} {cv}\n", st.warn("!")));
                }
                o.push('\n');
            }
            for f in &comps.only_left {
                o.push_str(&format!("only in left:  {f}\n"));
            }
            for f in &comps.only_right {
                o.push_str(&format!("only in right: {f}\n"));
            }
            o
        }
    };
    emit(c, &text)?;
    Ok(0)
}

fn redact_diff(f: &mut compare::FieldDiff) {
    let name = f.key.rsplit(':').next().unwrap_or("");
    if metadata_core::is_sensitive_name(name) {
        if f.left.is_some() {
            f.left = Some(render::REDACTED.into());
        }
        if f.right.is_some() {
            f.right = Some(render::REDACTED.into());
        }
    }
}

fn cmd_hash(
    ctx: &Ctx,
    paths: &[PathBuf],
    alg: &str,
    manifest_out: Option<PathBuf>,
    c: &Common,
) -> Result<i32> {
    use rayon::prelude::*;
    let alg = if alg == "sha512" {
        Algorithm::Sha512
    } else {
        Algorithm::Sha256
    };
    let so = scan_opts(ctx, c);
    let col = collect_files(paths, &so);
    for (p, r) in &col.skipped {
        eprintln!("skipped {}: {r}", p.display());
    }
    let root = if paths.len() == 1 && paths[0].is_dir() {
        paths[0].clone()
    } else {
        common_parent(&col.files)
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(so.jobs)
        .build()?;
    let results: Vec<(PathBuf, Result<String, String>)> = pool.install(|| {
        col.files
            .par_iter()
            .map(|p| {
                if ctx.cancel.load(Ordering::Relaxed) {
                    return (p.clone(), Err("cancelled".into()));
                }
                (p.clone(), digest_file(p, alg).map_err(|e| e.to_string()))
            })
            .collect()
    });
    let mut failed = 0;
    let fmt = pick_format(ctx, c.format, Fmt::Human);
    let mut text = String::new();
    match fmt {
        Fmt::Human => {
            for (p, r) in &results {
                match r {
                    Ok(h) => text.push_str(&format!("{h}  {}\n", p.display())),
                    Err(e) => {
                        failed += 1;
                        eprintln!("error: {}: {e}", p.display());
                    }
                }
            }
        }
        Fmt::Csv => {
            text.push_str("path,algorithm,hash,error\n");
            for (p, r) in &results {
                if r.is_err() {
                    failed += 1;
                }
                text.push_str(&format!(
                    "{},{},{},{}\n",
                    p.display(),
                    alg.name(),
                    r.as_ref().cloned().unwrap_or_default(),
                    r.as_ref().err().cloned().unwrap_or_default()
                ));
            }
        }
        Fmt::Json => {
            let rows: Vec<_> = results.iter().map(|(p, r)| { if r.is_err() { failed += 1; } serde_json::json!({"path": p.display().to_string(), "algorithm": alg.name(), "hash": r.as_ref().ok(), "error": r.as_ref().err()}) }).collect();
            text = serde_json::to_string_pretty(
                &serde_json::json!({"schema_version": metadata_core::SCHEMA_VERSION, "tool": render::tool(), "results": rows}),
            )? + "\n";
        }
    }
    emit(c, &text)?;
    if let Some(mp) = manifest_out {
        if failed > 0 {
            bail!("{failed} file(s) could not be hashed; manifest not written");
        }
        let m = manifest::create(&root, &col.files, alg)?;
        write_output(&mp, &(serde_json::to_string_pretty(&m)? + "\n"), c.force)?;
    }
    Ok(if failed > 0 { EXIT_PARTIAL } else { 0 })
}

fn common_parent(files: &[PathBuf]) -> PathBuf {
    let mut it = files
        .iter()
        .map(|f| f.parent().map(Path::to_path_buf).unwrap_or_default());
    let Some(mut acc) = it.next() else {
        return PathBuf::from(".");
    };
    for p in it {
        while !p.starts_with(&acc) {
            if !acc.pop() {
                break;
            }
        }
    }
    acc
}

fn cmd_verify(
    ctx: &Ctx,
    mpath: &Path,
    root: Option<PathBuf>,
    check_extra: bool,
    format: Option<Fmt>,
) -> Result<i32> {
    let m = manifest::load(mpath)?;
    let root = root.unwrap_or_else(|| PathBuf::from(&m.root));
    let extra = if check_extra {
        Some(
            collect_files(
                std::slice::from_ref(&root),
                &ScanOptions {
                    recursive: true,
                    ..Default::default()
                },
            )
            .files,
        )
    } else {
        None
    };
    let rep = manifest::verify(&m, &root, extra.as_deref());
    let st = ctx.style;
    match pick_format(ctx, format, Fmt::Human) {
        Fmt::Json | Fmt::Csv => println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({"schema_version": metadata_core::SCHEMA_VERSION, "tool": render::tool(), "ok": rep.all_ok(), "report": rep, "limitations": manifest::LIMITATIONS})
            )?
        ),
        Fmt::Human => {
            use manifest::EntryStatus::*;
            println!(
                "manifest digest: {}",
                if rep.manifest_digest_ok {
                    st.ok("ok")
                } else {
                    st.err("MISMATCH (manifest entries were altered)")
                }
            );
            for r in &rep.results {
                let tag = match r.status {
                    Ok => st.ok("ok      "),
                    Modified => st.err("MODIFIED"),
                    Missing => st.err("MISSING "),
                    Unreadable => st.err("UNREAD  "),
                };
                println!("  {tag} {}", r.path);
            }
            for e in &rep.extra {
                println!("  {} {e}", st.warn("EXTRA   "));
            }
            println!(
                "\n{}\n{}",
                if rep.all_ok() {
                    st.ok("verification passed")
                } else {
                    st.err("verification FAILED")
                },
                st.dim(manifest::LIMITATIONS)
            );
        }
    }
    Ok(if rep.all_ok() { 0 } else { EXIT_FAIL })
}

fn cmd_caps(ctx: &Ctx, format: Option<Fmt>) -> Result<i32> {
    let b = metadata_engine::Backends::discover();
    let m = metadata_engine::capabilities::matrix(&b);
    match pick_format(ctx, format, Fmt::Human) {
        Fmt::Json | Fmt::Csv => println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({"backends": b.status(), "formats": m})
            )?
        ),
        Fmt::Human => {
            let st = ctx.style;
            println!("{}", st.bold("Backends"));
            for s in b.status() {
                let state = if s.available {
                    st.ok("available")
                } else {
                    st.warn("missing  ")
                };
                println!(
                    "  {:<10}{state}  {}",
                    s.name,
                    st.dim(s.version.as_deref().unwrap_or(""))
                );
            }
            println!("\n{}", st.bold("Formats"));
            println!("  {:<38}{:<22}{:<12}NOTES", "FORMAT", "READ", "WRITE");
            for f in m {
                let read = if f.read.is_empty() {
                    "detect only".to_string()
                } else {
                    f.read.join("+")
                };
                let write = if f.write.is_empty() {
                    "—".to_string()
                } else {
                    f.write.join("+")
                };
                println!(
                    "  {:<38}{:<22}{:<12}{}",
                    f.format,
                    read,
                    write,
                    st.dim(f.notes)
                );
            }
            println!("\n{}", st.dim("Write support is listed only for format/backend pairs verified by round-trip tests (none yet: metadata editing is PLANNED)."));
        }
    }
    Ok(0)
}

fn cmd_doctor(ctx: &Ctx, format: Option<Fmt>) -> Result<i32> {
    let b = metadata_engine::Backends::discover();
    let st = ctx.style;
    let cfgp = config::config_path();
    let cfg_state = if cfgp.exists() {
        match config::load() {
            Ok(_) => "present and valid".to_string(),
            Err(e) => format!("INVALID: {e}"),
        }
    } else {
        "not created (defaults in use)".into()
    };
    let tmp_ok = tempdir_writable();
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    match pick_format(ctx, format, Fmt::Human) {
        Fmt::Json | Fmt::Csv => println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"), "backends": b.status(), "config_path": cfgp.display().to_string(),
            "config": cfg_state, "temp_writable": tmp_ok, "cpus": threads, "os": std::env::consts::OS }))?
        ),
        Fmt::Human => {
            println!(
                "{} {}  ({} {})",
                st.bold("metadata"),
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH
            );
            println!("\n{}", st.bold("Backends"));
            for s in b.status() {
                if s.available {
                    println!(
                        "  {} {:<10}{}  {}",
                        st.ok("✓"),
                        s.name,
                        s.version.unwrap_or_default(),
                        st.dim(&s.path.unwrap_or_default())
                    );
                } else {
                    println!(
                        "  {} {:<10}not found\n      {}\n      install: {}",
                        st.warn("!"),
                        s.name,
                        st.dim(&s.purpose),
                        s.install_hint
                    );
                }
            }
            println!("\n{}", st.bold("Environment"));
            println!(
                "  config     {}  {}",
                cfgp.display(),
                st.dim(&format!("({cfg_state})"))
            );
            println!(
                "  temp dir   {}",
                if tmp_ok {
                    st.ok("writable")
                } else {
                    st.err("NOT writable")
                }
            );
            println!("  cpus       {threads}");
            println!(
                "\n{}\n  Apache Tika and 7-Zip adapters: PLANNED (not integrated).",
                st.bold("Not integrated")
            );
        }
    }
    Ok(if tmp_ok { 0 } else { EXIT_FAIL })
}

fn tempdir_writable() -> bool {
    let p = std::env::temp_dir().join(format!("metadata-doctor-{}", std::process::id()));
    let ok = std::fs::write(&p, b"x").is_ok();
    let _ = std::fs::remove_file(&p);
    ok
}

fn cmd_config(ctx: &Ctx, path_only: bool, init: bool) -> Result<i32> {
    let p = config::config_path();
    if path_only {
        println!("{}", p.display());
        return Ok(0);
    }
    if init {
        if p.exists() {
            bail!("{} already exists", p.display());
        }
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(&p, config::TEMPLATE)?;
        println!("created {}", p.display());
        return Ok(0);
    }
    println!(
        "# {}{}",
        p.display(),
        if p.exists() {
            ""
        } else {
            " (does not exist; run `metadata config --init`)"
        }
    );
    println!("{}", toml::to_string_pretty(&ctx.cfg)?);
    Ok(0)
}
