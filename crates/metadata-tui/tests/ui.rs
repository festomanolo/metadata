use metadata_engine::{collect_files, Engine, Options, ScanOptions};
use metadata_testkit as kit;
use metadata_tui::app::{App, Focus, Modal, Section};
use metadata_tui::ui;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use std::path::Path;

fn key(c: KeyCode) -> KeyEvent {
    KeyEvent::new(c, KeyModifiers::NONE)
}
fn ch(c: char) -> KeyEvent {
    key(KeyCode::Char(c))
}
fn screen(app: &App, w: u16, h: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| ui::draw(f, app)).unwrap();
    let b = t.backend().buffer().clone();
    (0..b.area.height)
        .map(|y| {
            (0..b.area.width)
                .map(|x| b[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn fixture() -> (tempfile::TempDir, App) {
    let d = tempfile::tempdir().unwrap();
    let w = |n: &str, b: &[u8]| std::fs::write(d.path().join(n), b).unwrap();
    w(
        "photo.jpg",
        &kit::jpeg(
            Some(&kit::ExifSpec {
                artist: Some("Ann Lee"),
                date_time_original: Some("2023:05:01 12:00:00"),
                gps: Some((10.5, 20.5)),
                ..Default::default()
            }),
            None,
            8,
            8,
        ),
    );
    w(
        "thesis.docx",
        &kit::docx(&kit::DocxSpec {
            creator: "Ann",
            last_modified_by: "Bob",
            created: "2024-01-01T09:00:00Z",
            modified: "2024-02-01T09:00:00Z",
            revision: 2,
            tracked_insertions: 0,
            application: "W",
        }),
    );
    w("notes.txt", b"hello");
    w("fake.pdf", &kit::jpeg(None, None, 2, 2));
    let e = Engine::new(Options {
        use_external: false,
        ..Default::default()
    });
    let files = collect_files(&[d.path().to_path_buf()], &ScanOptions::default()).files;
    let reports = e.inspect_many(&files, 2, &|_, _| {});
    let mut app = App::new(d.path().to_path_buf(), false);
    app.set_reports(reports);
    (d, app)
}

#[test]
fn renders_all_regions_at_80x24_and_120x40() {
    let (_d, app) = fixture();
    for (w, h) in [(80, 24), (120, 40)] {
        let s = screen(&app, w, h);
        assert!(s.contains("METADATA"), "header");
        for label in [
            "Overview",
            "Inspect",
            "Scan",
            "Timeline",
            "Compare",
            "Edit",
            "Cases",
            "Reports",
            "Settings",
        ] {
            assert!(s.contains(label), "nav {label} at {w}x{h}");
        }
        assert!(
            s.contains("Files examined") && s.contains("4"),
            "real stats"
        );
        assert!(s.contains("help"), "footer hints");
    }
}

#[test]
fn small_terminal_degrades_gracefully() {
    let (_d, app) = fixture();
    let s = screen(&app, 40, 10);
    assert!(s.contains("too small") && s.contains("60×14"));
}

#[test]
fn overview_statistics_are_computed_not_hardcoded() {
    let (_d, app) = fixture();
    let st = metadata_tui::app::Stats::compute(&app.reports);
    assert_eq!(st.files, 4);
    assert!(st.needs_review >= 1, "fake.pdf has an extension mismatch");
    assert_eq!(st.formats.get("jpeg"), Some(&2));
    let empty = App::new(std::path::PathBuf::from("."), false);
    assert!(screen(&empty, 100, 30).contains("No files loaded"));
}

#[test]
fn tab_and_number_navigation() {
    let (_d, mut app) = fixture();
    assert_eq!(app.section, Section::Overview);
    app.on_key(key(KeyCode::Tab));
    assert_eq!(app.section, Section::Inspect);
    app.on_key(key(KeyCode::BackTab));
    app.on_key(key(KeyCode::BackTab));
    assert_eq!(app.section, Section::Settings, "wraps backwards");
    app.on_key(ch('3'));
    assert_eq!(app.section, Section::Scan);
}

#[test]
fn inspect_select_expand_collapse_and_sensitive_hiding() {
    let (_d, mut app) = fixture();
    app.on_key(ch('2'));
    // files sort by name: fake.pdf, notes.txt, photo.jpg, thesis.docx
    app.on_key(ch('j'));
    app.on_key(ch('j'));
    assert!(app.current().unwrap().path.ends_with("photo.jpg"));
    let s = screen(&app, 140, 40);
    assert!(s.contains("EXIF") && s.contains("Artist"));
    assert!(
        !s.contains("Ann Lee"),
        "personal value hidden until revealed"
    );
    assert!(s.contains("hidden"));
    app.on_key(ch('r'));
    assert!(screen(&app, 140, 40).contains("Ann Lee"));
    app.on_key(ch('r'));
    // collapse a group
    app.on_key(key(KeyCode::Right));
    assert_eq!(app.focus, Focus::Fields);
    let before = app.field_rows().len();
    app.on_key(key(KeyCode::Enter));
    assert!(app.field_rows().len() < before, "group collapsed");
    app.on_key(key(KeyCode::Enter));
    assert_eq!(app.field_rows().len(), before);
}

#[test]
fn search_filters_files_and_fields() {
    let (_d, mut app) = fixture();
    app.on_key(ch('2'));
    app.on_key(ch('/'));
    for c in "docx".chars() {
        app.on_key(ch(c));
    }
    app.on_key(key(KeyCode::Enter));
    assert_eq!(app.visible.len(), 1);
    app.on_key(key(KeyCode::Esc));
    assert_eq!(app.visible.len(), 4, "Esc clears the filter");
    app.on_key(ch('j'));
    app.on_key(ch('j'));
    app.on_key(ch('j'));
    app.on_key(key(KeyCode::Right));
    app.on_key(ch('/'));
    for c in "revision".chars() {
        app.on_key(ch(c));
    }
    app.on_key(key(KeyCode::Enter));
    let rows = app.field_rows();
    assert_eq!(rows.len(), 2, "one group header + one field");
}

#[test]
fn copy_requires_reveal_for_sensitive_values() {
    let (_d, mut app) = fixture();
    app.on_key(ch('2'));
    app.on_key(ch('j'));
    app.on_key(ch('j'));
    app.on_key(key(KeyCode::Right));
    app.on_key(ch('/'));
    for c in "artist".chars() {
        app.on_key(ch(c));
    }
    app.on_key(key(KeyCode::Enter));
    app.on_key(key(KeyCode::Down)); // onto the field row
    app.on_key(ch('y'));
    assert!(app.clipboard.is_none());
    app.on_key(ch('r'));
    app.on_key(ch('y'));
    assert_eq!(app.clipboard.as_deref(), Some("Ann Lee"));
}

#[test]
fn compare_flow_and_timeline_render() {
    let (_d, mut app) = fixture();
    app.on_key(ch('2'));
    app.on_key(ch('j')); // notes.txt
    app.on_key(ch('m'));
    app.on_key(ch('j')); // photo.jpg
    app.on_key(ch('c'));
    assert_eq!(app.section, Section::Compare);
    assert!(app.comparison.is_some());
    assert!(screen(&app, 120, 30).contains("changed"));
    app.on_key(ch('5')); // compare section stays reachable; go to timeline
    app.on_key(ch('4'));
    let s = screen(&app, 120, 30);
    assert!(s.contains("Timeline") && s.contains("embedded") && s.contains("filesystem"));
    assert!(
        s.contains("?"),
        "no-timezone marker shown for naive timestamps"
    );
}

#[test]
fn help_overlay_and_modal_escape() {
    let (_d, mut app) = fixture();
    app.on_key(ch('?'));
    assert_eq!(app.modal, Modal::Help);
    assert!(screen(&app, 100, 30).contains("Ctrl+C"));
    app.on_key(key(KeyCode::Esc));
    assert_eq!(app.modal, Modal::None);
}

#[test]
fn quit_rules() {
    let (_d, mut app) = fixture();
    app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(!app.should_quit, "first Ctrl+C when idle only arms");
    app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(app.should_quit);
    let (_d, mut app) = fixture();
    app.on_key(ch('q'));
    assert!(app.should_quit);
}

#[test]
fn background_scan_reports_progress_and_can_cancel() {
    let d = tempfile::tempdir().unwrap();
    for i in 0..30 {
        std::fs::write(d.path().join(format!("f{i}.txt")), format!("file {i}")).unwrap();
    }
    let mut app = App::new(d.path().to_path_buf(), false);
    app.start_scan(d.path().to_path_buf());
    assert!(app.scan.is_some());
    app.on_key(ch('q'));
    assert!(!app.should_quit, "cannot quit mid-scan");
    for _ in 0..200 {
        app.poll();
        if app.scan.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert!(app.scan.is_none());
    assert_eq!(app.reports.len(), 30);
    assert_eq!(app.visible.len(), 30);
    // cancellation flag stops a scan
    let mut app = App::new(d.path().to_path_buf(), false);
    app.start_scan(d.path().to_path_buf());
    app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    for _ in 0..200 {
        app.poll();
        if app.scan.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert!(app.scan.is_none(), "scan terminates after cancel");
}

#[test]
fn edit_flow_writes_verified_copy_and_leaves_original() {
    let (d, mut app) = fixture();
    app.on_key(ch('2'));
    for _ in 0..3 {
        app.on_key(ch('j'));
    }
    assert!(app.current().unwrap().path.ends_with("thesis.docx"));
    let original = std::fs::read(d.path().join("thesis.docx")).unwrap();
    app.on_key(ch('e'));
    assert_eq!(app.section, Section::Edit);
    assert!(screen(&app, 120, 30).contains("OOXML-Core:Title"));
    app.on_key(key(KeyCode::Enter));
    assert!(matches!(app.modal, Modal::Input { .. }));
    for c in "Final Thesis".chars() {
        app.on_key(ch(c));
    }
    app.on_key(key(KeyCode::Enter));
    app.on_key(ch('p'));
    assert!(app.edit.preview.iter().any(|l| l.contains("Final Thesis")));
    app.on_key(ch('w'));
    assert_eq!(app.modal, Modal::ConfirmWrite);
    app.on_key(ch('y'));
    assert!(d.path().join("thesis.edited.docx").exists());
    assert_eq!(
        std::fs::read(d.path().join("thesis.docx")).unwrap(),
        original
    );
    assert!(app.edit.result.iter().any(|l| l.contains("verified: true")));
    assert_eq!(app.reports.len(), 5, "edited copy added to the session");
}

#[test]
fn export_redacts_unless_revealed() {
    let (_d, mut app) = fixture();
    let cwd = std::env::current_dir().unwrap();
    let out = tempfile::tempdir().unwrap();
    std::env::set_current_dir(out.path()).unwrap();
    app.on_key(ch('2'));
    app.on_key(ch('j'));
    app.on_key(ch('j'));
    app.on_key(ch('x'));
    let text = std::fs::read_to_string(Path::new("photo.jpg.metadata.json")).unwrap();
    std::env::set_current_dir(cwd).unwrap();
    assert!(!text.contains("Ann Lee") && text.contains("[redacted]"));
}

#[test]
fn themes_render_without_panicking() {
    let (_d, mut app) = fixture();
    for _ in 0..3 {
        app.on_key(KeyEvent::new(KeyCode::Char('T'), KeyModifiers::SHIFT));
        for s in [1, 2, 3, 4, 5, 6, 7, 8, 9] {
            app.on_key(ch(char::from_digit(s, 10).unwrap()));
            let _ = screen(&app, 90, 26);
        }
    }
}
