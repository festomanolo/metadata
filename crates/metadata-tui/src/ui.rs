//! Rendering. Reads `App`, never mutates domain state (only list scroll offsets via widgets' local state).
use crate::app::{App, Focus, InputPurpose, Modal, Row, Section, SortKey, SECTIONS};
use crate::theme::{truecolor, Theme};
use metadata_forensics::compare::DiffStatus;
use metadata_forensics::timeline::Source;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap,
};
use ratatui::Frame;

pub const MIN_W: u16 = 60;
pub const MIN_H: u16 = 14;

fn clip(s: &str, w: usize) -> String {
    let one: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if one.chars().count() > w {
        format!(
            "{}…",
            one.chars().take(w.saturating_sub(1)).collect::<String>()
        )
    } else {
        one
    }
}

fn panel<'a>(t: &Theme, title: &'a str, focused: bool) -> Block<'a> {
    let b = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(if focused { t.accent_s() } else { t.border_s() });
    b.title(Span::styled(
        format!(" {title} "),
        if focused { t.title_s() } else { t.dim_s() },
    ))
}

pub fn draw(f: &mut Frame, app: &App) {
    let t = Theme::new(app.theme, truecolor());
    let area = f.area();
    if area.width < MIN_W || area.height < MIN_H {
        let msg = Paragraph::new(vec![
            Line::from(Span::styled("METADATA", t.title_s())),
            Line::from(format!(
                "Terminal too small ({}×{}).",
                area.width, area.height
            )),
            Line::from(format!(
                "Need at least {MIN_W}×{MIN_H}. Resize, or use the CLI commands."
            )),
        ])
        .alignment(Alignment::Center);
        f.render_widget(msg, area);
        return;
    }
    let [header, tabs, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(2),
    ])
    .areas(area);
    draw_header(f, header, app, &t);
    draw_tabs(f, tabs, app, &t);
    let wide = body.width >= 100;
    let tall = body.height >= 22;
    let (main, details) = if app.show_details && wide {
        let [m, d] = Layout::horizontal([Constraint::Min(40), Constraint::Length(42)]).areas(body);
        (m, Some(d))
    } else if app.show_details && tall {
        let [m, d] = Layout::vertical([Constraint::Min(8), Constraint::Length(9)]).areas(body);
        (m, Some(d))
    } else {
        (body, None)
    };
    match app.section {
        Section::Overview => draw_overview(f, main, app, &t),
        Section::Inspect => draw_inspect(f, main, app, &t),
        Section::Scan => draw_scan(f, main, app, &t),
        Section::Timeline => draw_timeline(f, main, app, &t),
        Section::Compare => draw_compare(f, main, app, &t),
        Section::Edit => draw_edit(f, main, app, &t),
        Section::Settings => draw_settings(f, main, app, &t),
        Section::Cases | Section::Reports => {
            let name = if app.section == Section::Cases {
                "Cases"
            } else {
                "Reports"
            };
            let p = Paragraph::new(vec![
                Line::from(Span::styled(
                    format!("{name} — not implemented yet"),
                    t.warn_s(),
                )),
                Line::from(""),
                Line::from("Persistent cases and HTML reports are planned. Use the CLI today:"),
                Line::from(Span::styled(
                    "  metadata assess <dir> --case ID -f json -o assessment.json",
                    t.dim_s(),
                )),
            ])
            .block(panel(&t, name, true));
            f.render_widget(p, main);
        }
    }
    if let Some(d) = details {
        draw_details(f, d, app, &t);
    }
    draw_footer(f, footer, app, &t);
    draw_modal(f, area, app, &t);
}

fn draw_header(f: &mut Frame, a: Rect, app: &App, t: &Theme) {
    let mut spans = vec![
        Span::styled("METADATA", t.title_s()),
        Span::styled(format!(" {}", env!("CARGO_PKG_VERSION")), t.dim_s()),
        Span::styled("  │  ", t.border_s()),
    ];
    spans.push(Span::styled("workspace ", t.dim_s()));
    spans.push(Span::raw(clip(
        &app.root.display().to_string(),
        (a.width as usize).saturating_sub(48).max(10),
    )));
    spans.push(Span::styled("  │  ", t.border_s()));
    for b in &app.backend_status {
        let active = b.name == "native" || (app.use_external && b.available);
        spans.push(Span::styled(
            if active { "● " } else { "○ " },
            if active { t.ok_s() } else { t.dim_s() },
        ));
        spans.push(Span::styled(
            format!("{} ", b.name),
            if active { t.base() } else { t.dim_s() },
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), a);
}

fn draw_tabs(f: &mut Frame, a: Rect, app: &App, t: &Theme) {
    // Progressive compaction so every section stays reachable on narrow terminals.
    let render = |numbers: bool, short: bool| -> Vec<(bool, bool, String)> {
        SECTIONS
            .iter()
            .enumerate()
            .map(|(i, (s, label, ok))| {
                let label = if short && *label == "Batch Scan" {
                    "Scan"
                } else {
                    label
                };
                let n = if numbers {
                    format!("{} ", i + 1)
                } else {
                    String::new()
                };
                (
                    *s == app.section,
                    *ok,
                    format!(" {n}{label}{} ", if *ok { "" } else { "·" }),
                )
            })
            .collect()
    };
    let width =
        |v: &Vec<(bool, bool, String)>| v.iter().map(|x| x.2.chars().count()).sum::<usize>();
    let mut tabs = render(true, false);
    for (n, s) in [(true, true), (false, false), (false, true)] {
        if width(&tabs) <= a.width as usize {
            break;
        }
        tabs = render(n, s);
    }
    let spans: Vec<Span> = tabs
        .into_iter()
        .map(|(active, ok, text)| {
            Span::styled(
                text,
                if active {
                    t.sel_s().add_modifier(Modifier::BOLD)
                } else if ok {
                    t.base()
                } else {
                    t.dim_s()
                },
            )
        })
        .collect();
    f.render_widget(Paragraph::new(Line::from(spans)), a);
}

fn draw_footer(f: &mut Frame, a: Rect, app: &App, t: &Theme) {
    let [l1, l2] = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(a);
    let keys = match app.section {
        Section::Inspect => "↑↓ select  ←→ pane  / search  s sort  Enter expand  y copy  m/c compare  e edit  x export  r reveal  ? help",
        Section::Scan => "Enter scan a directory  Ctrl+C cancel  Tab next section  ? help",
        Section::Timeline => "↑↓ scroll  s source filter  Tab next section  ? help",
        Section::Compare => "↑↓ scroll  i identical  Tab next section  ? help",
        Section::Edit => "↑↓ field  Enter set  x remove  u undo  p preview  w write copy  ? help",
        _ => "Tab/Shift+Tab sections  1-9 jump  ? help  q quit",
    };
    f.render_widget(
        Paragraph::new(Span::styled(clip(keys, a.width as usize), t.dim_s())),
        l1,
    );
    if let Some(s) = &app.scan {
        let ratio = if s.total == 0 {
            0.0
        } else {
            s.done as f64 / s.total as f64
        };
        let g = Gauge::default()
            .ratio(ratio.clamp(0.0, 1.0))
            .label(format!("scanning {}/{}  (Ctrl+C cancels)", s.done, s.total))
            .gauge_style(t.accent_s());
        f.render_widget(g, l2);
    } else {
        f.render_widget(
            Paragraph::new(Span::styled(clip(&app.status, a.width as usize), t.base())),
            l2,
        );
    }
}

fn draw_overview(f: &mut Frame, a: Rect, app: &App, t: &Theme) {
    let s = crate::app::Stats::compute(&app.reports);
    let [top, bottom] = Layout::vertical([Constraint::Length(9), Constraint::Min(3)]).areas(a);
    let [l, r] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(top);
    let kv = |k: &str, v: String, st: Style| {
        Line::from(vec![
            Span::styled(format!("{k:<26}"), t.dim_s()),
            Span::styled(v, st),
        ])
    };
    let lines = vec![
        kv("Files examined", s.files.to_string(), t.base()),
        kv("Metadata fields extracted", s.fields.to_string(), t.base()),
        kv(
            "Files with no stored metadata",
            s.missing_metadata.to_string(),
            t.base(),
        ),
        kv(
            "Need review (warnings/mismatch)",
            s.needs_review.to_string(),
            if s.needs_review > 0 {
                t.warn_s()
            } else {
                t.base()
            },
        ),
        kv(
            "Limited support (see Details)",
            s.unsupported.to_string(),
            t.base(),
        ),
        kv(
            "Extraction errors",
            s.errors.to_string(),
            if s.errors > 0 { t.err_s() } else { t.base() },
        ),
        kv("Warnings", s.warnings.to_string(), t.base()),
    ];
    f.render_widget(Paragraph::new(lines).block(panel(t, "Session", false)), l);
    let mut fm: Vec<(&String, &usize)> = s.formats.iter().collect();
    fm.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let items: Vec<Line> = if fm.is_empty() {
        vec![Line::from(Span::styled(
            "No files loaded. Open Batch Scan (3) and press Enter.",
            t.dim_s(),
        ))]
    } else {
        fm.iter()
            .take(7)
            .map(|(k, v)| {
                Line::from(vec![
                    Span::styled(format!("{k:<14}"), t.base()),
                    Span::styled(v.to_string(), t.accent_s()),
                ])
            })
            .collect()
    };
    f.render_widget(
        Paragraph::new(items).block(panel(t, "File types detected", false)),
        r,
    );
    let mut lines: Vec<Line> = app
        .backend_status
        .iter()
        .map(|b| {
            let (mark, st) = if b.available {
                ("available", t.ok_s())
            } else {
                ("missing", t.warn_s())
            };
            Line::from(vec![
                Span::styled(format!("{:<10}", b.name), t.base()),
                Span::styled(format!("{mark:<10}"), st),
                Span::styled(b.version.clone().unwrap_or_default(), t.dim_s()),
            ])
        })
        .collect();
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Recent operations", t.accent_s())));
    if app.recent.is_empty() {
        lines.push(Line::from(Span::styled("  none yet", t.dim_s())));
    }
    lines.extend(
        app.recent
            .iter()
            .map(|r| Line::from(format!("  {}", clip(r, a.width as usize - 6)))),
    );
    f.render_widget(
        Paragraph::new(lines).block(panel(t, "Backends & activity", false)),
        bottom,
    );
}

fn draw_inspect(f: &mut Frame, a: Rect, app: &App, t: &Theme) {
    let lw = (a.width / 3).clamp(24, 44);
    let [l, r] = Layout::horizontal([Constraint::Length(lw), Constraint::Min(20)]).areas(a);
    let sort = match app.sort {
        SortKey::Name => "name",
        SortKey::Size => "size",
        SortKey::Format => "format",
        SortKey::Warnings => "warnings",
    };
    let title = format!(
        "Files {}/{} · sort {sort}{}",
        app.visible.len(),
        app.reports.len(),
        if app.file_filter.is_empty() {
            String::new()
        } else {
            format!(" · /{}", app.file_filter)
        }
    );
    let items: Vec<ListItem> = app
        .visible
        .iter()
        .map(|&i| {
            let rp = &app.reports[i];
            let rel = std::path::Path::new(&rp.path)
                .strip_prefix(&app.root)
                .unwrap_or(std::path::Path::new(&rp.path))
                .display()
                .to_string();
            let mut spans = vec![];
            if app.mark == Some(i) {
                spans.push(Span::styled("◆ ", t.accent_s()));
            }
            if rp.error.is_some() {
                spans.push(Span::styled("✗ ", t.err_s()));
            } else if !rp.warnings.is_empty() {
                spans.push(Span::styled("! ", t.warn_s()));
            }
            spans.push(Span::raw(clip(&rel, lw as usize - 8)));
            ListItem::new(Line::from(spans))
        })
        .collect();
    let mut st = ListState::default().with_selected(if app.visible.is_empty() {
        None
    } else {
        Some(app.file_sel)
    });
    let list = List::new(items)
        .block(panel(t, &title, app.focus == Focus::Files))
        .highlight_style(t.sel_s());
    f.render_stateful_widget(list, l, &mut st);

    let rows = app.field_rows();
    let title = format!(
        "Metadata{}",
        if app.field_filter.is_empty() {
            String::new()
        } else {
            format!(" · /{}", app.field_filter)
        }
    );
    let Some(rep) = app.current() else {
        f.render_widget(
            Paragraph::new(Span::styled(
                "No file selected. Scan a directory from Batch Scan (3).",
                t.dim_s(),
            ))
            .block(panel(t, &title, false)),
            r,
        );
        return;
    };
    let w = r.width as usize;
    let items: Vec<ListItem> = rows
        .iter()
        .map(|row| match row {
            Row::Group {
                name,
                count,
                collapsed,
            } => ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{} {name} ", if *collapsed { "▸" } else { "▾" }),
                    t.accent_s(),
                ),
                Span::styled(format!("({count})"), t.dim_s()),
            ])),
            Row::Field(i) => {
                let fl = &rep.fields[*i];
                let nm = clip(&fl.name, 26);
                let mut spans = vec![
                    Span::styled(format!("   {nm:<26} "), t.dim_s()),
                    Span::raw(clip(&app.display_value(fl), w.saturating_sub(34))),
                ];
                if fl.time.as_ref().is_some_and(|x| !x.offset_known) {
                    spans.push(Span::styled(" ?tz", t.warn_s()));
                }
                ListItem::new(Line::from(spans))
            }
        })
        .collect();
    let mut st = ListState::default().with_selected(if rows.is_empty() {
        None
    } else {
        Some(app.field_cursor.min(rows.len() - 1))
    });
    let list = List::new(items)
        .block(panel(t, &title, app.focus == Focus::Fields))
        .highlight_style(if app.focus == Focus::Fields {
            t.sel_s()
        } else {
            Style::default()
        });
    f.render_stateful_widget(list, r, &mut st);
}

fn draw_scan(f: &mut Frame, a: Rect, app: &App, t: &Theme) {
    let mut lines = vec![Line::from(vec![Span::styled("Target  ", t.dim_s()), Span::raw(app.root.display().to_string())]), Line::from(Span::styled("Press Enter to choose a directory. Scans are recursive, skip .git/node_modules/target, and never follow symlinks.", t.dim_s())), Line::from("")];
    if let Some(s) = &app.scan {
        lines.push(Line::from(Span::styled(
            format!("Scanning… {}/{} files", s.done, s.total),
            t.accent_s(),
        )));
    }
    let problems: Vec<&metadata_core::FileReport> = app
        .reports
        .iter()
        .filter(|r| r.error.is_some() || !r.warnings.is_empty())
        .collect();
    lines.push(Line::from(Span::styled(
        format!(
            "{} files with errors or warnings · {} skipped paths",
            problems.len(),
            app.skipped.len()
        ),
        t.base(),
    )));
    for (i, r) in problems
        .iter()
        .enumerate()
        .skip(app.scan_cursor.saturating_sub(3))
        .take(a.height.saturating_sub(8) as usize)
    {
        let (mark, st) = if r.error.is_some() {
            ("✗", t.err_s())
        } else {
            ("!", t.warn_s())
        };
        let msg = r
            .error
            .clone()
            .or_else(|| r.warnings.first().map(|w| w.message.clone()))
            .unwrap_or_default();
        let sel = if i == app.scan_cursor {
            t.sel_s()
        } else {
            Style::default()
        };
        lines.push(Line::from(vec![
            Span::styled(format!(" {mark} "), st),
            Span::styled(
                clip(
                    &format!(
                        "{}  {}",
                        std::path::Path::new(&r.path)
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy(),
                        msg
                    ),
                    a.width as usize - 8,
                ),
                sel,
            ),
        ]));
    }
    f.render_widget(Paragraph::new(lines).block(panel(t, "Batch Scan", true)), a);
}

fn source_style(t: &Theme, s: Source) -> Style {
    match s {
        Source::Embedded => t.accent_s(),
        Source::Filesystem => t.dim_s(),
        Source::Archive => t.ok_s(),
        Source::Git => t.warn_s(),
        _ => t.base(),
    }
}

fn draw_timeline(f: &mut Frame, a: Rect, app: &App, t: &Theme) {
    let ev = app.events();
    let title = format!(
        "Timeline · {} events · source: {}",
        ev.len(),
        app.timeline_filter.map(|s| s.label()).unwrap_or("all")
    );
    let w = a.width as usize;
    let items: Vec<ListItem> = ev
        .iter()
        .map(|e| {
            let tz = if e.time.offset_known { ' ' } else { '?' };
            ListItem::new(Line::from(vec![
                Span::raw(format!("{}{tz} ", e.time.iso)),
                Span::styled(
                    format!("{:<10}", e.source.label()),
                    source_style(t, e.source),
                ),
                Span::raw(clip(&format!("{}  ", e.file), 30)),
                Span::styled(clip(&e.field, w.saturating_sub(70).max(10)), t.dim_s()),
            ]))
        })
        .collect();
    let mut st = ListState::default().with_selected(if ev.is_empty() {
        None
    } else {
        Some(app.timeline_scroll.min(ev.len() - 1))
    });
    f.render_stateful_widget(
        List::new(items)
            .block(panel(t, &title, true))
            .highlight_style(t.sel_s()),
        a,
        &mut st,
    );
}

fn draw_compare(f: &mut Frame, a: Rect, app: &App, t: &Theme) {
    let Some(c) = &app.comparison else {
        let p = Paragraph::new("In Inspect, press m on one file, move to another, then press c.")
            .style(t.dim_s())
            .block(panel(t, "Compare", true));
        return f.render_widget(p, a);
    };
    let n = |s| c.count(s);
    let title = format!(
        "{} ↔ {} · {} changed · {} added · {} removed",
        clip(&c.left, 24),
        clip(&c.right, 24),
        n(DiffStatus::Changed),
        n(DiffStatus::Added),
        n(DiffStatus::Removed)
    );
    let rows: Vec<&metadata_forensics::compare::FieldDiff> = c
        .fields
        .iter()
        .filter(|d| app.show_identical || d.status != DiffStatus::Identical)
        .collect();
    let w = a.width as usize;
    let items: Vec<ListItem> = rows
        .iter()
        .map(|d| {
            let (m, st) = match d.status {
                DiffStatus::Changed => ("~", t.warn_s()),
                DiffStatus::Added => ("+", t.ok_s()),
                DiffStatus::Removed => ("-", t.err_s()),
                _ => ("=", t.dim_s()),
            };
            let val = match d.status {
                DiffStatus::Changed => format!(
                    "{} → {}",
                    clip(d.left.as_deref().unwrap_or(""), (w - 40) / 2),
                    clip(d.right.as_deref().unwrap_or(""), (w - 40) / 2)
                ),
                DiffStatus::Added => d.right.clone().unwrap_or_default(),
                _ => d.left.clone().unwrap_or_default(),
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{m} "), st),
                Span::raw(format!("{:<30}", clip(&d.key, 30))),
                Span::styled(clip(&val, w.saturating_sub(36)), t.dim_s()),
            ]))
        })
        .collect();
    let mut st = ListState::default().with_selected(if rows.is_empty() {
        None
    } else {
        Some(app.compare_scroll.min(rows.len() - 1))
    });
    f.render_stateful_widget(
        List::new(items)
            .block(panel(t, &title, true))
            .highlight_style(t.sel_s()),
        a,
        &mut st,
    );
}

fn draw_edit(f: &mut Frame, a: Rect, app: &App, t: &Theme) {
    let Some(rep) = app.current() else {
        return f.render_widget(
            Paragraph::new("Select a file in Inspect first, then press e.")
                .style(t.dim_s())
                .block(panel(t, "Edit", true)),
            a,
        );
    };
    let (keys, free) = app.edit_keys();
    let [l, r] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(a);
    let mut items: Vec<ListItem> = keys
        .iter()
        .map(|k| {
            let cur = rep
                .find(k)
                .first()
                .map(|x| app.display_value(x))
                .unwrap_or_else(|| "(unset)".into());
            let pend = app.edit.ops.iter().rev().find_map(|o| match o {
                metadata_edit::Op::Set(x, v) if x == k => Some(format!(" → {v}")),
                metadata_edit::Op::Remove(x) if x == k => Some(" → (remove)".into()),
                _ => None,
            });
            ListItem::new(Line::from(vec![
                Span::raw(format!("{k:<26}")),
                Span::styled(clip(&cur, 22), t.dim_s()),
                Span::styled(pend.unwrap_or_default(), t.warn_s()),
            ]))
        })
        .collect();
    if free {
        items.push(ListItem::new(Span::styled("+ other field…", t.accent_s())));
    }
    let title = format!(
        "Writable fields · {} ({})",
        clip(
            &std::path::Path::new(&rep.path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy(),
            24
        ),
        rep.format.id
    );
    if items.is_empty() {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(format!(
                    "{} has no natively writable fields.",
                    rep.format.id
                )),
                Line::from(Span::styled(
                    "Install exiftool for broader write support (see Overview).",
                    t.dim_s(),
                )),
            ])
            .block(panel(t, &title, true)),
            l,
        );
    } else {
        let mut st = ListState::default().with_selected(Some(app.edit.cursor.min(items.len() - 1)));
        f.render_stateful_widget(
            List::new(items)
                .block(panel(t, &title, true))
                .highlight_style(t.sel_s()),
            l,
            &mut st,
        );
    }
    let mut lines: Vec<Line> = vec![Line::from(Span::styled("Edits are written to a NEW copy (<name>.edited.<ext>); the original is never modified here.", t.dim_s())), Line::from("")];
    lines.extend(
        app.edit
            .preview
            .iter()
            .map(|s| Line::from(clip(s, r.width as usize - 4))),
    );
    if !app.edit.result.is_empty() {
        lines.push(Line::from(""));
        lines.extend(
            app.edit
                .result
                .iter()
                .map(|s| Line::from(Span::styled(clip(s, r.width as usize - 4), t.ok_s()))),
        );
    }
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(panel(t, "Preview & result", false)),
        r,
    );
}

fn draw_settings(f: &mut Frame, a: Rect, app: &App, t: &Theme) {
    let on = |b: bool| if b { "on" } else { "off" };
    let items = [
        format!("Theme                  {:?}   (T cycles)", app.theme),
        format!("Reveal personal data   {}   (r)", on(app.reveal)),
        format!("Details panel          {}   (d)", on(app.show_details)),
        format!(
            "External backends      {}   (set with --no-external when launching)",
            on(app.use_external)
        ),
    ];
    let list = List::new(items.into_iter().map(ListItem::new).collect::<Vec<_>>())
        .block(panel(
            t,
            "Settings (session only; persistent options live in `metadata config`)",
            true,
        ))
        .highlight_style(t.sel_s());
    let mut st = ListState::default().with_selected(Some(app.settings_cursor));
    f.render_stateful_widget(list, a, &mut st);
}

fn draw_details(f: &mut Frame, a: Rect, app: &App, t: &Theme) {
    let w = a.width.saturating_sub(4) as usize;
    let mut lines: Vec<Line> = vec![];
    if let Some(r) = app.current() {
        let name = std::path::Path::new(&r.path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        lines.push(Line::from(Span::styled(clip(&name, w), t.title_s())));
        lines.push(Line::from(Span::styled(
            clip(&format!("{} · {} B", r.format.description, r.size), w),
            t.dim_s(),
        )));
        if let Some(h) = &r.sha256 {
            lines.push(Line::from(Span::styled(
                format!("sha256 {}", &h[..16]),
                t.dim_s(),
            )));
        }
        let selected_field = if app.section == Section::Inspect && app.focus == Focus::Fields {
            match app.field_rows().into_iter().nth(app.field_cursor) {
                Some(Row::Field(i)) => Some(&r.fields[i]),
                _ => None,
            }
        } else {
            None
        };
        if let Some(fl) = selected_field {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(clip(&fl.key(), w), t.accent_s())));
            lines.push(Line::from(clip(&app.display_value(fl), w * 3)));
            if let Some(tm) = &fl.time {
                lines.push(Line::from(Span::styled(
                    if tm.offset_known {
                        format!("UTC {}", tm.utc.clone().unwrap_or_default())
                    } else {
                        "timezone NOT recorded — not assumed UTC".into()
                    },
                    if tm.offset_known {
                        t.ok_s()
                    } else {
                        t.warn_s()
                    },
                )));
            }
            lines.push(Line::from(Span::styled(
                format!("source: {} · {:?}", fl.backend, fl.provenance),
                t.dim_s(),
            )));
            if let Some(x) = metadata_engine::explain::explain(&fl.group, &fl.name) {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(x, t.dim_s())));
            }
        } else {
            lines.push(Line::from(""));
            if r.format.extension_match == metadata_core::ExtensionMatch::Mismatch {
                lines.push(Line::from(Span::styled(
                    "extension does not match content",
                    t.warn_s(),
                )));
            }
            for w_ in r.warnings.iter().take(4) {
                lines.push(Line::from(Span::styled(
                    format!("! {}", clip(&w_.message, w)),
                    t.warn_s(),
                )));
            }
            for u in r.unavailable.iter().take(3) {
                lines.push(Line::from(Span::styled(
                    format!("– {}", clip(u, w)),
                    t.dim_s(),
                )));
            }
            if let Some(m) = &r.filesystem.modified {
                lines.push(Line::from(Span::styled(
                    format!("fs modified {}", m.iso),
                    t.dim_s(),
                )));
            }
            lines.push(Line::from(Span::styled(
                "filesystem times change on copy/extract",
                t.dim_s(),
            )));
        }
    } else {
        lines.push(Line::from(Span::styled("No file selected.", t.dim_s())));
    }
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(panel(t, "Details", false)),
        a,
    );
}

fn centered(a: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(a.width);
    let h = h.min(a.height);
    Rect {
        x: a.x + (a.width - w) / 2,
        y: a.y + (a.height - h) / 2,
        width: w,
        height: h,
    }
}

fn draw_modal(f: &mut Frame, area: Rect, app: &App, t: &Theme) {
    match &app.modal {
        Modal::None => {}
        Modal::Help => {
            let r = centered(area, 72, 22);
            f.render_widget(Clear, r);
            let rows = [
                ("Tab / Shift+Tab", "next / previous section"),
                ("1-9", "jump to section"),
                ("↑ ↓  j k  PgUp PgDn", "move selection"),
                ("← →  h l", "switch pane (Inspect)"),
                ("/", "search files or fields"),
                ("Enter / Space", "expand or collapse group, open"),
                ("s", "sort files / cycle source filter"),
                ("y", "copy selected value (OSC 52)"),
                ("m then c", "mark a file, compare with another"),
                ("e", "edit selected file (writes a new copy)"),
                ("x", "export selected file as JSON"),
                ("r", "reveal / hide personal & location values"),
                ("T", "cycle theme"),
                ("d", "toggle details panel"),
                ("Esc", "cancel / back / clear filter"),
                ("Ctrl+C", "cancel running scan (twice when idle quits)"),
                ("q", "quit"),
            ];
            let mut lines: Vec<Line> = rows
                .iter()
                .map(|(k, d)| {
                    Line::from(vec![
                        Span::styled(format!("{k:<22}"), t.accent_s()),
                        Span::raw(*d),
                    ])
                })
                .collect();
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("Metadata is evidence to weigh, not proof. Timestamps shown with ?tz have no recorded timezone.", t.dim_s())));
            f.render_widget(
                Paragraph::new(lines).block(panel(t, "Help — Esc to close", true)),
                r,
            );
        }
        Modal::Message(m) => {
            let r = centered(area, 60, 7);
            f.render_widget(Clear, r);
            f.render_widget(
                Paragraph::new(m.as_str())
                    .wrap(Wrap { trim: true })
                    .block(panel(t, "Notice", true)),
                r,
            );
        }
        Modal::ConfirmWrite => {
            let r = centered(area, 64, 9);
            f.render_widget(Clear, r);
            let mut lines = vec![Line::from(
                "Write an edited COPY with these changes? The original is not modified.",
            )];
            if let Some(p) = &app.edit.plan {
                lines.extend(
                    p.warnings.iter().map(|w| {
                        Line::from(Span::styled(format!("! {}", clip(w, 58)), t.warn_s()))
                    }),
                );
                if p.signature_risk {
                    lines.push(Line::from(Span::styled(
                        "Signed file: use the CLI with --accept-signature-loss.",
                        t.err_s(),
                    )));
                }
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("y / Enter = write     any other key = cancel", t.dim_s())));
            f.render_widget(Paragraph::new(lines).block(panel(t, "Confirm", true)), r);
        }
        Modal::Search { buf, .. } => {
            let r = Rect { x: area.x, y: area.y + area.height.saturating_sub(2), width: area.width, height: 1 };
            f.render_widget(Clear, r);
            f.render_widget(Paragraph::new(Line::from(vec![Span::styled("/", t.accent_s()), Span::raw(buf.clone()), Span::styled("▏", t.accent_s()), Span::styled("   Enter keep · Esc then / to refine", t.dim_s())])), r);
        }
        Modal::Input { prompt, buf, purpose } => {
            let r = centered(area, 70, 5);
            f.render_widget(Clear, r);
            let hint = if *purpose == InputPurpose::ScanPath { "Enter to start · Esc to cancel" } else { "Enter to queue · Esc to cancel" };
            f.render_widget(Paragraph::new(vec![Line::from(vec![Span::raw(buf.clone()), Span::styled("▏", t.accent_s())]), Line::from(Span::styled(hint, t.dim_s()))]).block(panel(t, prompt, true)), r);
        }
    }
}
