//! OOXML (docx/xlsx/pptx), ODF (odt/ods/odp) and EPUB metadata from ZIP parts. Never executes content.
use super::xmp::{self, entity_char};
use crate::{Extraction, Options};
use metadata_core::{time, Field, Provenance};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::io::{Read, Seek};
use std::path::Path;

const B: &str = "native";

fn read_part<R: Read + Seek>(z: &mut zip::ZipArchive<R>, name: &str, max: u64) -> Option<String> {
    let mut e = z.by_name(name).ok()?;
    if e.size() > max {
        return None;
    }
    let mut s = String::new();
    (&mut e).take(max).read_to_string(&mut s).ok()?;
    Some(s)
}

/// Leaf elements `(qname, text)` and attributes of elements named in `attr_elems`.
type Pairs = Vec<(String, String)>;

fn leaves(xml: &str, attr_elems: &[&str]) -> (Pairs, Pairs) {
    let mut r = Reader::from_str(xml);
    let (mut out, mut attrs) = (vec![], vec![]);
    let mut stack: Vec<String> = vec![];
    let mut text = String::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let n = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if attr_elems.contains(&n.as_str()) {
                    for a in e.attributes().flatten() {
                        if let Ok(v) = a.unescape_value() {
                            attrs.push((
                                String::from_utf8_lossy(a.key.as_ref()).to_string(),
                                v.to_string(),
                            ));
                        }
                    }
                }
                stack.push(n);
                text.clear();
            }
            Ok(Event::Text(t)) => {
                if let Ok(s) = t.decode() {
                    text.push_str(&s);
                }
            }
            Ok(Event::GeneralRef(r)) => {
                if let Some(c) = r.decode().ok().and_then(|n| entity_char(&n)) {
                    text.push(c);
                }
            }
            Ok(Event::End(_)) => {
                if let Some(n) = stack.pop() {
                    if !text.trim().is_empty() && !stack.is_empty() {
                        out.push((n, text.trim().to_string()));
                    }
                }
                text.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    (out, attrs)
}

fn pascal(q: &str) -> String {
    let n = q.rsplit(':').next().unwrap_or(q);
    n.split('-')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        })
        .collect()
}

fn push_props(group: &str, items: Vec<(String, String)>, ex: &mut Extraction, dates: &[&str]) {
    for (q, v) in items {
        let name = pascal(&q);
        let t = if dates.iter().any(|d| d.eq_ignore_ascii_case(&name)) {
            time::parse_iso(&v)
        } else {
            None
        };
        ex.fields
            .push(Field::new(group, &name, v, B, Provenance::Stored).with_time(t));
    }
}

pub fn extract(path: &Path, id: &str, opts: &Options, ex: &mut Extraction) {
    let Ok(file) = std::fs::File::open(path) else {
        return;
    };
    let mut z = match zip::ZipArchive::new(file) {
        Ok(z) => z,
        Err(e) => return ex.warn(B, format!("not a readable ZIP container: {e}")),
    };
    let max = opts.limits.max_entry_bytes;
    let names: Vec<String> = z
        .file_names()
        .take(opts.limits.max_archive_entries)
        .map(String::from)
        .collect();
    let has = |p: &str| names.iter().any(|n| n.starts_with(p));
    let count = |p: &str| names.iter().filter(|n| n.starts_with(p)).count();

    match id {
        "docx" | "xlsx" | "pptx" => {
            if let Some(x) = read_part(&mut z, "docProps/core.xml", max) {
                let (l, _) = leaves(&x, &[]);
                push_props("OOXML-Core", l, ex, &["Created", "Modified", "LastPrinted"]);
            } else {
                ex.fields.push(Field::new(
                    "OOXML-Core",
                    "CorePropertiesPart",
                    "absent",
                    B,
                    Provenance::Inferred,
                ));
            }
            if let Some(x) = read_part(&mut z, "docProps/app.xml", max) {
                let (l, _) = leaves(&x, &[]);
                push_props(
                    "OOXML-App",
                    l.into_iter()
                        .filter(|(k, _)| {
                            !k.starts_with("vt:")
                                && !k.contains("TitlesOfParts")
                                && !k.contains("HeadingPairs")
                        })
                        .collect(),
                    ex,
                    &[],
                );
            }
            if let Some(x) = read_part(&mut z, "docProps/custom.xml", max) {
                let mut r = Reader::from_str(&x);
                let mut cur: Option<String> = None;
                let mut txt = String::new();
                loop {
                    match r.read_event() {
                        Ok(Event::Start(e)) if e.name().as_ref() == b"property" => {
                            cur = e
                                .attributes()
                                .flatten()
                                .find(|a| a.key.as_ref() == b"name")
                                .and_then(|a| a.unescape_value().ok().map(|v| v.to_string()));
                        }
                        Ok(Event::Text(t)) => {
                            if let Ok(s) = t.decode() {
                                txt = s.to_string();
                            }
                        }
                        Ok(Event::End(e)) if e.name().as_ref() == b"property" => {
                            if let Some(n) = cur.take() {
                                ex.fields.push(Field::new(
                                    "OOXML-Custom",
                                    &n,
                                    txt.clone(),
                                    B,
                                    Provenance::Stored,
                                ));
                            }
                        }
                        Ok(Event::Eof) | Err(_) => break,
                        _ => {}
                    }
                }
            }
            match id {
                "docx" => {
                    if let Some(x) = read_part(&mut z, "word/document.xml", max) {
                        let ins = x.matches("<w:ins ").count();
                        let del = x.matches("<w:del ").count();
                        ex.fields.push(Field::new(
                            "Document",
                            "TrackedInsertions",
                            ins.to_string(),
                            B,
                            Provenance::Inferred,
                        ));
                        ex.fields.push(Field::new(
                            "Document",
                            "TrackedDeletions",
                            del.to_string(),
                            B,
                            Provenance::Inferred,
                        ));
                    }
                    if let Some(x) = read_part(&mut z, "word/comments.xml", max) {
                        ex.fields.push(Field::new(
                            "Document",
                            "CommentCount",
                            x.matches("<w:comment ").count().to_string(),
                            B,
                            Provenance::Inferred,
                        ));
                    }
                }
                "xlsx" => {
                    if let Some(x) = read_part(&mut z, "xl/workbook.xml", max) {
                        ex.fields.push(Field::new(
                            "Document",
                            "SheetCount",
                            x.matches("<sheet ").count().to_string(),
                            B,
                            Provenance::Inferred,
                        ));
                    }
                }
                _ => {
                    let slides = names
                        .iter()
                        .filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"))
                        .count();
                    ex.fields.push(Field::new(
                        "Document",
                        "SlideCount",
                        slides.to_string(),
                        B,
                        Provenance::Inferred,
                    ));
                }
            }
            let media = count(&format!(
                "{}/media/",
                match id {
                    "docx" => "word",
                    "xlsx" => "xl",
                    _ => "ppt",
                }
            ));
            let emb = count(&format!(
                "{}/embeddings/",
                match id {
                    "docx" => "word",
                    "xlsx" => "xl",
                    _ => "ppt",
                }
            ));
            ex.fields.push(Field::new(
                "Document",
                "EmbeddedMediaCount",
                media.to_string(),
                B,
                Provenance::Inferred,
            ));
            ex.fields.push(Field::new(
                "Document",
                "EmbeddedObjectCount",
                emb.to_string(),
                B,
                Provenance::Inferred,
            ));
            ex.fields.push(Field::new(
                "Document",
                "HasMacros",
                names
                    .iter()
                    .any(|n| n.ends_with("vbaProject.bin"))
                    .to_string(),
                B,
                Provenance::Inferred,
            ));
            ex.fields.push(Field::new(
                "Document",
                "HasDigitalSignature",
                has("_xmlsignatures/").to_string(),
                B,
                Provenance::Inferred,
            ));
            ex.fields.push(Field::new(
                "Document",
                "HasThumbnail",
                has("docProps/thumbnail").to_string(),
                B,
                Provenance::Inferred,
            ));
        }
        "odt" | "ods" | "odp" => {
            if let Some(x) = read_part(&mut z, "meta.xml", max) {
                let (l, a) = leaves(&x, &["meta:document-statistic"]);
                push_props("ODF-Meta", l, ex, &["CreationDate", "Date", "PrintDate"]);
                for (k, v) in a {
                    let name = pascal(&k);
                    ex.fields
                        .push(Field::new("ODF-Meta", &name, v, B, Provenance::Stored));
                }
            }
            ex.fields.push(Field::new(
                "Document",
                "HasDigitalSignature",
                has("META-INF/documentsignatures.xml").to_string(),
                B,
                Provenance::Inferred,
            ));
            ex.fields.push(Field::new(
                "Document",
                "HasMacros",
                (has("Basic/") || has("Scripts/")).to_string(),
                B,
                Provenance::Inferred,
            ));
            ex.fields.push(Field::new(
                "Document",
                "EmbeddedMediaCount",
                count("Pictures/").to_string(),
                B,
                Provenance::Inferred,
            ));
        }
        "epub" => {
            let opf = read_part(&mut z, "META-INF/container.xml", max).and_then(|c| {
                let i = c.find("full-path=\"")? + 11;
                Some(c[i..i + c[i..].find('"')?].to_string())
            });
            if let Some(p) = opf {
                if let Some(x) = read_part(&mut z, &p, max) {
                    let (l, _) = leaves(&x, &[]);
                    push_props(
                        "EPUB",
                        l.into_iter()
                            .filter(|(k, _)| k.starts_with("dc:"))
                            .collect(),
                        ex,
                        &["Date"],
                    );
                }
            } else {
                ex.warn(B, "EPUB container.xml missing or has no rootfile");
            }
        }
        _ => {}
    }
    // Embedded XMP (rare for OOXML; common in some producers)
    if let Some(x) = read_part(&mut z, "docProps/metadata.xmp", max) {
        for (k, v) in xmp::parse(&x) {
            ex.fields
                .push(Field::new("XMP", &pascal(&k), v, B, Provenance::Stored));
        }
    }
}
