//! Exercises the ExifTool code path with a *stub* executable (unix only). This verifies our plumbing
//! (argument vector, `-o` output, JSON merge, verify-after-write), not real ExifTool behaviour.
#![cfg(unix)]
use metadata_edit::{execute, plan, ApplyOpts, EditRequest, Op, Profile};
use metadata_engine::{Backends, Engine, Options};
use metadata_testkit as kit;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const STUB: &str = r#"#!/bin/sh
# stub exiftool: -ver | -j (read) | write with -o
out=""; src=""; read=0; tags=""; prev=""
for a in "$@"; do
  case "$a" in
    -ver) echo "12.99"; exit 0;;
    -j) read=1;;
    -o) ;;
    --) ;;
    -*=*) tags="$tags
${a#-}";;
    -gps:all=) tags="$tags
GPSREMOVE";;
  esac
  if [ "$prev" = "-o" ]; then out="$a"; fi
  prev="$a"; src="$a"
done
if [ "$read" = "1" ]; then
  printf '[{"SourceFile":"%s"' "$src"
  grep -a '^STUBTAG:' "$src" | sed 's/^STUBTAG://' | while IFS='=' read -r k v; do printf ',"%s":"%s"' "$k" "$v"; done
  printf '}]\n'
  exit 0
fi
cp "$src" "$out" || { echo "Error: cannot copy" ; exit 1; }
echo >> "$out"
printf '%s\n' "$tags" | while IFS= read -r l; do [ -n "$l" ] && echo "STUBTAG:$l" >> "$out"; done
exit 0
"#;

fn stub_engine(dir: &Path) -> (Engine, PathBuf) {
    let exe = dir.join("exiftool");
    std::fs::write(&exe, STUB).unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut e = Engine::new(Options {
        use_external: true,
        ..Default::default()
    });
    e.backends = Backends {
        exiftool: Some((exe.clone(), "12.99".into())),
        ffprobe: None,
    };
    (e, exe)
}

#[test]
fn exiftool_backend_writes_verifies_and_leaves_original() {
    let d = tempfile::tempdir().unwrap();
    let (e, _) = stub_engine(d.path());
    let src = d.path().join("a.jpg");
    std::fs::write(&src, kit::jpeg(None, None, 4, 4)).unwrap();
    let before = std::fs::read(&src).unwrap();
    // EXIF:Artist is not natively writable for JPEG → must route to exiftool
    let p = plan(
        &e,
        &src,
        &EditRequest {
            ops: vec![Op::Set("EXIF:Artist".into(), "Zed; $(touch pwned)".into())],
            profile: None,
        },
    )
    .unwrap();
    assert_eq!(p.backend, "exiftool");
    assert!(p.warnings.iter().any(|w| w.contains("exiftool")));
    let dest = d.path().join("b.jpg");
    let out = execute(&e, &src, &dest, &p, &ApplyOpts::default()).unwrap();
    assert!(out.verified);
    assert_eq!(std::fs::read(&src).unwrap(), before);
    assert!(
        !d.path().join("pwned").exists(),
        "values are never shell-interpreted"
    );
    assert!(!d.path().join("scratch").exists());
    let r = e.inspect(&dest);
    assert_eq!(r.find("Artist")[0].raw, "Zed; $(touch pwned)");
    assert!(r.backends.iter().any(|b| b.name == "exiftool"));
    // leftover scratch dirs must be cleaned
    let leftovers: Vec<_> = std::fs::read_dir(d.path())
        .unwrap()
        .map(|x| x.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with(".tmp") || n.starts_with(".metadata-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn exiftool_failure_surfaces_and_original_survives() {
    let d = tempfile::tempdir().unwrap();
    let (mut e, exe) = stub_engine(d.path());
    // a stub that always fails on write
    std::fs::write(
        &exe,
        "#!/bin/sh\ncase \"$1\" in -ver) echo 1.0; exit 0;; esac\necho 'Error: boom'\nexit 1\n",
    )
    .unwrap();
    e.backends.exiftool = Some((exe, "1.0".into()));
    let src = d.path().join("a.jpg");
    std::fs::write(&src, kit::jpeg(None, None, 4, 4)).unwrap();
    let before = std::fs::read(&src).unwrap();
    let p = plan(
        &e,
        &src,
        &EditRequest {
            ops: vec![Op::Set("EXIF:Artist".into(), "x".into())],
            profile: None,
        },
    )
    .unwrap();
    let err = execute(&e, &src, &d.path().join("b.jpg"), &p, &ApplyOpts::default())
        .unwrap_err()
        .to_string();
    assert!(err.contains("boom") || err.contains("exiftool"), "{err}");
    assert_eq!(std::fs::read(&src).unwrap(), before);
    assert!(!d.path().join("b.jpg").exists());
}

#[test]
fn gps_profile_is_routed_to_exiftool() {
    let d = tempfile::tempdir().unwrap();
    let (e, _) = stub_engine(d.path());
    let src = d.path().join("a.jpg");
    std::fs::write(&src, kit::jpeg(None, None, 4, 4)).unwrap();
    let p = plan(&e, &src, &EditRequest { ops: vec![], profile: Some(Profile::Gps) }).unwrap();
    assert_eq!(p.backend, "exiftool");
}
