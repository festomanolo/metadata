use metadata_edit::{execute, plan, ApplyOpts, Backup, EditRequest, Op, Profile};
use metadata_engine::{Backends, Engine, Options};
use metadata_testkit as kit;
use std::path::{Path, PathBuf};

fn engine() -> Engine {
    let mut e = Engine::new(Options {
        use_external: false,
        ..Default::default()
    });
    e.backends = Backends::none();
    e
}
fn put(dir: &Path, name: &str, data: &[u8]) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, data).unwrap();
    p
}
fn set(k: &str, v: &str) -> Op {
    Op::Set(k.into(), v.into())
}
fn val(e: &Engine, p: &Path, key: &str) -> Option<String> {
    e.inspect(p).find(key).first().map(|f| f.raw.clone())
}
fn docx() -> Vec<u8> {
    kit::docx(&kit::DocxSpec {
        creator: "Ann",
        last_modified_by: "Bob",
        created: "2024-01-01T09:00:00Z",
        modified: "2024-02-01T09:00:00Z",
        revision: 3,
        tracked_insertions: 0,
        application: "Word",
    })
}
fn sha(p: &Path) -> String {
    metadata_core::hash::sha256_file(p).unwrap()
}

#[test]
fn docx_set_and_remove_roundtrip_leaves_original_untouched() {
    let d = tempfile::tempdir().unwrap();
    let src = put(d.path(), "a.docx", &docx());
    let before = sha(&src);
    let e = engine();
    let req = EditRequest {
        ops: vec![
            set("Creator", "Zed & Co"),
            set("OOXML-Core:Title", "New"),
            Op::Remove("LastModifiedBy".into()),
        ],
        profile: None,
    };
    let p = plan(&e, &src, &req).unwrap();
    assert_eq!(p.backend, "native");
    assert_eq!(p.changes[0].before.as_deref(), Some("Ann"));
    let dest = d.path().join("a.edited.docx");
    let out = execute(&e, &src, &dest, &p, &ApplyOpts::default()).unwrap();
    assert!(out.verified);
    assert_eq!(sha(&src), before, "original must be untouched");
    assert_eq!(
        val(&e, &dest, "OOXML-Core:Creator").as_deref(),
        Some("Zed & Co")
    );
    assert_eq!(val(&e, &dest, "Title").as_deref(), Some("New"));
    assert_eq!(val(&e, &dest, "LastModifiedBy"), None);
    assert_eq!(
        val(&e, &dest, "Revision").as_deref(),
        Some("3"),
        "untouched fields preserved"
    );
    assert!(
        out.unexpected_changes.is_empty(),
        "{:?}",
        out.unexpected_changes
    );
}

#[test]
fn docx_date_requires_timezone_and_validates() {
    let d = tempfile::tempdir().unwrap();
    let src = put(d.path(), "a.docx", &docx());
    let e = engine();
    let p = plan(
        &e,
        &src,
        &EditRequest {
            ops: vec![set("Created", "2024-03-01T10:00:00")],
            profile: None,
        },
    )
    .unwrap();
    let err = execute(
        &e,
        &src,
        &d.path().join("o.docx"),
        &p,
        &ApplyOpts::default(),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("timezone"), "{err}");
    assert!(!d.path().join("o.docx").exists());
    let p = plan(
        &e,
        &src,
        &EditRequest {
            ops: vec![set("Created", "2024-03-01T10:00:00+02:00")],
            profile: None,
        },
    )
    .unwrap();
    let dest = d.path().join("o2.docx");
    execute(&e, &src, &dest, &p, &ApplyOpts::default()).unwrap();
    assert_eq!(
        e.inspect(&dest).find("Created")[0]
            .time
            .as_ref()
            .unwrap()
            .utc
            .as_deref(),
        Some("2024-03-01T08:00:00Z")
    );
}

#[test]
fn unwritable_field_is_rejected_with_guidance() {
    let d = tempfile::tempdir().unwrap();
    let src = put(d.path(), "a.docx", &docx());
    let err = plan(
        &engine(),
        &src,
        &EditRequest {
            ops: vec![set("EXIF:Artist", "x")],
            profile: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("exiftool") || err.contains("not writable"),
        "{err}"
    );
    let err = plan(
        &engine(),
        &put(d.path(), "p.mp4", b"\0\0\0\x18ftypisom....more"),
        &EditRequest {
            ops: vec![set("Title", "x")],
            profile: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("cannot be written"), "{err}");
}

#[test]
fn refuses_overwrite_and_in_place_without_flags() {
    let d = tempfile::tempdir().unwrap();
    let src = put(d.path(), "a.docx", &docx());
    let e = engine();
    let p = plan(
        &e,
        &src,
        &EditRequest {
            ops: vec![set("Title", "T")],
            profile: None,
        },
    )
    .unwrap();
    assert!(execute(&e, &src, &src, &p, &ApplyOpts::default())
        .unwrap_err()
        .to_string()
        .contains("--in-place"));
    let dest = put(d.path(), "exists.docx", b"x");
    assert!(execute(&e, &src, &dest, &p, &ApplyOpts::default())
        .unwrap_err()
        .to_string()
        .contains("--force"));
    assert_eq!(std::fs::read(&dest).unwrap(), b"x");
}

#[test]
fn in_place_makes_backup_and_logs() {
    let d = tempfile::tempdir().unwrap();
    let src = put(d.path(), "a.docx", &docx());
    let before = sha(&src);
    let e = engine();
    let p = plan(
        &e,
        &src,
        &EditRequest {
            ops: vec![set("Title", "T")],
            profile: None,
        },
    )
    .unwrap();
    let log = d.path().join("logs/ops.jsonl");
    let out = execute(
        &e,
        &src,
        &src,
        &p,
        &ApplyOpts {
            replace_original: true,
            backup: Backup::Copy,
            log_path: Some(log.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    let backup = PathBuf::from(out.backup.clone().unwrap());
    assert_eq!(sha(&backup), before, "backup holds the original bytes");
    assert_eq!(val(&e, &src, "Title").as_deref(), Some("T"));
    let line: serde_json::Value = serde_json::from_str(
        std::fs::read_to_string(log)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(line["source_sha256"], before.as_str());
    assert_eq!(line["destination_sha256"], out.destination_sha256.as_str());
}

#[test]
fn signed_documents_need_explicit_acknowledgement() {
    let d = tempfile::tempdir().unwrap();
    let base = kit::zip_with(&[("[Content_Types].xml", b"<Types/>"), ("word/document.xml", b"<w/>"), ("_xmlsignatures/sig1.xml", b"<s/>"),
        ("docProps/core.xml", br#"<cp:coreProperties xmlns:cp="c" xmlns:dc="d"><dc:title>x</dc:title></cp:coreProperties>"#)], None);
    let src = put(d.path(), "s.docx", &base);
    let e = engine();
    let p = plan(
        &e,
        &src,
        &EditRequest {
            ops: vec![set("Title", "y")],
            profile: None,
        },
    )
    .unwrap();
    assert!(p.signature_risk && p.warnings.iter().any(|w| w.contains("signed")));
    let dest = d.path().join("o.docx");
    assert!(execute(&e, &src, &dest, &p, &ApplyOpts::default())
        .unwrap_err()
        .to_string()
        .contains("--accept-signature-loss"));
    assert!(!dest.exists());
    execute(
        &e,
        &src,
        &dest,
        &p,
        &ApplyOpts {
            allow_signature_loss: true,
            ..Default::default()
        },
    )
    .unwrap();
}

#[test]
fn odt_edit() {
    let d = tempfile::tempdir().unwrap();
    let src = put(d.path(), "a.odt", &kit::odt("Cy", "2023-09-09T08:00:00"));
    let e = engine();
    let p = plan(
        &e,
        &src,
        &EditRequest {
            ops: vec![set("ODF-Meta:Title", "Hello"), set("Creator", "Dee")],
            profile: None,
        },
    )
    .unwrap();
    let dest = d.path().join("b.odt");
    execute(&e, &src, &dest, &p, &ApplyOpts::default()).unwrap();
    assert_eq!(val(&e, &dest, "Title").as_deref(), Some("Hello"));
    assert_eq!(val(&e, &dest, "ODF-Meta:Creator").as_deref(), Some("Dee"));
    assert_eq!(val(&e, &dest, "InitialCreator").as_deref(), Some("Cy"));
    // mimetype must remain the first, stored entry for ODF validity
    let z = zip::ZipArchive::new(std::fs::File::open(&dest).unwrap()).unwrap();
    assert_eq!(z.file_names().next(), Some("mimetype"));
}

#[test]
fn pdf_edit_dates_and_privacy_profile() {
    let d = tempfile::tempdir().unwrap();
    let src = put(
        d.path(),
        "a.pdf",
        &kit::pdf("T", "Dee", "Gen", "D:20230501120000Z", false),
    );
    let e = engine();
    let p = plan(
        &e,
        &src,
        &EditRequest {
            ops: vec![
                set("Title", "Ünïcode ✓"),
                set("CreationDate", "2024-03-01T10:00:00+02:00"),
            ],
            profile: None,
        },
    )
    .unwrap();
    let dest = d.path().join("b.pdf");
    execute(&e, &src, &dest, &p, &ApplyOpts::default()).unwrap();
    assert_eq!(val(&e, &dest, "Title").as_deref(), Some("Ünïcode ✓"));
    assert_eq!(
        e.inspect(&dest).find("CreationDate")[0]
            .time
            .as_ref()
            .unwrap()
            .utc
            .as_deref(),
        Some("2024-03-01T08:00:00Z")
    );
    let p = plan(
        &e,
        &dest,
        &EditRequest {
            ops: vec![],
            profile: Some(Profile::Privacy),
        },
    )
    .unwrap();
    assert!(p.residual_note.is_some());
    let clean = d.path().join("c.pdf");
    execute(&e, &dest, &clean, &p, &ApplyOpts::default()).unwrap();
    assert_eq!(val(&e, &clean, "Author"), None);
    assert_eq!(
        val(&e, &clean, "Title").as_deref(),
        Some("Ünïcode ✓"),
        "privacy keeps non-personal fields"
    );
}

#[test]
fn png_text_set_remove_and_clean() {
    let d = tempfile::tempdir().unwrap();
    let src = put(
        d.path(),
        "a.png",
        &kit::png(4, 4, &[("Author", "Bob"), ("Software", "X")]),
    );
    let e = engine();
    let p = plan(
        &e,
        &src,
        &EditRequest {
            ops: vec![
                set("Author", "Eve"),
                set("Description", "héllo ✓"),
                Op::Remove("Software".into()),
            ],
            profile: None,
        },
    )
    .unwrap();
    let dest = d.path().join("b.png");
    execute(&e, &src, &dest, &p, &ApplyOpts::default()).unwrap();
    assert_eq!(val(&e, &dest, "Author").as_deref(), Some("Eve"));
    assert_eq!(val(&e, &dest, "Description").as_deref(), Some("héllo ✓"));
    assert_eq!(val(&e, &dest, "Software"), None);
    assert_eq!(val(&e, &dest, "ImageWidth").as_deref(), Some("4"));
    let p = plan(
        &e,
        &dest,
        &EditRequest {
            ops: vec![],
            profile: Some(Profile::Privacy),
        },
    )
    .unwrap();
    let c = d.path().join("c.png");
    execute(&e, &dest, &c, &p, &ApplyOpts::default()).unwrap();
    assert_eq!(val(&e, &c, "Author"), None);
    assert_eq!(val(&e, &c, "ImageHeight").as_deref(), Some("4"));
}

#[test]
fn jpeg_comment_and_privacy_strip() {
    let d = tempfile::tempdir().unwrap();
    let spec = kit::ExifSpec {
        artist: Some("Ann"),
        date_time_original: Some("2023:05:01 12:00:00"),
        gps: Some((10.5, 20.5)),
        ..Default::default()
    };
    let src = put(
        d.path(),
        "a.jpg",
        &kit::jpeg(Some(&spec), Some("old"), 8, 6),
    );
    let e = engine();
    let p = plan(
        &e,
        &src,
        &EditRequest {
            ops: vec![set("Comment", "new")],
            profile: None,
        },
    )
    .unwrap();
    let a = d.path().join("a2.jpg");
    execute(&e, &src, &a, &p, &ApplyOpts::default()).unwrap();
    assert_eq!(val(&e, &a, "Comment").as_deref(), Some("new"));
    assert_eq!(
        val(&e, &a, "Artist").as_deref(),
        Some("Ann"),
        "EXIF preserved when only the comment changes"
    );
    // dry-run plan lists every field a privacy clean would drop
    let p = plan(
        &e,
        &a,
        &EditRequest {
            ops: vec![],
            profile: Some(Profile::Privacy),
        },
    )
    .unwrap();
    assert!(
        p.changes.iter().any(|c| c.key == "EXIF:Artist")
            && p.changes.iter().any(|c| c.key.starts_with("GPS:"))
    );
    let c = d.path().join("c.jpg");
    execute(&e, &a, &c, &p, &ApplyOpts::default()).unwrap();
    let r = e.inspect(&c);
    assert!(
        r.find("Artist").is_empty()
            && r.find("GPSLatitude").is_empty()
            && r.find("Comment").is_empty()
    );
    assert_eq!(r.find("ImageWidth")[0].raw, "8");
    // gps-only needs exiftool
    assert!(plan(
        &e,
        &a,
        &EditRequest {
            ops: vec![],
            profile: Some(Profile::Gps)
        }
    )
    .is_err());
}

#[test]
fn failed_write_leaves_no_output_and_original_intact() {
    let d = tempfile::tempdir().unwrap();
    let src = put(d.path(), "bad.docx", b"PK\x03\x04 not really a zip at all");
    let e = engine();
    // not detected as docx, so planning must fail cleanly
    assert!(plan(
        &e,
        &src,
        &EditRequest {
            ops: vec![set("Title", "x")],
            profile: None
        }
    )
    .is_err());
    let leftovers: Vec<_> = std::fs::read_dir(d.path())
        .unwrap()
        .map(|x| x.unwrap().file_name())
        .collect();
    assert_eq!(leftovers.len(), 1, "{leftovers:?}");
}

#[test]
fn temp_files_are_cleaned_up_after_success() {
    let d = tempfile::tempdir().unwrap();
    let src = put(d.path(), "a.docx", &docx());
    let e = engine();
    let p = plan(
        &e,
        &src,
        &EditRequest {
            ops: vec![set("Title", "T")],
            profile: None,
        },
    )
    .unwrap();
    execute(
        &e,
        &src,
        &d.path().join("o.docx"),
        &p,
        &ApplyOpts::default(),
    )
    .unwrap();
    let names: Vec<String> = std::fs::read_dir(d.path())
        .unwrap()
        .map(|x| x.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert!(
        names.iter().all(|n| !n.starts_with(".metadata-")),
        "{names:?}"
    );
}
