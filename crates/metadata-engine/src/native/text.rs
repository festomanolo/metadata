//! Text-family metadata: encoding, line endings, counts, HTML <meta>, RTF info, CSV shape.
use crate::{Extraction, Options};
use metadata_core::{time, Field, Provenance};
use std::io::Read;
use std::path::Path;

const B: &str = "native";
const TEXT_CAP: u64 = 16 << 20;

fn f(name: &str, raw: impl Into<String>) -> Field { Field::new("Text", name, raw, B, Provenance::Inferred) }

pub fn extract(path: &Path, id: &str, opts: &Options, ex: &mut Extraction) {
    let cap = opts.limits.max_structural_bytes.min(TEXT_CAP);
    let mut data = Vec::new();
    let len = match std::fs::File::open(path).and_then(|fh| { let l = fh.metadata()?.len(); fh.take(cap).read_to_end(&mut data)?; Ok(l) }) {
        Ok(l) => l,
        Err(e) => return ex.warn(B, format!("cannot read: {e}")),
    };
    if len > cap { ex.warn(B, format!("text analysis limited to first {cap} bytes")); }
    let (enc, body): (&str, String) = if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
        ("UTF-8 (BOM)", String::from_utf8_lossy(&data[3..]).into())
    } else if data.starts_with(&[0xFF, 0xFE]) || data.starts_with(&[0xFE, 0xFF]) {
        let le = data[0] == 0xFF;
        let u: Vec<u16> = data[2..].chunks_exact(2).map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) }).collect();
        (if le { "UTF-16LE (BOM)" } else { "UTF-16BE (BOM)" }, String::from_utf16_lossy(&u))
    } else if let Ok(s) = std::str::from_utf8(&data) {
        (if data.is_ascii() { "ASCII" } else { "UTF-8" }, s.to_string())
    } else {
        ("non-UTF-8 (lossy)", String::from_utf8_lossy(&data).into())
    };
    ex.fields.push(f("Encoding", enc));
    let (crlf, lf) = (body.matches("\r\n").count(), body.matches('\n').count());
    let lone_lf = lf - crlf;
    let lone_cr = body.matches('\r').count() - crlf;
    let eol = match (crlf > 0, lone_lf > 0, lone_cr > 0) {
        (false, false, false) => "none",
        (true, false, false) => "CRLF",
        (false, true, false) => "LF",
        (false, false, true) => "CR",
        _ => "mixed",
    };
    ex.fields.push(f("LineEndings", eol));
    ex.fields.push(f("LineCount", body.lines().count().to_string()));
    ex.fields.push(f("CharacterCount", body.chars().count().to_string()));
    if !matches!(id, "csv" | "tsv" | "json" | "xml" | "html" | "rtf") {
        ex.fields.push(f("WordCount", body.split_whitespace().count().to_string()));
    }
    if let Some(l) = body.lines().next().filter(|l| l.starts_with("#!")) { ex.fields.push(f("Shebang", l.trim())); }

    match id {
        "html" => html_meta(&body, ex),
        "xml" | "svg" => {
            let mut r = quick_xml::Reader::from_str(&body);
            while let Ok(ev) = r.read_event() {
                match ev {
                    quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e) => {
                        ex.fields.push(f("RootElement", String::from_utf8_lossy(e.name().as_ref())));
                        break;
                    }
                    quick_xml::events::Event::Eof => break,
                    _ => {}
                }
            }
        }
        "csv" | "tsv" => {
            let d = if id == "tsv" { '\t' } else { ',' };
            let rows = body.lines().filter(|l| !l.is_empty()).count();
            let cols = body.lines().next().map(|l| l.split(d).count()).unwrap_or(0);
            ex.fields.push(f("RowCount", rows.to_string()));
            ex.fields.push(f("ColumnCount", cols.to_string()));
        }
        "json" => match serde_json::from_str::<serde_json::Value>(&body) {
            Ok(v) => ex.fields.push(f("JsonRootType", match v { serde_json::Value::Object(_) => "object", serde_json::Value::Array(_) => "array", _ => "scalar" })),
            Err(e) if body.lines().count() > 1 && body.lines().all(|l| l.trim().is_empty() || serde_json::from_str::<serde_json::Value>(l).is_ok()) => { let _ = e; ex.fields.push(f("JsonRootType", "json-lines")); }
            Err(e) => ex.warn(B, format!("invalid JSON: {e}")),
        },
        "rtf" => rtf_info(&body, ex),
        _ => {}
    }
}

fn html_meta(s: &str, ex: &mut Extraction) {
    let low = s.to_ascii_lowercase();
    if let (Some(a), Some(b)) = (low.find("<title"), low.find("</title>")) {
        if let Some(gt) = low[a..].find('>') { if a + gt < b { ex.fields.push(Field::new("HTML", "Title", s[a + gt + 1..b].trim(), B, Provenance::Stored)); } }
    }
    let mut i = 0;
    while let Some(p) = low[i..].find("<meta") {
        let start = i + p;
        let Some(end) = low[start..].find('>') else { break };
        let tag = &s[start..start + end];
        let attr = |n: &str| -> Option<String> {
            let l = tag.to_ascii_lowercase();
            let k = l.find(&format!("{n}="))? + n.len() + 1;
            let q = tag[k..].chars().next()?;
            if q == '"' || q == '\'' { let rest = &tag[k + 1..]; Some(rest[..rest.find(q)?].to_string()) } else { None }
        };
        if let (Some(n), Some(c)) = (attr("name"), attr("content")) {
            let t = if n.to_ascii_lowercase().contains("date") { time::parse_any(&c) } else { None };
            ex.fields.push(Field::new("HTML", &n, c, B, Provenance::Stored).with_time(t));
        } else if let Some(cs) = attr("charset") { ex.fields.push(Field::new("HTML", "charset", cs, B, Provenance::Stored)); }
        i = start + end;
    }
}

fn rtf_info(s: &str, ex: &mut Extraction) {
    let Some(info) = s.find("{\\info") else { return };
    let seg = &s[info..s.len().min(info + 4096)];
    for key in ["title", "subject", "author", "operator", "company", "keywords"] {
        if let Some(p) = seg.find(&format!("{{\\{key} ")) {
            let v = &seg[p + key.len() + 3..];
            if let Some(e) = v.find('}') { ex.fields.push(Field::new("RTF", key, v[..e].trim(), B, Provenance::Stored)); }
        }
    }
    for key in ["creatim", "revtim", "printim"] {
        if let Some(p) = seg.find(&format!("{{\\{key}")) {
            let v = &seg[p..seg[p..].find('}').map(|e| p + e).unwrap_or(seg.len())];
            let num = |k: &str| v.find(&format!("\\{k}")).and_then(|i| v[i + k.len() + 1..].chars().take_while(char::is_ascii_digit).collect::<String>().parse::<u32>().ok());
            if let (Some(y), Some(mo), Some(d)) = (num("yr"), num("mo"), num("dy")) {
                let iso = format!("{y:04}-{mo:02}-{d:02}T{:02}:{:02}:00", num("hr").unwrap_or(0), num("min").unwrap_or(0));
                ex.fields.push(Field::new("RTF", key, iso.clone(), B, Provenance::Stored).with_time(time::parse_iso(&iso)));
            }
        }
    }
}
