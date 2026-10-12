//! Application state and input handling. Pure logic: no terminal I/O, so it is unit-testable.
use metadata_core::{FileReport, Provenance};
use metadata_edit::{execute, plan, ApplyOpts, EditRequest, Op, Plan};
use metadata_engine::backends::BackendStatus;
use metadata_engine::{collect_files, Backends, Engine, Options, ScanOptions};
use metadata_forensics::compare::{compare_reports, Comparison};
use metadata_forensics::timeline::{self, Event, Source};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;

use crate::theme::ThemeKind;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Overview,
    Inspect,
    Scan,
    Timeline,
    Compare,
    Edit,
    Cases,
    Reports,
    Settings,
}

/// (section, label, implemented)
pub const SECTIONS: &[(Section, &str, bool)] = &[
    (Section::Overview, "Overview", true),
    (Section::Inspect, "Inspect", true),
    (Section::Scan, "Batch Scan", true),
    (Section::Timeline, "Timeline", true),
    (Section::Compare, "Compare", true),
    (Section::Edit, "Edit", true),
    (Section::Cases, "Cases", false),
    (Section::Reports, "Reports", false),
    (Section::Settings, "Settings", true),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    Files,
    Fields,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortKey {
    Name,
    Size,
    Format,
    Warnings,
}

#[derive(Clone, Debug, PartialEq)]
pub enum InputPurpose {
    ScanPath,
    EditValue(String),
    EditKey,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SearchTarget {
    Files,
    Fields,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Modal {
    None,
    Help,
    Search {
        target: SearchTarget,
        buf: String,
    },
    Input {
        prompt: String,
        buf: String,
        purpose: InputPurpose,
    },
    ConfirmWrite,
    Message(String),
}

pub enum Row {
    Group {
        name: String,
        count: usize,
        collapsed: bool,
    },
    Field(usize),
}

pub enum ScanMsg {
    Progress(usize, usize),
    Done(Vec<FileReport>, Vec<(String, String)>),
}

pub struct ScanState {
    pub rx: Receiver<ScanMsg>,
    pub done: usize,
    pub total: usize,
    pub cancel: Arc<AtomicBool>,
    pub root: PathBuf,
}

#[derive(Default)]
pub struct Stats {
    pub files: usize,
    pub fields: usize,
    pub formats: BTreeMap<String, usize>,
    pub missing_metadata: usize,
    pub needs_review: usize,
    pub unsupported: usize,
    pub errors: usize,
    pub warnings: usize,
}

impl Stats {
    pub fn compute(reports: &[FileReport]) -> Stats {
        let mut s = Stats {
            files: reports.len(),
            ..Default::default()
        };
        for r in reports {
            *s.formats.entry(r.format.id.clone()).or_default() += 1;
            s.fields += r.fields.len();
            s.warnings += r.warnings.len();
            if r.error.is_some() {
                s.errors += 1;
                continue;
            }
            let stored = r
                .fields
                .iter()
                .filter(|f| f.provenance == Provenance::Stored)
                .count();
            if stored == 0 && !matches!(r.format.id.as_str(), "binary" | "text" | "source") {
                s.missing_metadata += 1;
            }
            if !r.warnings.is_empty()
                || r.format.extension_match == metadata_core::ExtensionMatch::Mismatch
            {
                s.needs_review += 1;
            }
            if !r.unavailable.is_empty() || r.format.id == "binary" {
                s.unsupported += 1;
            }
        }
        s
    }
}

#[derive(Default)]
pub struct EditState {
    pub cursor: usize,
    pub ops: Vec<Op>,
    pub preview: Vec<String>,
    pub result: Vec<String>,
    pub plan: Option<Plan>,
    pub pending_key: Option<String>,
}

pub struct App {
    pub root: PathBuf,
    pub backends: Backends,
    pub backend_status: Vec<BackendStatus>,
    pub use_external: bool,
    pub reports: Vec<FileReport>,
    pub skipped: Vec<(String, String)>,
    pub section: Section,
    pub focus: Focus,
    pub visible: Vec<usize>,
    pub file_sel: usize,
    pub file_filter: String,
    pub sort: SortKey,
    pub field_cursor: usize,
    pub field_filter: String,
    pub collapsed: HashSet<String>,
    pub reveal: bool,
    pub theme: ThemeKind,
    pub show_details: bool,
    pub scan: Option<ScanState>,
    pub status: String,
    pub recent: VecDeque<String>,
    pub mark: Option<usize>,
    pub comparison: Option<Comparison>,
    pub compare_scroll: usize,
    pub show_identical: bool,
    pub timeline_filter: Option<Source>,
    pub timeline_scroll: usize,
    pub settings_cursor: usize,
    pub edit: EditState,
    pub modal: Modal,
    pub should_quit: bool,
    pub quit_armed: bool,
    pub clipboard: Option<String>,
    pub scan_cursor: usize,
}

impl App {
    pub fn new(root: PathBuf, use_external: bool) -> App {
        let backends = if use_external {
            Backends::discover()
        } else {
            Backends::none()
        };
        let backend_status = Backends::discover().status();
        App {
            root,
            backends,
            backend_status,
            use_external,
            reports: vec![],
            skipped: vec![],
            section: Section::Overview,
            focus: Focus::Files,
            visible: vec![],
            file_sel: 0,
            file_filter: String::new(),
            sort: SortKey::Name,
            field_cursor: 0,
            field_filter: String::new(),
            collapsed: HashSet::new(),
            reveal: false,
            theme: if std::env::var_os("NO_COLOR").is_some() {
                ThemeKind::Mono
            } else {
                ThemeKind::Dark
            },
            show_details: true,
            scan: None,
            status: "Ready. Press ? for help.".into(),
            recent: VecDeque::new(),
            mark: None,
            comparison: None,
            compare_scroll: 0,
            show_identical: false,
            timeline_filter: None,
            timeline_scroll: 0,
            settings_cursor: 0,
            edit: EditState::default(),
            modal: Modal::None,
            should_quit: false,
            quit_armed: false,
            clipboard: None,
            scan_cursor: 0,
        }
    }

    /// Load reports directly (tests, or after a synchronous scan).
    pub fn set_reports(&mut self, reports: Vec<FileReport>) {
        self.reports = reports;
        self.refresh_visible();
    }

    fn engine(&self, cancel: Option<Arc<AtomicBool>>) -> Engine {
        Engine {
            backends: self.backends.clone(),
            opts: Options {
                use_external: self.use_external,
                cancel,
                ..Default::default()
            },
        }
    }

    pub fn log(&mut self, s: impl Into<String>) {
        self.recent.push_front(s.into());
        self.recent.truncate(8);
    }

    // ---------- scanning ----------

    pub fn start_scan(&mut self, path: PathBuf) {
        if self.scan.is_some() {
            self.status = "A scan is already running (Ctrl+C cancels).".into();
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let engine = self.engine(Some(cancel.clone()));
        let (tx, rx) = channel();
        let p = path.clone();
        std::thread::spawn(move || {
            let so = ScanOptions {
                recursive: true,
                exclude_dirs: [".git", "node_modules", "target", "__pycache__"]
                    .map(String::from)
                    .to_vec(),
                ..Default::default()
            };
            let col = collect_files(std::slice::from_ref(&p), &so);
            let skipped: Vec<(String, String)> = col
                .skipped
                .iter()
                .map(|(a, b)| (a.display().to_string(), b.clone()))
                .collect();
            let _ = tx.send(ScanMsg::Progress(0, col.files.len()));
            let tx2 = tx.clone();
            let reports = engine.inspect_many(&col.files, so.jobs, &move |d, t| {
                let _ = tx2.send(ScanMsg::Progress(d, t));
            });
            let _ = tx.send(ScanMsg::Done(reports, skipped));
        });
        self.status = format!("Scanning {} …", path.display());
        self.root = path.clone();
        self.scan = Some(ScanState {
            rx,
            done: 0,
            total: 0,
            cancel,
            root: path,
        });
    }

    /// Drain background messages; call once per UI tick.
    pub fn poll(&mut self) {
        let mut finished = None;
        if let Some(s) = &mut self.scan {
            loop {
                match s.rx.try_recv() {
                    Ok(ScanMsg::Progress(d, t)) => {
                        s.done = d;
                        s.total = t;
                    }
                    Ok(ScanMsg::Done(r, sk)) => {
                        finished = Some((r, sk, s.cancel.load(Ordering::Relaxed)));
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        finished = Some((vec![], vec![], true));
                        break;
                    }
                }
            }
        }
        if let Some((r, sk, cancelled)) = finished {
            let n = r.len();
            self.scan = None;
            self.reports = r;
            self.skipped = sk;
            self.refresh_visible();
            self.file_sel = 0;
            self.mark = None;
            let msg = if cancelled {
                format!("Scan cancelled after {n} files")
            } else {
                format!("Scan finished: {n} files")
            };
            self.status = msg.clone();
            self.log(msg);
        }
    }

    // ---------- derived data ----------

    pub fn refresh_visible(&mut self) {
        let f = self.file_filter.to_ascii_lowercase();
        let mut v: Vec<usize> = (0..self.reports.len())
            .filter(|&i| {
                f.is_empty()
                    || self.reports[i].path.to_ascii_lowercase().contains(&f)
                    || self.reports[i].format.id.contains(&f)
            })
            .collect();
        let r = &self.reports;
        match self.sort {
            SortKey::Name => v.sort_by(|&a, &b| r[a].path.cmp(&r[b].path)),
            SortKey::Size => v.sort_by(|&a, &b| r[b].size.cmp(&r[a].size)),
            SortKey::Format => v.sort_by(|&a, &b| {
                r[a].format
                    .id
                    .cmp(&r[b].format.id)
                    .then(r[a].path.cmp(&r[b].path))
            }),
            SortKey::Warnings => v.sort_by(|&a, &b| {
                r[b].warnings
                    .len()
                    .cmp(&r[a].warnings.len())
                    .then(r[a].path.cmp(&r[b].path))
            }),
        }
        self.visible = v;
        self.file_sel = self.file_sel.min(self.visible.len().saturating_sub(1));
    }

    pub fn current_index(&self) -> Option<usize> {
        self.visible.get(self.file_sel).copied()
    }

    pub fn current(&self) -> Option<&FileReport> {
        self.current_index().map(|i| &self.reports[i])
    }

    pub fn field_rows(&self) -> Vec<Row> {
        let Some(r) = self.current() else {
            return vec![];
        };
        let flt = self.field_filter.to_ascii_lowercase();
        let mut groups: Vec<(String, Vec<usize>)> = vec![];
        for (i, f) in r.fields.iter().enumerate() {
            if !flt.is_empty()
                && !(f.name.to_ascii_lowercase().contains(&flt)
                    || f.group.to_ascii_lowercase().contains(&flt)
                    || f.raw.to_ascii_lowercase().contains(&flt))
            {
                continue;
            }
            match groups.iter_mut().find(|(g, _)| *g == f.group) {
                Some((_, v)) => v.push(i),
                None => groups.push((f.group.clone(), vec![i])),
            }
        }
        let mut rows = vec![];
        for (g, idx) in groups {
            let collapsed = self.collapsed.contains(&g);
            rows.push(Row::Group {
                name: g,
                count: idx.len(),
                collapsed,
            });
            if !collapsed {
                rows.extend(idx.into_iter().map(Row::Field));
            }
        }
        rows
    }

    pub fn events(&self) -> Vec<Event> {
        let mut v = timeline::file_events(&self.root, &self.reports);
        if let Some(s) = self.timeline_filter {
            v.retain(|e| e.source == s);
        }
        v.sort_by(|a, b| {
            a.time
                .sort_key()
                .cmp(&b.time.sort_key())
                .then(a.file.cmp(&b.file))
        });
        v
    }

    pub fn display_value(&self, f: &metadata_core::Field) -> String {
        if f.sensitive && !self.reveal {
            "●●●●●● (hidden — press r to reveal personal/location data)".into()
        } else {
            f.raw.clone()
        }
    }

    /// Writable keys for the current file (native writers only) plus whether free-form keys are accepted.
    pub fn edit_keys(&self) -> (Vec<String>, bool) {
        let Some(r) = self.current() else {
            return (vec![], false);
        };
        let k = metadata_edit::native::keys(&r.format.id);
        let free = k.is_empty() && self.backends.exiftool.is_some() || r.format.id == "png";
        (k.into_iter().filter(|k| !k.contains('<')).collect(), free)
    }

    // ---------- input ----------

    pub fn on_key(&mut self, k: KeyEvent) {
        if k.kind == KeyEventKind::Release {
            return;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && k.code == KeyCode::Char('c') {
            return self.ctrl_c();
        }
        self.quit_armed = false;
        match self.modal.clone() {
            Modal::None => self.on_key_main(k),
            Modal::Help | Modal::Message(_) => {
                if matches!(
                    k.code,
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') | KeyCode::Char('?')
                ) {
                    self.modal = Modal::None;
                }
            }
            Modal::ConfirmWrite => match k.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    self.modal = Modal::None;
                    self.do_write();
                }
                _ => {
                    self.modal = Modal::None;
                    self.status = "Write cancelled.".into();
                }
            },
            Modal::Search { target, mut buf } => match k.code {
                KeyCode::Esc => {
                    self.modal = Modal::None;
                }
                KeyCode::Enter => {
                    self.modal = Modal::None;
                }
                KeyCode::Backspace => {
                    buf.pop();
                    self.apply_search(&target, &buf);
                    self.modal = Modal::Search { target, buf };
                }
                KeyCode::Char(c) => {
                    buf.push(c);
                    self.apply_search(&target, &buf);
                    self.modal = Modal::Search { target, buf };
                }
                _ => {}
            },
            Modal::Input {
                prompt,
                mut buf,
                purpose,
            } => match k.code {
                KeyCode::Esc => {
                    self.modal = Modal::None;
                    self.status = "Cancelled.".into();
                }
                KeyCode::Enter => {
                    self.modal = Modal::None;
                    self.submit_input(purpose, buf);
                }
                KeyCode::Backspace => {
                    buf.pop();
                    self.modal = Modal::Input {
                        prompt,
                        buf,
                        purpose,
                    };
                }
                KeyCode::Char(c) => {
                    buf.push(c);
                    self.modal = Modal::Input {
                        prompt,
                        buf,
                        purpose,
                    };
                }
                _ => {}
            },
        }
    }

    fn ctrl_c(&mut self) {
        if let Some(s) = &self.scan {
            s.cancel.store(true, Ordering::SeqCst);
            self.status = "Cancelling scan…".into();
        } else if self.quit_armed {
            self.should_quit = true;
        } else {
            self.quit_armed = true;
            self.status = "Nothing running. Press Ctrl+C again or q to quit.".into();
        }
    }

    fn apply_search(&mut self, t: &SearchTarget, buf: &str) {
        match t {
            SearchTarget::Files => {
                self.file_filter = buf.to_string();
                self.refresh_visible();
            }
            SearchTarget::Fields => {
                self.field_filter = buf.to_string();
                self.field_cursor = 0;
            }
        }
    }

    fn submit_input(&mut self, purpose: InputPurpose, text: String) {
        match purpose {
            InputPurpose::ScanPath => {
                let p = PathBuf::from(text.trim());
                if p.exists() {
                    self.start_scan(p);
                } else {
                    self.status = format!("{} does not exist.", p.display());
                }
            }
            InputPurpose::EditKey => {
                let k = text.trim().to_string();
                if k.is_empty() {
                    return;
                }
                self.modal = Modal::Input { prompt: format!("New value for {k}"), buf: String::new(), purpose: InputPurpose::EditValue(k) };
            }
            InputPurpose::EditValue(k) => {
                self.edit.ops.retain(|o| !matches!(o, Op::Set(x, _) | Op::Remove(x) if *x == k));
                self.edit.ops.push(Op::Set(k.clone(), text));
                self.edit.plan = None;
                self.status = format!("Queued change to {k}. Press p to preview, w to write a copy.");
            }
        }
    }

    fn move_cursor(cur: &mut usize, len: usize, delta: isize) {
        if len == 0 {
            *cur = 0;
            return;
        }
        *cur = (*cur as isize + delta).clamp(0, len as isize - 1) as usize;
    }

    fn nav(&mut self, delta: isize) {
        match self.section {
            Section::Inspect => match self.focus {
                Focus::Files => {
                    let n = self.visible.len();
                    Self::move_cursor(&mut self.file_sel, n, delta);
                    self.field_cursor = 0;
                    self.edit = EditState::default();
                }
                Focus::Fields => {
                    let n = self.field_rows().len();
                    Self::move_cursor(&mut self.field_cursor, n, delta);
                }
            },
            Section::Scan => {
                let n = self.reports.iter().filter(|r| r.error.is_some() || !r.warnings.is_empty()).count();
                Self::move_cursor(&mut self.scan_cursor, n, delta);
            }
            Section::Timeline => {
                let n = self.events().len();
                Self::move_cursor(&mut self.timeline_scroll, n, delta);
            }
            Section::Compare => {
                let n = self.comparison.as_ref().map(|c| c.fields.len()).unwrap_or(0);
                Self::move_cursor(&mut self.compare_scroll, n, delta);
            }
            Section::Edit => {
                let (keys, free) = self.edit_keys();
                let n = keys.len() + usize::from(free);
                Self::move_cursor(&mut self.edit.cursor, n, delta);
            }
            Section::Settings => Self::move_cursor(&mut self.settings_cursor, 4, delta),
            _ => {}
        }
    }

    pub fn goto(&mut self, s: Section) {
        self.section = s;
    }

    fn cycle_section(&mut self, d: isize) {
        let i = SECTIONS.iter().position(|(s, _, _)| *s == self.section).unwrap_or(0) as isize;
        let n = SECTIONS.len() as isize;
        self.section = SECTIONS[((i + d).rem_euclid(n)) as usize].0;
    }

    fn on_key_main(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Tab => return self.cycle_section(1),
            KeyCode::BackTab => return self.cycle_section(-1),
            KeyCode::Char('?') => {
                self.modal = Modal::Help;
                return;
            }
            KeyCode::Char('q') => {
                if self.scan.is_some() {
                    self.status = "A scan is running: Ctrl+C cancels it, then q quits.".into();
                } else {
                    self.should_quit = true;
                }
                return;
            }
            KeyCode::Char(c @ '1'..='9') => {
                if let Some((s, _, _)) = SECTIONS.get(c as usize - '1' as usize) {
                    self.section = *s;
                }
                return;
            }
            KeyCode::Up | KeyCode::Char('k') => return self.nav(-1),
            KeyCode::Down | KeyCode::Char('j') => return self.nav(1),
            KeyCode::PageUp => return self.nav(-10),
            KeyCode::PageDown => return self.nav(10),
            KeyCode::Home => return self.nav(-100_000),
            KeyCode::End => return self.nav(100_000),
            KeyCode::Char('r') => {
                self.reveal = !self.reveal;
                self.status = if self.reveal { "Personal and location values are now VISIBLE.".into() } else { "Personal and location values hidden.".into() };
                return;
            }
            KeyCode::Char('d') if self.section != Section::Edit => {
                self.show_details = !self.show_details;
                return;
            }
            KeyCode::Char('T') => {
                self.theme = match self.theme {
                    ThemeKind::Dark => ThemeKind::Light,
                    ThemeKind::Light => ThemeKind::Mono,
                    ThemeKind::Mono => ThemeKind::Dark,
                };
                return;
            }
            _ => {}
        }
        match self.section {
            Section::Inspect => self.key_inspect(k),
            Section::Scan => {
                if k.code == KeyCode::Enter {
                    self.modal = Modal::Input { prompt: "Directory to scan (recursive)".into(), buf: self.root.display().to_string(), purpose: InputPurpose::ScanPath };
                }
            }
            Section::Timeline => {
                if k.code == KeyCode::Char('s') {
                    self.timeline_filter = match self.timeline_filter {
                        None => Some(Source::Embedded),
                        Some(Source::Embedded) => Some(Source::Archive),
                        Some(Source::Archive) => Some(Source::Filesystem),
                        _ => None,
                    };
                    self.timeline_scroll = 0;
                }
            }
            Section::Compare => {
                if k.code == KeyCode::Char('i') {
                    self.show_identical = !self.show_identical;
                }
            }
            Section::Edit => self.key_edit(k),
            Section::Settings => {
                if matches!(k.code, KeyCode::Enter | KeyCode::Char(' ')) {
                    match self.settings_cursor {
                        0 => self.on_key(KeyEvent::new(KeyCode::Char('T'), KeyModifiers::NONE)),
                        1 => self.reveal = !self.reveal,
                        2 => self.show_details = !self.show_details,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn key_inspect(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Right | KeyCode::Char('l') => self.focus = Focus::Fields,
            KeyCode::Left | KeyCode::Char('h') => self.focus = Focus::Files,
            KeyCode::Esc => {
                if !self.field_filter.is_empty() {
                    self.field_filter.clear();
                } else if !self.file_filter.is_empty() {
                    self.file_filter.clear();
                    self.refresh_visible();
                } else {
                    self.focus = Focus::Files;
                }
            }
            KeyCode::Char('/') => {
                let (target, buf) = match self.focus {
                    Focus::Files => (SearchTarget::Files, self.file_filter.clone()),
                    Focus::Fields => (SearchTarget::Fields, self.field_filter.clone()),
                };
                self.modal = Modal::Search { target, buf };
            }
            KeyCode::Char('s') => {
                self.sort = match self.sort {
                    SortKey::Name => SortKey::Size,
                    SortKey::Size => SortKey::Format,
                    SortKey::Format => SortKey::Warnings,
                    SortKey::Warnings => SortKey::Name,
                };
                self.refresh_visible();
            }
            KeyCode::Enter | KeyCode::Char(' ') => match self.focus {
                Focus::Files => self.focus = Focus::Fields,
                Focus::Fields => {
                    if let Some(Row::Group { name, .. }) = self.field_rows().into_iter().nth(self.field_cursor) {
                        if !self.collapsed.remove(&name) {
                            self.collapsed.insert(name);
                        }
                    }
                }
            },
            KeyCode::Char('y') => self.copy_selected(),
            KeyCode::Char('m') => {
                self.mark = self.current_index();
                self.status = "Marked for comparison. Select another file and press c.".into();
            }
            KeyCode::Char('c') => match (self.mark, self.current_index()) {
                (Some(a), Some(b)) if a != b => {
                    self.comparison = Some(compare_reports(&self.reports[a], &self.reports[b]));
                    self.compare_scroll = 0;
                    self.section = Section::Compare;
                }
                (None, _) => self.status = "Mark a file first with m.".into(),
                _ => self.status = "Pick a different file to compare against.".into(),
            },
            KeyCode::Char('e') => {
                self.edit = EditState::default();
                self.section = Section::Edit;
            }
            KeyCode::Char('x') => self.export_selected(),
            _ => {}
        }
    }

    fn copy_selected(&mut self) {
        let rows = self.field_rows();
        let Some(Row::Field(i)) = rows.into_iter().nth(self.field_cursor) else {
            self.status = "Select a field (→ then ↑/↓) to copy its value.".into();
            return;
        };
        let f = self.current().unwrap().fields[i].clone();
        if f.sensitive && !self.reveal {
            self.status = "Value is hidden: press r to reveal before copying personal/location data.".into();
            return;
        }
        self.status = format!("Copied {} to clipboard (terminal OSC 52; support varies).", f.key());
        self.clipboard = Some(f.raw);
    }

    fn export_selected(&mut self) {
        let Some(r) = self.current().cloned() else { return };
        let mut r = r;
        if !self.reveal {
            for f in r.fields.iter_mut().filter(|f| f.sensitive) {
                f.raw = "[redacted]".into();
                f.normalized = None;
                f.time = None;
            }
        }
        let stem = Path::new(&r.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let mut out = PathBuf::from(format!("{stem}.metadata.json"));
        let mut n = 2;
        while out.exists() {
            out = PathBuf::from(format!("{stem}.metadata-{n}.json"));
            n += 1;
        }
        match serde_json::to_string_pretty(&r).map_err(|e| e.to_string()).and_then(|s| std::fs::write(&out, s).map_err(|e| e.to_string())) {
            Ok(()) => {
                let m = format!("Exported {}{}", out.display(), if self.reveal { "" } else { " (personal/location values redacted)" });
                self.status = m.clone();
                self.log(m);
            }
            Err(e) => self.status = format!("Export failed: {e}"),
        }
    }

    fn key_edit(&mut self, k: KeyEvent) {
        let (keys, free) = self.edit_keys();
        match k.code {
            KeyCode::Enter => {
                if self.current().is_none() {
                    return;
                }
                if let Some(key) = keys.get(self.edit.cursor) {
                    self.modal = Modal::Input { prompt: format!("New value for {key}"), buf: String::new(), purpose: InputPurpose::EditValue(key.clone()) };
                } else if free {
                    self.modal = Modal::Input { prompt: "Field name (Group:Name or PNG keyword)".into(), buf: String::new(), purpose: InputPurpose::EditKey };
                }
            }
            KeyCode::Char('x') => {
                if let Some(key) = keys.get(self.edit.cursor).cloned() {
                    self.edit.ops.retain(|o| !matches!(o, Op::Set(x, _) | Op::Remove(x) if *x == key));
                    self.edit.ops.push(Op::Remove(key.clone()));
                    self.edit.plan = None;
                    self.status = format!("Queued removal of {key}.");
                }
            }
            KeyCode::Char('u') => {
                self.edit.ops.pop();
                self.edit.plan = None;
            }
            KeyCode::Char('p') => self.preview_edit(),
            KeyCode::Char('w') => {
                self.preview_edit();
                if self.edit.plan.is_some() {
                    self.modal = Modal::ConfirmWrite;
                }
            }
            _ => {}
        }
    }

    fn preview_edit(&mut self) {
        let Some(r) = self.current() else { return };
        let src = PathBuf::from(&r.path);
        if self.edit.ops.is_empty() {
            self.status = "No changes queued. Enter edits a field; x marks it for removal.".into();
            return;
        }
        let engine = self.engine(None);
        match plan(&engine, &src, &EditRequest { ops: self.edit.ops.clone(), profile: None }) {
            Ok(p) => {
                let mut lines = vec![format!("{} · backend {}", p.format, p.backend)];
                for c in &p.changes {
                    lines.push(format!("{} {}: {} → {}", if c.action == "set" { "~" } else { "-" }, c.key, c.before.as_deref().unwrap_or("(unset)"), c.after.as_deref().unwrap_or("(removed)")));
                }
                lines.extend(p.warnings.iter().map(|w| format!("! {w}")));
                self.edit.preview = lines;
                self.edit.plan = Some(p);
                self.status = "Preview ready. Press w to write an edited copy.".into();
            }
            Err(e) => {
                self.edit.plan = None;
                self.edit.preview = vec![format!("Cannot edit: {e}")];
                self.status = "Edit not possible.".into();
            }
        }
    }

    fn do_write(&mut self) {
        let (Some(p), Some(r)) = (self.edit.plan.clone(), self.current().cloned()) else { return };
        let src = PathBuf::from(&r.path);
        let stem = src.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let ext = src.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        let mut dest = src.with_file_name(format!("{stem}.edited{ext}"));
        let mut n = 2;
        while dest.exists() {
            dest = src.with_file_name(format!("{stem}.edited-{n}{ext}"));
            n += 1;
        }
        let engine = self.engine(None);
        match execute(&engine, &src, &dest, &p, &ApplyOpts { allow_signature_loss: false, // never auto-accepted from the TUI; use the CLI flag deliberately
                 ..Default::default() }) {
            Ok(o) => {
                let mut res = vec![format!("Wrote {} (verified: {})", o.destination, o.verified), format!("sha256 {}", o.destination_sha256)];
                res.extend(o.unexpected_changes.iter().map(|u| format!("! unexpected: {u}")));
                if let Some(n) = &o.residual_note {
                    res.push(n.clone());
                }
                self.edit.result = res;
                let rep = engine.inspect(&dest);
                self.reports.push(rep);
                self.refresh_visible();
                self.edit.ops.clear();
                self.edit.plan = None;
                let m = format!("Wrote verified copy {}", dest.display());
                self.status = m.clone();
                self.log(m);
            }
            Err(e) => {
                self.edit.result = vec![format!("Write failed, original untouched: {e}")];
                self.status = "Write failed; original untouched.".into();
            }
        }
    }
}
