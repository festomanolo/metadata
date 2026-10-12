//! External backend discovery and safe subprocess execution (no shell, timeout, output cap).
use crate::{Extraction, Options};
use metadata_core::{time, BackendUsed, Category, Field, FormatInfo, MetaError, Provenance, Result};
use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize)]
pub struct BackendStatus {
    pub name: String,
    pub available: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub purpose: String,
    pub install_hint: String,
}

#[derive(Debug, Clone)]
pub struct Backends {
    pub exiftool: Option<(PathBuf, String)>,
    pub ffprobe: Option<(PathBuf, String)>,
}

pub fn find_in_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) { vec![".exe".into(), ".cmd".into(), ".bat".into(), String::new()] } else { vec![String::new()] };
    for dir in std::env::split_paths(&paths) {
        for e in &exts {
            let c = dir.join(format!("{name}{e}"));
            if c.is_file() { return Some(c); }
        }
    }
    None
}

/// Run a program with an argument vector, bounded time and output.
pub fn run_capped(program: &Path, args: &[&std::ffi::OsStr], timeout: Duration, max_out: usize) -> Result<Vec<u8>> {
    let name = program.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| MetaError::BackendFailed { backend: name.clone(), reason: format!("spawn failed: {e}") })?;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        let mut over = false;
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if buf.len() + n > max_out { over = true; break; }
                    buf.extend_from_slice(&chunk[..n]);
                }
            }
        }
        (buf, over)
    });
    let err_reader = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = (&mut stderr).take(16 * 1024).read_to_end(&mut s);
        s
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(MetaError::BackendFailed { backend: name, reason: format!("timed out after {}s", timeout.as_secs()) });
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(MetaError::BackendFailed { backend: name, reason: e.to_string() }),
        }
    };
    let (out, over) = reader.join().unwrap_or_default();
    let err = err_reader.join().unwrap_or_default();
    if over {
        let _ = child.kill();
        return Err(MetaError::LimitExceeded(format!("{name} output exceeded {max_out} bytes")));
    }
    if !status.success() && out.is_empty() {
        return Err(MetaError::BackendFailed { backend: name, reason: String::from_utf8_lossy(&err).trim().to_string() });
    }
    Ok(out)
}

fn probe_version(path: &Path, args: &[&str]) -> Option<String> {
    let a: Vec<&std::ffi::OsStr> = args.iter().map(std::ffi::OsStr::new).collect();
    let out = run_capped(path, &a, Duration::from_secs(10), 64 * 1024).ok()?;
    Some(String::from_utf8_lossy(&out).lines().next()?.trim().to_string())
}

impl Backends {
    /// Locate tools. `METADATA_EXIFTOOL` / `METADATA_FFPROBE` override PATH lookup.
    pub fn discover() -> Self {
        let find = |env: &str, name: &str| std::env::var_os(env).map(PathBuf::from).filter(|p| p.is_file()).or_else(|| find_in_path(name));
        let exiftool = find("METADATA_EXIFTOOL", "exiftool").and_then(|p| probe_version(&p, &["-ver"]).map(|v| (p, v)));
        let ffprobe = find("METADATA_FFPROBE", "ffprobe").and_then(|p| {
            probe_version(&p, &["-version"]).map(|v| (p, v.split_whitespace().nth(2).unwrap_or("?").to_string()))
        });
        Backends { exiftool, ffprobe }
    }

    pub fn none() -> Self { Backends { exiftool: None, ffprobe: None } }

    pub fn status(&self) -> Vec<BackendStatus> {
        let mk = |name: &str, b: &Option<(PathBuf, String)>, purpose: &str, hint: &str| BackendStatus {
            name: name.into(), available: b.is_some(), version: b.as_ref().map(|x| x.1.clone()),
            path: b.as_ref().map(|x| x.0.display().to_string()), purpose: purpose.into(), install_hint: hint.into(),
        };
        vec![
            BackendStatus { name: "native".into(), available: true, version: Some(env!("CARGO_PKG_VERSION").into()), path: None,
                purpose: "Built-in parsers: EXIF/IPTC/XMP/ICC, PNG/JPEG/GIF/BMP/WebP, OOXML/ODF/EPUB, PDF, ZIP/TAR/GZIP, text".into(), install_hint: String::new() },
            mk("exiftool", &self.exiftool, "Broad read support (RAW, video, audio, HEIC, many documents) and tag writing",
               "macOS: brew install exiftool | Debian/Ubuntu: apt install libimage-exiftool-perl | Windows: winget install OliverBetz.ExifTool (or exiftool.org)"),
            mk("ffprobe", &self.ffprobe, "Audio/video container and stream analysis",
               "macOS: brew install ffmpeg | Debian/Ubuntu: apt install ffmpeg | Windows: winget install Gyan.FFmpeg"),
        ]
    }

    pub fn extract_external(&self, path: &Path, fmt: &FormatInfo, opts: &Options, ex: &mut Extraction, used: &mut Vec<BackendUsed>) {
        let timeout = Duration::from_secs(opts.timeout_secs);
        let wants_ffprobe = matches!(fmt.category, Category::Video | Category::Audio);
        if let Some((exe, ver)) = &self.exiftool {
            if fmt.id != "binary" && !matches!(fmt.category, Category::Source | Category::Text) {
                let args: Vec<&std::ffi::OsStr> = ["-j", "-G1", "-a", "-u", "-struct", "--"].iter().map(std::ffi::OsStr::new).chain(std::iter::once(path.as_os_str())).collect();
                match run_capped(exe, &args, timeout, opts.limits.max_tool_output).and_then(|o| parse_exiftool(&o, opts.all, ex)) {
                    Ok(()) => used.push(BackendUsed { name: "exiftool".into(), version: ver.clone() }),
                    Err(e) => ex.warn("exiftool", e.to_string()),
                }
            }
        } else if matches!(fmt.id.as_str(), "heic" | "ole2" | "raw-raf" | "raw-cr3" | "tiff") || (matches!(fmt.category, Category::Video | Category::Audio) && self.ffprobe.is_none()) {
            ex.unavailable.push(format!("detailed {} metadata requires exiftool (not installed)", fmt.id));
        }
        if wants_ffprobe {
            if let Some((exe, ver)) = &self.ffprobe {
                let args: Vec<&std::ffi::OsStr> = ["-v", "error", "-print_format", "json", "-show_format", "-show_streams", "-show_chapters", "--"].iter().map(std::ffi::OsStr::new).chain(std::iter::once(path.as_os_str())).collect();
                match run_capped(exe, &args, timeout, opts.limits.max_tool_output).and_then(|o| parse_ffprobe(&o, ex)) {
                    Ok(()) => used.push(BackendUsed { name: "ffprobe".into(), version: ver.clone() }),
                    Err(e) => ex.warn("ffprobe", e.to_string()),
                }
            } else {
                ex.unavailable.push("audio/video stream analysis requires ffprobe (not installed)".into());
            }
        }
    }
}

fn val_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(a) => a.iter().map(val_to_string).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

pub fn parse_exiftool(out: &[u8], all: bool, ex: &mut Extraction) -> Result<()> {
    let v: serde_json::Value = serde_json::from_slice(out).map_err(|e| MetaError::BackendFailed { backend: "exiftool".into(), reason: format!("bad JSON: {e}") })?;
    let obj = v.get(0).and_then(|o| o.as_object()).ok_or_else(|| MetaError::BackendFailed { backend: "exiftool".into(), reason: "empty result".into() })?;
    for (k, val) in obj {
        let Some((group, name)) = k.split_once(':') else { continue };
        if group == "System" || group == "ExifTool" || group == "SourceFile" { 
            if group == "ExifTool" && name == "Warning" { ex.warn("exiftool", val_to_string(val)); }
            continue;
        }
        if group == "File" && matches!(name, "FileName" | "Directory" | "FileSize" | "FileModifyDate" | "FileAccessDate" | "FileInodeChangeDate" | "FilePermissions" | "FileCreateDate") { continue; }
        let raw = val_to_string(val);
        if !all && raw.starts_with("(Binary data") { continue; }
        let mut f = Field::new(group, name, raw.clone(), "exiftool", Provenance::Stored);
        if name.contains("Date") || name.contains("Time") && !name.contains("Offset") { f = f.with_time(time::parse_any(&raw)); }
        ex.fields.push(f);
    }
    Ok(())
}

pub fn parse_ffprobe(out: &[u8], ex: &mut Extraction) -> Result<()> {
    let v: serde_json::Value = serde_json::from_slice(out).map_err(|e| MetaError::BackendFailed { backend: "ffprobe".into(), reason: format!("bad JSON: {e}") })?;
    let mut add = |group: &str, name: &str, val: &serde_json::Value, prov: Provenance| {
        let raw = val_to_string(val);
        if raw.is_empty() || raw == "null" { return; }
        let mut f = Field::new(group, name, raw.clone(), "ffprobe", prov);
        if name.eq_ignore_ascii_case("creation_time") { f = f.with_time(time::parse_iso(&raw)); }
        ex.fields.push(f);
    };
    if let Some(fm) = v.get("format").and_then(|f| f.as_object()) {
        for k in ["format_name", "format_long_name", "duration", "bit_rate", "nb_streams", "probe_score"] {
            if let Some(x) = fm.get(k) { add("Format", k, x, Provenance::Inferred); }
        }
        if let Some(t) = fm.get("tags").and_then(|t| t.as_object()) {
            for (k, x) in t { add("Format-Tags", k, x, Provenance::Stored); }
        }
    }
    for (i, s) in v.get("streams").and_then(|s| s.as_array()).into_iter().flatten().enumerate() {
        let kind = s.get("codec_type").and_then(|x| x.as_str()).unwrap_or("data");
        let group = format!("Stream{i}-{kind}");
        for k in ["codec_name", "codec_long_name", "profile", "width", "height", "pix_fmt", "r_frame_rate", "avg_frame_rate", "sample_rate", "channels", "channel_layout", "bit_rate", "duration", "nb_frames", "bits_per_sample"] {
            if let Some(x) = s.get(k) { add(&group, k, x, Provenance::Inferred); }
        }
        if let Some(t) = s.get("tags").and_then(|t| t.as_object()) {
            for (k, x) in t { add(&format!("{group}-Tags"), k, x, Provenance::Stored); }
        }
    }
    for (i, c) in v.get("chapters").and_then(|s| s.as_array()).into_iter().flatten().enumerate() {
        let title = c.pointer("/tags/title").map(val_to_string).unwrap_or_default();
        ex.fields.push(Field::new("Chapters", &format!("Chapter{}", i + 1), format!("{} → {} {}", val_to_string(&c["start_time"]), val_to_string(&c["end_time"]), title), "ffprobe", Provenance::Stored));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exiftool_json_parsing() {
        let j = br#"[{"SourceFile":"a.jpg","System:FileSize":"1 kB","EXIF:Artist":"Ann","EXIF:DateTimeOriginal":"2023:05:01 12:00:00+02:00","ExifTool:ExifToolVersion":12.5}]"#;
        let mut ex = Extraction::default();
        parse_exiftool(j, false, &mut ex).unwrap();
        assert_eq!(ex.fields.len(), 2);
        assert!(ex.fields[1].time.as_ref().unwrap().offset_known);
    }
    #[test]
    fn ffprobe_json_parsing() {
        let j = br#"{"format":{"format_name":"mov","duration":"1.5","tags":{"creation_time":"2023-05-01T10:00:00.000000Z"}},"streams":[{"codec_type":"video","codec_name":"h264","width":640,"height":480}]}"#;
        let mut ex = Extraction::default();
        parse_ffprobe(j, &mut ex).unwrap();
        assert!(ex.fields.iter().any(|f| f.name == "creation_time" && f.time.is_some()));
        assert!(ex.fields.iter().any(|f| f.group == "Stream0-video" && f.name == "width"));
    }
    #[test]
    fn timeout_kills_child() {
        if cfg!(unix) {
            let r = run_capped(Path::new("/bin/sleep"), &[std::ffi::OsStr::new("5")], Duration::from_millis(200), 1024);
            assert!(r.is_err());
        }
    }
}
