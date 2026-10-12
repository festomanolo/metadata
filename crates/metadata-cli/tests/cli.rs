use assert_cmd::Command;
use metadata_testkit as kit;
use predicates::prelude::*;
use serde_json::Value;
use std::path::Path;

fn md() -> Command {
    let mut c = Command::cargo_bin("metadata").unwrap();
    c.env("METADATA_CONFIG", "/nonexistent/metadata-test-config.toml")
        .env("NO_COLOR", "1");
    c
}
fn put(dir: &Path, name: &str, data: &[u8]) -> std::path::PathBuf {
    let p = dir.join(name);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(&p, data).unwrap();
    p
}
fn json(out: &[u8]) -> Value {
    serde_json::from_slice(out).expect("stdout must be pure JSON")
}

fn sample_jpeg() -> Vec<u8> {
    kit::jpeg(
        Some(&kit::ExifSpec {
            artist: Some("Ann Lee"),
            date_time_original: Some("2023:05:01 12:00:00"),
            gps: Some((10.5, 20.5)),
            ..Default::default()
        }),
        None,
        8,
        8,
    )
}

#[test]
fn help_and_version() {
    md().arg("--help").assert().success().stdout(
        predicate::str::contains("inspect").and(predicate::str::contains("verify-manifest")),
    );
    md().arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("metadata "));
    md().arg("bogus").assert().code(2);
    md().args(["inspect"]).assert().code(2);
}

#[test]
fn extract_json_is_pure_and_versioned() {
    let d = tempfile::tempdir().unwrap();
    let p = put(d.path(), "a.jpg", &sample_jpeg());
    let out = md()
        .args(["extract", "--no-external"])
        .arg(&p)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json(&out);
    assert_eq!(v["schema_version"], "1.0.0");
    assert_eq!(v["files"][0]["format"]["id"], "jpeg");
    assert!(v["files"][0]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["name"] == "Artist" && f["raw"] == "Ann Lee"));
}

#[test]
fn redact_masks_sensitive_values() {
    let d = tempfile::tempdir().unwrap();
    let p = put(d.path(), "a.jpg", &sample_jpeg());
    let out = md()
        .args(["extract", "--redact"])
        .arg(&p)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let s = String::from_utf8(out).unwrap();
    assert!(!s.contains("Ann Lee"));
    assert!(s.contains("[redacted]"));
}

#[test]
fn human_inspect_shows_groups_and_timezone_note() {
    let d = tempfile::tempdir().unwrap();
    let p = put(d.path(), "a.jpg", &sample_jpeg());
    md().arg("inspect").arg(&p).assert().success().stdout(
        predicate::str::contains("JPEG image")
            .and(predicate::str::contains("Artist"))
            .and(predicate::str::contains("no timezone in source")),
    );
}

#[test]
fn csv_output_is_valid_and_neutralizes_formulas() {
    let d = tempfile::tempdir().unwrap();
    let p = put(
        d.path(),
        "a.png",
        &kit::png(2, 2, &[("Comment", "=HYPERLINK(\"http://x\")")]),
    );
    let out = md()
        .args(["extract", "-f", "csv"])
        .arg(&p)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let s = String::from_utf8(out).unwrap();
    assert!(s.starts_with("path,group,name,value"));
    assert!(s.contains("'=HYPERLINK"), "{s}");
}

#[test]
fn directory_requires_recursive_and_batch_reports_partial_failure() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "ok/a.txt", b"hello");
    put(d.path(), "ok/b.jpg", &sample_jpeg());
    md().args(["extract"])
        .arg(d.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("--recursive"));
    let out = md()
        .args(["batch", "--no-external"])
        .arg(d.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json(&out);
    assert_eq!(v["summary"]["files_processed"], 2);
    assert_eq!(v["summary"]["files_failed"], 0);
    // a missing path yields a partial-failure exit (3) while others still process
    let missing = d.path().join("nope.bin");
    md().args(["extract", "-f", "json"])
        .arg(d.path().join("ok/a.txt"))
        .arg(&missing)
        .assert()
        .code(3)
        .stderr(predicate::str::contains("nope.bin"));
}

#[test]
fn output_file_not_overwritten_without_force() {
    let d = tempfile::tempdir().unwrap();
    let p = put(d.path(), "a.txt", b"x");
    let out = d.path().join("r.json");
    md().args(["extract", "-o"])
        .arg(&out)
        .arg(&p)
        .assert()
        .success();
    assert!(out.exists());
    json(&std::fs::read(&out).unwrap());
    md().args(["extract", "-o"])
        .arg(&out)
        .arg(&p)
        .assert()
        .failure()
        .stderr(predicate::str::contains("--force"));
    md().args(["extract", "--force", "-o"])
        .arg(&out)
        .arg(&p)
        .assert()
        .success();
}

#[test]
fn search_distinguishes_found_absent_unsupported() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "a.jpg", &sample_jpeg());
    put(
        d.path(),
        "b.pdf",
        &kit::pdf("T", "Dee", "G", "D:2023", false),
    );
    let out = md()
        .args([
            "search",
            "-r",
            "--no-external",
            "--field",
            "GPSLatitude",
            "-f",
            "json",
        ])
        .arg(d.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json(&out);
    let by = |suffix: &str| {
        v["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["path"].as_str().unwrap().ends_with(suffix))
            .unwrap()
            .clone()
    };
    assert_eq!(by("a.jpg")["presence"], "found");
    assert_eq!(by("b.pdf")["presence"], "unsupported");
    // Author alias finds JPEG Artist and PDF Author
    let out = md()
        .args([
            "search",
            "-r",
            "--no-external",
            "--field",
            "Author",
            "-f",
            "json",
        ])
        .arg(d.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json(&out);
    assert!(v["results"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["presence"] == "found"));
    md().args(["search", "-r"]).arg(d.path()).assert().failure();
}

#[test]
fn compare_files_and_dirs() {
    let d = tempfile::tempdir().unwrap();
    let a = put(
        d.path(),
        "l/a.docx",
        &kit::docx(&kit::DocxSpec {
            creator: "Ann",
            last_modified_by: "Ann",
            created: "2024-01-01T00:00:00Z",
            modified: "2024-01-02T00:00:00Z",
            revision: 1,
            tracked_insertions: 0,
            application: "W",
        }),
    );
    let b = put(
        d.path(),
        "r/a.docx",
        &kit::docx(&kit::DocxSpec {
            creator: "Ann",
            last_modified_by: "Bob",
            created: "2024-01-01T00:00:00Z",
            modified: "2024-01-05T00:00:00Z",
            revision: 2,
            tracked_insertions: 0,
            application: "W",
        }),
    );
    let out = md()
        .args(["compare", "-f", "json", "--no-external"])
        .arg(&a)
        .arg(&b)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json(&out);
    let fields = v["comparisons"][0]["fields"].as_array().unwrap();
    let st = |k: &str| fields.iter().find(|f| f["key"] == k).unwrap()["status"].clone();
    assert_eq!(st("OOXML-Core:LastModifiedBy"), "changed");
    assert_eq!(st("OOXML-Core:Creator"), "identical");
    md().arg("compare")
        .arg(d.path().join("l"))
        .arg(d.path().join("r"))
        .arg("-r")
        .assert()
        .success()
        .stdout(predicate::str::contains("changed"));
    md().arg("compare")
        .arg(&a)
        .arg(d.path().join("l"))
        .assert()
        .failure();
}

#[test]
fn hash_manifest_verify_detects_tampering() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("sub");
    put(&dir, "a.txt", b"one");
    put(&dir, "n/b.txt", b"two");
    let m = d.path().join("manifest.json");
    md().args(["hash", "-r", "--manifest"])
        .arg(&m)
        .arg(&dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("a.txt").and(predicate::str::contains("b.txt")));
    let mv: Value = serde_json::from_slice(&std::fs::read(&m).unwrap()).unwrap();
    assert_eq!(mv["schema"], "metadata.manifest/1");
    assert_eq!(mv["entries"].as_array().unwrap().len(), 2);
    assert_eq!(mv["entries"][1]["path"], "n/b.txt");
    md().arg("verify-manifest")
        .arg(&m)
        .arg("--root")
        .arg(&dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("verification passed"));
    std::fs::write(dir.join("a.txt"), b"changed").unwrap();
    md().arg("verify-manifest")
        .arg(&m)
        .arg("--root")
        .arg(&dir)
        .assert()
        .code(1)
        .stdout(predicate::str::contains("MODIFIED"));
    std::fs::write(dir.join("a.txt"), b"one").unwrap();
    put(&dir, "extra.txt", b"x");
    md().arg("verify-manifest")
        .arg(&m)
        .arg("--root")
        .arg(&dir)
        .arg("--check-extra")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("EXTRA"));
    // editing the manifest itself is caught
    let mut v: Value = serde_json::from_slice(&std::fs::read(&m).unwrap()).unwrap();
    v["entries"][0]["hash"] = "00".into();
    std::fs::write(&m, serde_json::to_vec(&v).unwrap()).unwrap();
    md().arg("verify-manifest")
        .arg(&m)
        .arg("--root")
        .arg(&dir)
        .assert()
        .code(1)
        .stdout(predicate::str::contains("MISMATCH"));
}

#[test]
fn hash_known_vector() {
    let d = tempfile::tempdir().unwrap();
    let p = put(d.path(), "abc.txt", b"abc");
    md().arg("hash")
        .arg(&p)
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ));
    md().args(["hash", "--algorithm", "sha512"])
        .arg(&p)
        .assert()
        .success()
        .stdout(predicate::str::starts_with("ddaf35a193617aba"));
}

#[test]
fn doctor_capabilities_config() {
    md().arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("Backends").and(predicate::str::contains("native")));
    let out = md()
        .args(["capabilities", "-f", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(json(&out)["formats"].as_array().unwrap().len() > 5);
    md().args(["config", "--path"]).assert().success();
    let d = tempfile::tempdir().unwrap();
    let cfg = d.path().join("c/config.toml");
    Command::cargo_bin("metadata")
        .unwrap()
        .env("METADATA_CONFIG", &cfg)
        .args(["config", "--init"])
        .assert()
        .success();
    Command::cargo_bin("metadata")
        .unwrap()
        .env("METADATA_CONFIG", &cfg)
        .args(["config", "--init"])
        .assert()
        .failure();
}

#[test]
fn config_default_format_applies_and_flag_overrides() {
    let d = tempfile::tempdir().unwrap();
    let cfg = d.path().join("c.toml");
    std::fs::write(&cfg, "default_format = \"csv\"\n").unwrap();
    let p = put(d.path(), "a.txt", b"x");
    let run = |args: &[&str]| {
        Command::cargo_bin("metadata")
            .unwrap()
            .env("METADATA_CONFIG", &cfg)
            .args(args)
            .arg(&p)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone()
    };
    assert!(String::from_utf8(run(&["extract"]))
        .unwrap()
        .starts_with("path,group"));
    assert!(json(&run(&["extract", "-f", "json"]))["files"].is_array());
    std::fs::write(&cfg, "bogus = [").unwrap();
    Command::cargo_bin("metadata")
        .unwrap()
        .env("METADATA_CONFIG", &cfg)
        .args(["extract"])
        .arg(&p)
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid config"));
}

fn docx_at(created: &str, modified: &str) -> Vec<u8> {
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

#[test]
fn timeline_separates_sources_and_filters() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "p/a.docx",
        &docx_at("2024-01-12T09:00:00Z", "2024-02-01T09:00:00Z"),
    );
    let out = md()
        .args(["timeline", "-f", "json", "--no-git", "--no-external"])
        .arg(d.path().join("p"))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json(&out);
    let ev = v["events"].as_array().unwrap();
    assert!(ev
        .iter()
        .any(|e| e["source"] == "embedded" && e["field"] == "OOXML-Core:Created"));
    assert!(ev.iter().any(|e| e["source"] == "filesystem"));
    assert!(v["source_reliability"]["git"].is_string());
    let out = md()
        .args(["timeline", "-f", "json", "--source", "embedded"])
        .arg(d.path().join("p"))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(json(&out)["events"]
        .as_array()
        .unwrap()
        .iter()
        .all(|e| e["source"] == "embedded"));
    md().args(["timeline", "--source", "bogus"])
        .arg(d.path().join("p"))
        .assert()
        .failure();
}

#[test]
fn assess_reports_findings_without_verdicts_and_redacts() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("proj");
    put(
        &root,
        "old.docx",
        &docx_at("2023-01-01T09:00:00Z", "2024-02-01T09:00:00Z"),
    );
    put(&root, "node_modules/x.txt", b"ignored");
    let run = |extra: &[&str]| {
        md().args([
            "assess",
            "-f",
            "json",
            "--no-git",
            "--case",
            "s1",
            "--start",
            "2024-01-10",
            "--deadline",
            "2024-06-01",
            "--milestone",
            "Proposal=2024-02-01:old",
        ])
        .args(extra)
        .arg(&root)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone()
    };
    let v = json(&run(&[]));
    assert_eq!(v["case_id"], "s1");
    assert_eq!(v["integrity"]["unchanged"], true);
    assert_eq!(
        v["inventory"].as_array().unwrap().len(),
        1,
        "node_modules excluded"
    );
    let f = v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["rule"] == "TS-BEFORE-START")
        .unwrap();
    assert_eq!(f["severity"], "REVIEW");
    assert!(!v["disclaimer"].as_str().unwrap().is_empty());
    // personal identifiers masked by default
    let all = String::from_utf8(run(&["--min-severity", "info"])).unwrap();
    assert!(!all.contains("\"Ann\""));
    // human output includes the alternative explanations
    md().args(["assess", "--no-git", "--start", "2024-01-10"])
        .arg(&root)
        .assert()
        .success()
        .stdout(
            predicate::str::contains("could also be").and(predicate::str::contains(
                "does not establish academic misconduct",
            )),
        );
    md().args(["assess", "--start", "garbage"])
        .arg(&root)
        .assert()
        .failure();
    md().args([
        "assess",
        "--start",
        "2024-05-01",
        "--deadline",
        "2024-01-01",
    ])
    .arg(&root)
    .assert()
    .failure();
}

#[test]
fn edit_dry_run_then_write_verified_copy() {
    let d = tempfile::tempdir().unwrap();
    let src = put(
        d.path(),
        "a.docx",
        &docx_at("2024-01-12T09:00:00Z", "2024-02-01T09:00:00Z"),
    );
    let before = std::fs::read(&src).unwrap();
    md().args(["edit", "--dry-run", "--set", "Creator=Zed"])
        .arg(&src)
        .assert()
        .success()
        .stdout(
            predicate::str::contains("\"Ann\" → \"Zed\"").and(predicate::str::contains("dry run")),
        );
    assert!(!d.path().join("a.edited.docx").exists());
    md().args([
        "edit",
        "--set",
        "OOXML-Core:Creator=Zed",
        "--remove",
        "LastModifiedBy",
    ])
    .arg(&src)
    .assert()
    .success()
    .stdout(predicate::str::contains("verified"));
    let out = d.path().join("a.edited.docx");
    assert!(out.exists());
    assert_eq!(
        std::fs::read(&src).unwrap(),
        before,
        "original unchanged by default"
    );
    let v = json(
        &md()
            .args(["extract", "--no-external", "--filter", "creator"])
            .arg(&out)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    assert!(v["files"][0]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["raw"] == "Zed"));
    // second run refuses to overwrite the existing edited copy
    md().args(["edit", "--set", "Title=x"])
        .arg(&src)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--force"));
}

#[test]
fn edit_in_place_requires_flag_and_keeps_backup() {
    let d = tempfile::tempdir().unwrap();
    let src = put(
        d.path(),
        "a.docx",
        &docx_at("2024-01-12T09:00:00Z", "2024-02-01T09:00:00Z"),
    );
    let cfg = d.path().join("cfg/config.toml");
    let run = |args: &[&str]| {
        Command::cargo_bin("metadata")
            .unwrap()
            .env("METADATA_CONFIG", &cfg)
            .args(args)
            .arg(&src)
            .assert()
    };
    run(&["edit", "--in-place", "--set", "Title=T"])
        .code(1)
        .stderr(predicate::str::contains("--yes"));
    assert!(std::fs::read_dir(d.path()).unwrap().all(|e| !e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains("backup")));
    run(&["edit", "--in-place", "--yes", "--set", "Title=T"]).success();
    let backups: Vec<_> = std::fs::read_dir(d.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains("metadata-backup"))
        .collect();
    assert_eq!(backups.len(), 1);
    assert!(
        d.path().join("cfg/operations.jsonl").exists(),
        "operation log written"
    );
}

#[test]
fn edit_unsupported_and_invalid_inputs_fail_clearly() {
    let d = tempfile::tempdir().unwrap();
    let src = put(
        d.path(),
        "a.docx",
        &docx_at("2024-01-12T09:00:00Z", "2024-02-01T09:00:00Z"),
    );
    md().args(["edit", "--set", "EXIF:Artist=x", "--no-external"])
        .arg(&src)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("not writable"));
    md().args(["edit", "--set", "novalue"])
        .arg(&src)
        .assert()
        .failure()
        .stderr(predicate::str::contains("KEY=VALUE"));
    md().arg("edit")
        .arg(&src)
        .assert()
        .failure()
        .stderr(predicate::str::contains("nothing to do"));
    md().args([
        "edit",
        "--in-place",
        "--output-dir",
        "x",
        "--set",
        "Title=1",
    ])
    .arg(&src)
    .assert()
    .failure();
}

#[test]
fn clean_privacy_profile_on_jpeg_and_batch_partial() {
    let d = tempfile::tempdir().unwrap();
    let jpg = put(d.path(), "p.jpg", &sample_jpeg());
    let out_dir = d.path().join("out");
    md().args(["clean", "--profile", "privacy", "--output-dir"])
        .arg(&out_dir)
        .arg(&jpg)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "note: Removing fields does not guarantee",
        ));
    let v = json(
        &md()
            .args(["extract", "--no-external", "--no-hash"])
            .arg(out_dir.join("p.jpg"))
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    let names: Vec<String> = v["files"][0]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap().to_string())
        .collect();
    assert!(!names.contains(&"Artist".to_string()) && !names.iter().any(|n| n.starts_with("GPS")));
    // gps-only is not offered natively; with several files one failure -> exit 3 for mixed batch
    let txt = put(d.path(), "t.txt", b"plain");
    md().args(["clean", "--yes", "--output-dir"])
        .arg(d.path().join("o2"))
        .arg(&jpg)
        .arg(&txt)
        .assert()
        .code(3);
    assert!(
        d.path().join("o2/p.jpg").exists(),
        "successful files are still written"
    );
}

#[test]
fn edit_interactive_reads_commands() {
    let d = tempfile::tempdir().unwrap();
    let src = put(
        d.path(),
        "a.docx",
        &docx_at("2024-01-12T09:00:00Z", "2024-02-01T09:00:00Z"),
    );
    md().args(["edit", "--interactive"])
        .arg(&src)
        .write_stdin("show\nset Title Hello there\nrm LastModifiedBy\ndone\n")
        .assert()
        .success();
    let v = json(
        &md()
            .args(["extract", "--no-external"])
            .arg(d.path().join("a.edited.docx"))
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    assert!(v["files"][0]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["name"] == "Title" && f["raw"] == "Hello there"));
    md().args(["edit", "--interactive"])
        .arg(&src)
        .write_stdin("quit\n")
        .assert()
        .failure();
}

#[test]
fn copy_from_transfers_writable_fields_but_not_dates() {
    let d = tempfile::tempdir().unwrap();
    let a = put(
        d.path(),
        "src.docx",
        &docx_at("2024-01-12T09:00:00Z", "2024-02-01T09:00:00Z"),
    );
    let b = put(
        d.path(),
        "dst.docx",
        &kit::docx(&kit::DocxSpec {
            creator: "Other",
            last_modified_by: "Other",
            created: "2020-01-01T00:00:00Z",
            modified: "2020-01-02T00:00:00Z",
            revision: 9,
            tracked_insertions: 0,
            application: "X",
        }),
    );
    md().args(["edit", "--copy-from"])
        .arg(&a)
        .arg(&b)
        .assert()
        .success();
    let v = json(
        &md()
            .args(["extract", "--no-external"])
            .arg(d.path().join("dst.edited.docx"))
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    let f = v["files"][0]["fields"].as_array().unwrap().clone();
    let get = |n: &str| {
        f.iter()
            .find(|x| x["name"] == n)
            .map(|x| x["raw"].as_str().unwrap().to_string())
    };
    assert_eq!(get("Creator").as_deref(), Some("Ann"));
    assert_eq!(
        get("Created").as_deref(),
        Some("2020-01-01T00:00:00Z"),
        "dates not copied"
    );
}
