//! ExifTool write backend: builds a strict argument vector (no shell) and writes to a *separate* output file (`-o`),
//! so ExifTool never creates `_original` backups and never touches the source.
use crate::native::{Resolved, WProfile};
use metadata_core::{MetaError, Result};
use metadata_engine::backends::run_capped;
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

pub fn tag_ok(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':' | '*'))
        && !key.starts_with('-')
}

pub fn value_ok(v: &str) -> bool {
    !v.chars().any(|c| c.is_control() && c != '\t') && v.len() <= 65_536
}

pub fn build_args(
    ops: &[Resolved],
    profile: Option<WProfile>,
    src: &Path,
    out: &Path,
) -> Result<Vec<OsString>> {
    let mut a: Vec<OsString> = vec!["-q".into(), "-P".into()];
    match profile {
        Some(WProfile::All) => a.push("-all=".into()),
        Some(WProfile::Privacy) => {
            // Strip everything but keep image orientation and color profile so the picture still displays correctly.
            a.extend(
                [
                    "-all=",
                    "-tagsFromFile",
                    "@",
                    "-Orientation",
                    "-ICC_Profile",
                ]
                .map(OsString::from),
            );
        }
        Some(WProfile::Gps) => a.push("-gps:all=".into()),
        None => {}
    }
    for o in ops {
        if !tag_ok(&o.key) {
            return Err(MetaError::InvalidArgument(format!(
                "'{}' is not a valid tag name",
                o.key
            )));
        }
        match &o.value {
            Some(v) if !value_ok(v) => {
                return Err(MetaError::InvalidArgument(format!(
                    "value for {} contains control characters or is too long",
                    o.key
                )))
            }
            Some(v) => a.push(format!("-{}={}", o.key, v).into()),
            None => a.push(format!("-{}=", o.key).into()),
        }
    }
    a.push("-o".into());
    a.push(out.as_os_str().to_owned());
    a.push("--".into());
    a.push(src.as_os_str().to_owned());
    Ok(a)
}

pub fn run(exe: &Path, args: &[OsString], timeout: Duration) -> Result<()> {
    let refs: Vec<&std::ffi::OsStr> = args.iter().map(|a| a.as_os_str()).collect();
    let out = run_capped(exe, &refs, timeout, 1 << 20)?;
    let text = String::from_utf8_lossy(&out);
    if text.contains("Error:") {
        return Err(MetaError::BackendFailed {
            backend: "exiftool".into(),
            reason: text.trim().to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn r(k: &str, v: Option<&str>) -> Resolved {
        Resolved {
            key: k.into(),
            value: v.map(String::from),
        }
    }
    #[test]
    fn args_are_a_vector_and_validated() {
        let a = build_args(
            &[r("EXIF:Artist", Some("A; rm -rf /")), r("XMP:Title", None)],
            Some(WProfile::Gps),
            Path::new("in.jpg"),
            Path::new("out.jpg"),
        )
        .unwrap();
        let s: Vec<String> = a.iter().map(|x| x.to_string_lossy().to_string()).collect();
        assert!(
            s.contains(&"-EXIF:Artist=A; rm -rf /".to_string()),
            "value stays one argument: {s:?}"
        );
        assert!(s.contains(&"-XMP:Title=".to_string()) && s.contains(&"-gps:all=".to_string()));
        assert_eq!(&s[s.len() - 4..], ["-o", "out.jpg", "--", "in.jpg"]);
        assert!(build_args(
            &[r("-evil", Some("x"))],
            None,
            Path::new("a"),
            Path::new("b")
        )
        .is_err());
        assert!(build_args(
            &[r("Artist", Some("a\0b"))],
            None,
            Path::new("a"),
            Path::new("b")
        )
        .is_err());
    }
}
