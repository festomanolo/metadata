use metadata_core::{ExtensionMatch, FileReport};
use metadata_engine::{Engine, Options};
use metadata_testkit as kit;
use std::path::Path;

fn engine() -> Engine {
    Engine::new(Options {
        use_external: false,
        ..Default::default()
    })
}
fn put(dir: &Path, name: &str, data: &[u8]) -> std::path::PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, data).unwrap();
    p
}
fn get<'a>(r: &'a FileReport, key: &str) -> Option<&'a str> {
    r.find(key).first().map(|f| f.raw.as_str())
}

#[test]
fn jpeg_exif_with_offset_and_gps() {
    let d = tempfile::tempdir().unwrap();
    let spec = kit::ExifSpec {
        artist: Some("Ann Lee"),
        software: Some("TestCam 1.0"),
        date_time_original: Some("2023:05:01 12:00:00"),
        offset_time_original: Some("+02:00"),
        gps: Some((-33.5, 151.25)),
    };
    let p = put(
        d.path(),
        "a.jpg",
        &kit::jpeg(Some(&spec), Some("hello"), 640, 480),
    );
    let r = engine().inspect(&p);
    assert_eq!(r.format.id, "jpeg");
    assert_eq!(get(&r, "ImageWidth"), Some("640"));
    assert_eq!(get(&r, "Artist"), Some("Ann Lee"));
    assert_eq!(get(&r, "Comment"), Some("hello"));
    let t = r.find("DateTimeOriginal")[0].time.as_ref().unwrap();
    assert!(t.offset_known);
    assert_eq!(t.utc.as_deref(), Some("2023-05-01T10:00:00Z"));
    let pos = get(&r, "GPSPositionDecimal").expect("decimal gps");
    assert!(pos.starts_with("-33.5"), "{pos}");
    assert!(pos.contains("151.25"), "{pos}");
    assert!(r.find("Artist")[0].sensitive);
    assert!(r.sha256.is_some());
}

#[test]
fn jpeg_without_offset_is_not_assumed_utc() {
    let d = tempfile::tempdir().unwrap();
    let spec = kit::ExifSpec {
        date_time_original: Some("2023:05:01 12:00:00"),
        ..Default::default()
    };
    let r = engine().inspect(&put(d.path(), "a.jpg", &kit::jpeg(Some(&spec), None, 2, 2)));
    let t = r.find("DateTimeOriginal")[0].time.as_ref().unwrap();
    assert!(!t.offset_known);
    assert!(t.utc.is_none());
}

#[test]
fn jpeg_trailing_data_warns() {
    let d = tempfile::tempdir().unwrap();
    let mut b = kit::jpeg(None, None, 2, 2);
    b.extend(vec![0x55u8; 64]);
    let r = engine().inspect(&put(d.path(), "a.jpg", &b));
    assert!(
        r.warnings
            .iter()
            .any(|w| w.message.contains("trailing data")),
        "{:?}",
        r.warnings
    );
}

#[test]
fn png_text_chunks() {
    let d = tempfile::tempdir().unwrap();
    let r = engine().inspect(&put(
        d.path(),
        "a.png",
        &kit::png(
            10,
            20,
            &[("Author", "Bob"), ("Creation Time", "2022-01-02T03:04:05Z")],
        ),
    ));
    assert_eq!(get(&r, "ImageWidth"), Some("10"));
    assert_eq!(get(&r, "ImageHeight"), Some("20"));
    assert_eq!(get(&r, "Author"), Some("Bob"));
    assert!(
        r.find("Creation Time")[0]
            .time
            .as_ref()
            .unwrap()
            .offset_known
    );
}

#[test]
fn docx_properties_and_tracked_changes() {
    let d = tempfile::tempdir().unwrap();
    let b = kit::docx(&kit::DocxSpec {
        creator: "Ann",
        last_modified_by: "Bob",
        created: "2024-01-01T09:00:00Z",
        modified: "2024-02-01T09:00:00Z",
        revision: 7,
        tracked_insertions: 3,
        application: "Microsoft Office Word",
    });
    let r = engine().inspect(&put(d.path(), "t.docx", &b));
    assert_eq!(r.format.id, "docx");
    assert_eq!(get(&r, "Creator"), Some("Ann"));
    assert_eq!(get(&r, "LastModifiedBy"), Some("Bob"));
    assert_eq!(get(&r, "Revision"), Some("7"));
    assert_eq!(get(&r, "Application"), Some("Microsoft Office Word"));
    assert_eq!(get(&r, "TrackedInsertions"), Some("3"));
    assert_eq!(get(&r, "HasMacros"), Some("false"));
    assert!(r.find("Created")[0].time.as_ref().unwrap().offset_known);
    assert!(get(&r, "EarliestEntryTime").is_some());
}

#[test]
fn odt_meta() {
    let d = tempfile::tempdir().unwrap();
    let r = engine().inspect(&put(
        d.path(),
        "t.odt",
        &kit::odt("Cy", "2023-09-09T08:00:00"),
    ));
    assert_eq!(r.format.id, "odt");
    assert_eq!(get(&r, "InitialCreator"), Some("Cy"));
    assert_eq!(get(&r, "Generator"), Some("TestWriter/1.0"));
    assert_eq!(get(&r, "PageCount"), Some("2"));
    assert!(
        !r.find("CreationDate")[0]
            .time
            .as_ref()
            .unwrap()
            .offset_known
    );
}

#[test]
fn pdf_info_and_revisions() {
    let d = tempfile::tempdir().unwrap();
    let r = engine().inspect(&put(
        d.path(),
        "a.pdf",
        &kit::pdf("T", "Dee", "Gen", "D:20230501120000+02'00'", false),
    ));
    assert_eq!(r.format.id, "pdf");
    assert_eq!(get(&r, "Author"), Some("Dee"));
    assert_eq!(get(&r, "Revisions"), Some("1"));
    assert_eq!(get(&r, "PageCount"), Some("1"));
    assert_eq!(
        r.find("CreationDate")[0]
            .time
            .as_ref()
            .unwrap()
            .utc
            .as_deref(),
        Some("2023-05-01T10:00:00Z")
    );
    let r2 = engine().inspect(&put(
        d.path(),
        "b.pdf",
        &kit::pdf("T", "Dee", "Gen", "D:20230501120000Z", true),
    ));
    assert_eq!(get(&r2, "Revisions"), Some("2"));
    assert!(r2
        .warnings
        .iter()
        .any(|w| w.message.contains("incremental")));
}

#[test]
fn zip_listing_and_safety() {
    let d = tempfile::tempdir().unwrap();
    let b = kit::zip_with(
        &[("a.txt", b"hi"), ("../evil.txt", b"x")],
        Some((2023, 6, 7, 8, 9, 10)),
    );
    let r = engine().inspect(&put(d.path(), "a.zip", &b));
    assert_eq!(r.format.id, "zip");
    assert_eq!(get(&r, "EntryCount"), Some("2"));
    assert!(
        r.warnings
            .iter()
            .any(|w| w.message.contains("unsafe entry path")),
        "{:?}",
        r.warnings
    );
    assert_eq!(get(&r, "UnsafePathCount"), Some("1"));
    assert_eq!(
        r.find("Entry")[0].time.as_ref().unwrap().iso,
        "2023-06-07T08:09:10"
    );
}

#[test]
fn zip_bomb_is_flagged_not_expanded() {
    let d = tempfile::tempdir().unwrap();
    let zeros = vec![0u8; 8 << 20];
    let b = kit::zip_with(&[("big.bin", &zeros)], None);
    let r = engine().inspect(&put(d.path(), "bomb.zip", &b));
    assert!(
        r.warnings
            .iter()
            .any(|w| w.message.contains("compression ratio")),
        "{:?}",
        r.warnings
    );
}

#[test]
fn nested_zip_requires_deep() {
    let d = tempfile::tempdir().unwrap();
    let inner = kit::zip_with(&[("deep.txt", b"x")], None);
    let outer = kit::zip_with(&[("inner.zip", &inner)], None);
    let p = put(d.path(), "o.zip", &outer);
    assert!(engine()
        .inspect(&p)
        .fields
        .iter()
        .all(|f| !f.raw.contains("deep.txt")));
    let deep = Engine::new(Options {
        use_external: false,
        deep: true,
        ..Default::default()
    })
    .inspect(&p);
    assert!(deep.fields.iter().any(|f| f.raw == "inner.zip!/deep.txt"));
}

#[test]
fn tar_gz_entries() {
    let d = tempfile::tempdir().unwrap();
    let r = engine().inspect(&put(
        d.path(),
        "a.tar.gz",
        &kit::tar_gz(&[("src/main.rs", b"fn main(){}", 1_700_000_000)]),
    ));
    assert_eq!(r.format.id, "gzip");
    assert_eq!(get(&r, "Contains"), Some("tar"));
    let e = &r.find("Entry")[0];
    assert_eq!(e.raw, "src/main.rs");
    assert_eq!(
        e.time.as_ref().unwrap().utc.as_deref(),
        Some("2023-11-14T22:13:20Z")
    );
}

#[test]
fn extension_mismatch_detected() {
    let d = tempfile::tempdir().unwrap();
    let r = engine().inspect(&put(d.path(), "photo.pdf", &kit::jpeg(None, None, 2, 2)));
    assert_eq!(r.format.id, "jpeg");
    assert_eq!(r.format.extension_match, ExtensionMatch::Mismatch);
    assert!(r
        .warnings
        .iter()
        .any(|w| w.message.contains("extension does not match")));
}

#[test]
fn text_and_source() {
    let d = tempfile::tempdir().unwrap();
    let r = engine().inspect(&put(d.path(), "m.rs", b"fn main() {\r\n}\r\n"));
    assert_eq!(r.format.id, "source");
    assert_eq!(get(&r, "LineEndings"), Some("CRLF"));
    assert_eq!(r.format.extension_match, ExtensionMatch::Match);
    let h = engine().inspect(&put(
        d.path(),
        "p.html",
        b"<html><head><title>Hi</title><meta name=\"author\" content=\"Zed\"></head></html>",
    ));
    assert_eq!(get(&h, "Title"), Some("Hi"));
    assert_eq!(get(&h, "author"), Some("Zed"));
}

#[test]
fn malformed_inputs_never_panic_or_abort() {
    let d = tempfile::tempdir().unwrap();
    let full = kit::jpeg(
        Some(&kit::ExifSpec {
            artist: Some("A"),
            ..Default::default()
        }),
        None,
        4,
        4,
    );
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty.jpg", vec![]),
        ("trunc.jpg", full[..full.len() / 2].to_vec()),
        ("junk.pdf", b"%PDF-1.4\nnot really".to_vec()),
        ("junk.docx", b"PK\x03\x04garbage garbage garbage".to_vec()),
        ("junk.zip", b"PK\x05\x06".to_vec()),
        ("trunc.png", kit::png(1, 1, &[])[..30].to_vec()),
        (
            "rand.bin",
            (0..5000u32)
                .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
                .collect(),
        ),
        ("bad.gz", b"\x1f\x8bnope".to_vec()),
    ];
    for (n, b) in cases {
        let r = engine().inspect(&put(d.path(), n, &b));
        assert!(r.error.is_none(), "{n}: {:?}", r.error);
    }
}

#[test]
fn missing_file_reports_error() {
    let r = engine().inspect(Path::new("/definitely/not/here.jpg"));
    assert!(r.error.is_some());
}

#[test]
fn inspecting_does_not_modify_file() {
    let d = tempfile::tempdir().unwrap();
    let p = put(d.path(), "a.pdf", &kit::pdf("T", "A", "G", "D:2023", false));
    let before = metadata_core::hash::sha256_file(&p).unwrap();
    let md = std::fs::metadata(&p).unwrap().modified().unwrap();
    engine().inspect(&p);
    assert_eq!(metadata_core::hash::sha256_file(&p).unwrap(), before);
    assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), md);
}

#[test]
fn parallel_scan_matches_sequential() {
    let d = tempfile::tempdir().unwrap();
    for i in 0..40 {
        put(
            d.path(),
            &format!("f{i}.txt"),
            format!("file {i}").as_bytes(),
        );
    }
    let e = engine();
    let files =
        metadata_engine::collect_files(&[d.path().to_path_buf()], &Default::default()).files;
    let seq = e.inspect_many(&files, 1, &|_, _| {});
    let par = e.inspect_many(&files, 8, &|_, _| {});
    assert_eq!(seq.len(), 40);
    assert_eq!(
        seq.iter().map(|r| &r.sha256).collect::<Vec<_>>(),
        par.iter().map(|r| &r.sha256).collect::<Vec<_>>()
    );
}

#[test]
fn cancellation_stops_scan() {
    use std::sync::{atomic::AtomicBool, Arc};
    let d = tempfile::tempdir().unwrap();
    for i in 0..20 {
        put(d.path(), &format!("f{i}.txt"), b"x");
    }
    let flag = Arc::new(AtomicBool::new(true));
    let e = Engine::new(Options {
        use_external: false,
        cancel: Some(flag),
        ..Default::default()
    });
    let files =
        metadata_engine::collect_files(&[d.path().to_path_buf()], &Default::default()).files;
    assert!(e.inspect_many(&files, 4, &|_, _| {}).is_empty());
}

#[cfg(unix)]
#[test]
fn symlinks_are_not_followed() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "real.txt", b"x");
    std::os::unix::fs::symlink(d.path().join("real.txt"), d.path().join("link.txt")).unwrap();
    let c = metadata_engine::collect_files(&[d.path().to_path_buf()], &Default::default());
    assert_eq!(c.files.len(), 1);
    assert_eq!(c.skipped.len(), 1);
}
