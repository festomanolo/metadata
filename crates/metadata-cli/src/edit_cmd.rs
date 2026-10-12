//! `edit` and `clean` handlers: plan, preview, confirm, write, verify, report.
use crate::args::{Common, Fmt, WriteOpts};
use crate::{engine_for, pick_format, scan_opts, Ctx};
use anyhow::{bail, Result};
use metadata_edit::{execute, plan, ApplyOpts, Backup, EditRequest, Op, Profile};
use metadata_engine::collect_files;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct EditArgs {
    pub set: Vec<String>,
    pub remove: Vec<String>,
    pub copy_from: Option<PathBuf>,
    pub interactive: bool,
    pub profile: Option<String>,
}

fn parse_set(s: &str) -> Result<Op> {
    let (k, v) = s
        .split_once('=')
        .ok_or_else(|| anyhow::anyhow!("--set expects KEY=VALUE, got '{s}'"))?;
    if k.trim().is_empty() {
        bail!("--set has an empty field name");
    }
    Ok(Op::Set(k.trim().to_string(), v.to_string()))
}

fn dest_for(src: &Path, w: &WriteOpts, suffix: &str, single: bool) -> PathBuf {
    if w.in_place {
        return src.to_path_buf();
    }
    let name = src
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = src
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let file = format!("{name}.{suffix}{ext}");
    let _ = single;
    match &w.output_dir {
        Some(d) => d.join(src.file_name().unwrap_or_default()),
        None => src.with_file_name(file),
    }
}

fn confirm(prompt: &str) -> bool {
    if !std::io::stdin().is_terminal() {
        return false;
    }
    eprint!("{prompt} [y/N] ");
    let _ = std::io::stderr().flush();
    let mut l = String::new();
    std::io::stdin().lock().read_line(&mut l).is_ok()
        && matches!(l.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn interactive_ops(engine: &metadata_engine::Engine, file: &Path) -> Result<Vec<Op>> {
    let r = engine.inspect(file);
    let keys = metadata_edit::native::keys(&r.format.id);
    eprintln!(
        "Interactive edit of {} ({}). Writable: {}",
        file.display(),
        r.format.id,
        if keys.is_empty() {
            "native writer unavailable; any Group:Name works with exiftool".into()
        } else {
            keys.join(", ")
        }
    );
    eprintln!("Commands: set KEY VALUE | rm KEY | show | done | quit");
    let mut ops = vec![];
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = line?;
        let t = line.trim();
        match t
            .split_once(' ')
            .map(|(a, b)| (a, b.trim()))
            .unwrap_or((t, ""))
        {
            ("done" | "apply" | "w", _) => return Ok(ops),
            ("quit" | "q", _) => bail!("cancelled; nothing written"),
            ("show", _) => {
                for k in &keys {
                    eprintln!(
                        "  {k} = {}",
                        r.find(k)
                            .first()
                            .map(|f| f.raw.as_str())
                            .unwrap_or("(unset)")
                    );
                }
                for o in &ops {
                    eprintln!("  pending: {o:?}");
                }
            }
            ("set", rest) => match rest.split_once(' ') {
                Some((k, v)) => ops.push(Op::Set(k.to_string(), v.to_string())),
                None => eprintln!("usage: set KEY VALUE"),
            },
            ("rm", k) if !k.is_empty() => ops.push(Op::Remove(k.to_string())),
            ("", _) => {}
            _ => eprintln!("unknown command"),
        }
    }
    bail!("input ended before 'done'; nothing written")
}

pub fn cmd_edit(
    ctx: &Ctx,
    paths: &[PathBuf],
    a: EditArgs,
    w: &WriteOpts,
    c: &Common,
) -> Result<i32> {
    let profile = match a.profile.as_deref() {
        Some("privacy") => Some(Profile::Privacy),
        Some("all") => Some(Profile::All),
        Some("gps") => Some(Profile::Gps),
        _ => None,
    };
    let mut base_ops = vec![];
    for s in &a.set {
        base_ops.push(parse_set(s)?);
    }
    for r in &a.remove {
        base_ops.push(Op::Remove(r.clone()));
    }
    let mut cc = c.clone();
    cc.no_hash = false;
    let engine = engine_for(ctx, &cc);
    let col = collect_files(paths, &scan_opts(ctx, c));
    for (p, r) in &col.skipped {
        eprintln!("skipped {}: {r}", p.display());
    }
    if col.files.is_empty() {
        bail!("no files to process (directories need --recursive)");
    }
    if a.interactive {
        if col.files.len() != 1 {
            bail!("--interactive works on exactly one file");
        }
        base_ops.extend(interactive_ops(&engine, &col.files[0])?);
    }
    let copy_src = a.copy_from.as_ref().map(|p| engine.inspect(p));
    if let Some(r) = &copy_src {
        if let Some(e) = &r.error {
            bail!("--copy-from: {e}");
        }
    }
    if base_ops.is_empty() && profile.is_none() && copy_src.is_none() {
        bail!("nothing to do: use --set, --remove, --copy-from, --interactive or `clean`");
    }
    let many = col.files.len() > 1;
    if many && !w.in_place && w.output_dir.is_none() {
        // default <name>.edited.<ext> is fine for several files too
    }
    if w.in_place && w.output_dir.is_some() {
        bail!("--in-place and --output-dir are mutually exclusive");
    }
    if let Some(d) = &w.output_dir {
        if !w.dry_run {
            std::fs::create_dir_all(d)?;
        }
    }
    let suffix = if profile.is_some() && base_ops.is_empty() {
        "cleaned"
    } else {
        "edited"
    };
    let fmt = pick_format(ctx, c.format, Fmt::Human);
    let human = fmt == Fmt::Human;
    let log_path = crate::config::config_path()
        .parent()
        .map(|d| d.join("operations.jsonl"));
    let st = ctx.style;
    let mut results = vec![];
    let mut failed = 0;
    let mut approved = w.yes || w.dry_run;
    for src in &col.files {
        let mut ops = base_ops.clone();
        if let Some(cs) = &copy_src {
            let dest_fmt = engine.inspect(src).format.id;
            let mut skipped = 0;
            for f in cs
                .fields
                .iter()
                .filter(|f| f.provenance == metadata_core::Provenance::Stored && !f.raw.is_empty())
            {
                let dateish = f.time.is_some();
                match metadata_edit::native::resolve_key(&dest_fmt, &f.key()) {
                    Ok(k)
                        if !dateish && !k.starts_with("PNG:")
                            || (k.starts_with("PNG:") && f.group == "PNG" && !dateish) =>
                    {
                        ops.push(Op::Set(k, f.raw.clone()))
                    }
                    Ok(_) => skipped += 1,
                    Err(_) => {}
                }
            }
            if skipped > 0 && human {
                eprintln!(
                    "note: {skipped} date field(s) not copied (dates are never copied implicitly)"
                );
            }
        }
        let req = EditRequest { ops, profile };
        let outcome = (|| -> Result<_> {
            let p = plan(&engine, src, &req)?;
            if human {
                print_plan(&p, st);
            }
            if w.dry_run {
                return Ok((p, None));
            }
            // Writing new copies is non-destructive; only replacing originals needs confirmation.
            if w.in_place && !approved {
                if confirm(&format!(
                    "About to modify {} original file(s) in place. Continue?",
                    col.files.len()
                )) {
                    approved = true;
                } else {
                    bail!("confirmation required for --in-place: pass --yes (not a terminal or declined)");
                }
            }
            let dest = dest_for(src, w, suffix, !many);
            let opts = ApplyOpts {
                backup: if w.no_backup {
                    Backup::None
                } else {
                    Backup::Copy
                },
                replace_original: w.in_place,
                allow_signature_loss: w.accept_signature_loss,
                force: c.force,
                log_path: log_path.clone(),
                timeout: std::time::Duration::from_secs(c.timeout.unwrap_or(60)),
            };
            let o = execute(&engine, src, &dest, &p, &opts)?;
            Ok((p, Some(o)))
        })();
        match outcome {
            Ok((p, o)) => {
                if human {
                    match &o {
                        Some(o) => {
                            println!(
                                "  {} wrote {} (verified, sha256 {})",
                                st.ok("✓"),
                                o.destination,
                                &o.destination_sha256[..16]
                            );
                            if let Some(b) = &o.backup {
                                println!("    backup: {b}");
                            }
                            for u in &o.unexpected_changes {
                                println!("    {} unexpected: {u}", st.warn("!"));
                            }
                        }
                        None => println!("  {}", st.dim("dry run: nothing written")),
                    }
                    println!();
                }
                results.push(serde_json::json!({"file": src.display().to_string(), "status": if o.is_some() {"written"} else {"planned"}, "plan": p, "outcome": o}));
            }
            Err(e) => {
                failed += 1;
                if human {
                    eprintln!("{} {}: {e}", st.err("error"), src.display());
                }
                results.push(serde_json::json!({"file": src.display().to_string(), "status": "failed", "error": e.to_string()}));
            }
        }
    }
    if !human {
        let v = serde_json::json!({"schema_version": metadata_core::SCHEMA_VERSION, "tool": crate::render::tool(), "dry_run": w.dry_run, "results": results});
        crate::emit(c, &(serde_json::to_string_pretty(&v)? + "\n"))?;
    }
    Ok(if failed == 0 {
        0
    } else if failed == col.files.len() && col.files.len() == 1 {
        1
    } else {
        3
    })
}

fn print_plan(p: &metadata_edit::Plan, st: crate::render::Style) {
    println!(
        "{}  {}",
        st.bold(&p.source),
        st.dim(&format!("({} · {})", p.format, p.backend))
    );
    for c in &p.changes {
        let before = c
            .before
            .as_deref()
            .map(|b| format!("{b:?}"))
            .unwrap_or_else(|| "(unset)".into());
        let after = c
            .after
            .as_deref()
            .map(|a| format!("{a:?}"))
            .unwrap_or_else(|| "(removed)".into());
        let mark = if c.action == "set" {
            st.warn("~")
        } else {
            st.err("-")
        };
        println!(
            "  {mark} {:<28} {before} → {after}{}",
            c.key,
            c.note
                .as_ref()
                .map(|n| st.dim(&format!("  [{n}]")))
                .unwrap_or_default()
        );
    }
    for w in &p.warnings {
        println!("  {} {w}", st.warn("!"));
    }
    if let Some(n) = &p.residual_note {
        println!("  {}", st.dim(&format!("note: {n}")));
    }
}
