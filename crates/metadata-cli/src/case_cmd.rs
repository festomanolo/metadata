//! `case` command group.
use crate::args::{CaseCmd, Fmt};
use crate::forensic::{build_claims, run_assessment};
use crate::{config, pick_format, Ctx};
use anyhow::{bail, Context, Result};
use metadata_forensics::assess::FindingStatus;
use metadata_forensics::compare;
use metadata_storage::Store;
use std::path::PathBuf;

fn open(db: Option<PathBuf>) -> Result<Store> {
    let p = db.unwrap_or_else(|| config::config_path().with_file_name("cases.sqlite"));
    Store::open(&p).with_context(|| format!("opening case database {}", p.display()))
}

fn require_case(s: &Store, id: &str) -> Result<metadata_storage::CaseRow> {
    s.get_case(id)?
        .with_context(|| format!("no such case '{id}' (see `metadata case list`)"))
}

pub fn cmd_case(ctx: &Ctx, db: Option<PathBuf>, cmd: CaseCmd) -> Result<i32> {
    let store = open(db)?;
    let st = ctx.style;
    match cmd {
        CaseCmd::New {
            id,
            title,
            student,
            start,
            completion,
            deadline,
            milestone,
            claims,
        } => {
            let (c, _) = build_claims(
                claims.as_deref(),
                &student,
                &start,
                &completion,
                &deadline,
                &milestone,
            )?;
            store.create_case(&id, &title, &c)?;
            println!("created case {id}");
        }
        CaseCmd::List { search, format } => {
            let cases = store.list_cases(search.as_deref())?;
            match pick_format(ctx, format, Fmt::Human) {
                Fmt::Json | Fmt::Csv => println!("{}", serde_json::to_string_pretty(&cases)?),
                Fmt::Human => {
                    if cases.is_empty() {
                        println!("no cases yet; create one with `metadata case new ID`");
                    }
                    for c in cases {
                        println!(
                            "{:<20} {:<32} {} submission(s)  {}",
                            st.bold(&c.id),
                            c.title,
                            c.submissions,
                            st.dim(&c.created_at)
                        );
                    }
                }
            }
        }
        CaseCmd::Show { id, format } => {
            let c = require_case(&store, &id)?;
            let subs = store.submissions(&id)?;
            let states = store.finding_states(&id)?;
            let notes = store.notes(&id)?;
            match pick_format(ctx, format, Fmt::Human) {
                Fmt::Json | Fmt::Csv => println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"case": c, "submissions": subs, "finding_states": states, "notes": notes})
                    )?
                ),
                Fmt::Human => {
                    println!("{} {}  {}", st.bold(&c.id), c.title, st.dim(&c.created_at));
                    println!(
                        "claims: start {} · completion {} · deadline {} · {} milestone(s)",
                        c.claims.start.as_deref().unwrap_or("-"),
                        c.claims.completion.as_deref().unwrap_or("-"),
                        c.claims.deadline.as_deref().unwrap_or("-"),
                        c.claims.milestones.len()
                    );
                    println!("\nSubmissions");
                    for s in &subs {
                        println!(
                            "  #{:<3} {:<24} {} files · {} findings · {}",
                            s.id,
                            s.label,
                            s.files,
                            s.findings,
                            st.dim(&s.ingested_at)
                        );
                    }
                    if let Some(last) = subs.last() {
                        let a = store.assessment(last.id)?;
                        println!("\nFindings in latest submission (#{})", last.id);
                        for f in &a.findings {
                            let status = states
                                .get(&f.id)
                                .map(|s| s.status.as_str())
                                .unwrap_or("open");
                            println!(
                                "  {:<14} {:<9} {}  {}",
                                format!("{:?}", f.severity).replace("HighPriority", "HIGH"),
                                status,
                                st.bold(&f.id),
                                f.title
                            );
                        }
                    }
                    for n in &notes {
                        println!("\nnote {}: {}", st.dim(&n.at), n.text);
                    }
                }
            }
        }
        CaseCmd::Add {
            id,
            path,
            label,
            no_git,
            include_personal,
            c,
        } => {
            let case = require_case(&store, &id)?;
            let prev = match store.latest_submission(&id)? {
                Some(sid) => Some((
                    PathBuf::from(store.assessment(sid)?.root),
                    store.reports(sid)?,
                )),
                None => None,
            };
            let had_prev = prev.is_some();
            let done = run_assessment(
                ctx,
                &c,
                &id,
                &path,
                &case.claims,
                &Default::default(),
                prev,
                no_git,
                include_personal,
            )?;
            let label = if label.is_empty() {
                format!("submission {}", store.submissions(&id)?.len() + 1)
            } else {
                label
            };
            let sid = store.add_submission(&id, &label, &done.assessment, &done.reports)?;
            let states = store.finding_states(&id)?;
            let known = done
                .assessment
                .findings
                .iter()
                .filter(|f| states.contains_key(&f.id))
                .count();
            println!(
                "added submission #{sid} ({label}) to case {id}: {} files, {} findings{}",
                done.assessment.inventory.len(),
                done.assessment.findings.len(),
                if had_prev {
                    ", compared with the previous submission"
                } else {
                    ""
                }
            );
            if known > 0 {
                println!("{known} finding(s) already have a reviewer disposition from earlier submissions");
            }
            if done
                .assessment
                .integrity
                .as_ref()
                .is_some_and(|i| !i.unchanged)
            {
                eprintln!("warning: input files changed during analysis");
                return Ok(1);
            }
        }
        CaseCmd::Finding {
            id,
            finding_id,
            status,
            note,
            reason,
        } => {
            let case = require_case(&store, &id)?;
            // refuse unknown ids so typos do not silently create dispositions
            let known = store.submissions(&case.id)?.iter().any(|s| {
                store
                    .assessment(s.id)
                    .map(|a| a.findings.iter().any(|f| f.id == finding_id))
                    .unwrap_or(false)
            });
            if !known {
                bail!("finding '{finding_id}' does not occur in any submission of case {id}");
            }
            let status = FindingStatus::parse(&status).context("bad status")?;
            store.set_finding_state(
                &id,
                &finding_id,
                status,
                note.as_deref(),
                reason.as_deref(),
            )?;
            println!("{finding_id}: {}", status.as_str());
        }
        CaseCmd::Note { id, text } => {
            require_case(&store, &id)?;
            store.add_note(&id, &text)?;
            println!("note added");
        }
        CaseCmd::Report {
            id,
            submission,
            html,
            csv,
            timeline,
            force,
            format,
        } => {
            require_case(&store, &id)?;
            let sid = match submission {
                Some(s) => s,
                None => store
                    .latest_submission(&id)?
                    .context("case has no submissions yet")?,
            };
            if !store.submissions(&id)?.iter().any(|s| s.id == sid) {
                bail!("submission #{sid} does not belong to case {id}");
            }
            let a = store.assessment(sid)?;
            let states = store.finding_states(&id)?;
            let notes: Vec<(String, String)> = store
                .notes(&id)?
                .into_iter()
                .map(|n| (n.at, n.text))
                .collect();
            let events = if timeline {
                let reports = store.reports(sid)?;
                metadata_forensics::timeline::build(
                    std::path::Path::new(&a.root),
                    &reports,
                    None,
                    None,
                    Some(&a.claims),
                )
            } else {
                vec![]
            };
            if let Some(p) = html {
                let h = metadata_report::html(&metadata_report::ReportInput {
                    assessment: &a,
                    events: &events,
                    states: &states,
                    notes: &notes,
                    title: None,
                    include_timeline: timeline,
                });
                crate::write_output(&p, &h, force)?;
            }
            if let Some(p) = csv {
                crate::write_output(&p, &metadata_report::findings_csv(&a, &states), force)?;
            }
            if matches!(format, Some(Fmt::Json)) {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"assessment": a, "finding_states": states, "notes": notes})
                    )?
                );
            }
        }
        CaseCmd::Diff { id, a, b, format } => {
            require_case(&store, &id)?;
            let subs = store.submissions(&id)?;
            let (sa, sb) = (
                subs.iter()
                    .find(|s| s.id == a)
                    .context("unknown submission A")?,
                subs.iter()
                    .find(|s| s.id == b)
                    .context("unknown submission B")?,
            );
            let (ra, rb) = (store.reports(a)?, store.reports(b)?);
            let cmp = compare::compare_sets(
                std::path::Path::new(&store.assessment(a)?.root),
                &ra,
                std::path::Path::new(&store.assessment(b)?.root),
                &rb,
            );
            let changed: Vec<_> = cmp
                .pairs
                .iter()
                .filter(|p| p.content_identical == Some(false))
                .collect();
            match pick_format(ctx, format, Fmt::Human) {
                Fmt::Json | Fmt::Csv => println!("{}", serde_json::to_string_pretty(&cmp)?),
                Fmt::Human => {
                    println!(
                        "#{} ({}) ↔ #{} ({}): {} identical, {} changed, {} only in A, {} only in B",
                        sa.id,
                        sa.label,
                        sb.id,
                        sb.label,
                        cmp.pairs.len() - changed.len(),
                        changed.len(),
                        cmp.only_left.len(),
                        cmp.only_right.len()
                    );
                    for p in changed {
                        println!("\n{}", st.bold(&p.left));
                        for f in p
                            .fields
                            .iter()
                            .filter(|f| f.status != compare::DiffStatus::Identical)
                        {
                            println!(
                                "  {:?} {}: {} → {}",
                                f.status,
                                f.key,
                                f.left.as_deref().unwrap_or("-"),
                                f.right.as_deref().unwrap_or("-")
                            );
                        }
                    }
                    for f in &cmp.only_left {
                        println!("only in A: {f}");
                    }
                    for f in &cmp.only_right {
                        println!("only in B: {f}");
                    }
                }
            }
        }
    }
    Ok(0)
}
