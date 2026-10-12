//! PDF metadata: Info dictionary, XMP, structure indicators (revisions, signatures, JS, attachments).
use super::xmp;
use crate::{Extraction, Options};
use metadata_core::{time, Field, Provenance};
use std::io::Read;
use std::path::Path;

const B: &str = "native";

fn count(hay: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() || hay.len() < needle.len() { return 0; }
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

fn decode_pdf_string(b: &[u8]) -> String {
    if b.starts_with(&[0xFE, 0xFF]) {
        let u: Vec<u16> = b[2..].chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&u)
    } else if b.starts_with(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(&b[3..]).to_string()
    } else {
        b.iter().map(|&c| c as char).collect() // PDFDocEncoding ≈ Latin-1 for the common range
    }
}

pub fn extract(path: &Path, opts: &Options, ex: &mut Extraction) {
    let mut data = Vec::new();
    let truncated = match std::fs::File::open(path).and_then(|f| {
        let len = f.metadata()?.len();
        f.take(opts.limits.max_structural_bytes).read_to_end(&mut data)?;
        Ok(len > opts.limits.max_structural_bytes)
    }) {
        Ok(t) => t,
        Err(e) => return ex.warn(B, format!("cannot read PDF: {e}")),
    };
    if truncated { ex.warn(B, "file larger than structural limit; only the first part was analyzed"); }
    if data.len() >= 8 {
        let v = String::from_utf8_lossy(&data[5..8]).to_string();
        ex.fields.push(Field::new("PDF", "PDFVersion", v, B, Provenance::Stored));
    }
    let eofs = count(&data, b"%%EOF");
    ex.fields.push(Field::new("PDF", "Revisions", eofs.max(1).to_string(), B, Provenance::Inferred));
    if eofs > 1 { ex.warn(B, format!("{eofs} %%EOF markers: file contains incremental updates")); }
    let flag = |name: &str, v: bool| Field::new("PDF-Structure", name, v.to_string(), B, Provenance::Inferred);
    ex.fields.push(flag("HasDigitalSignature", count(&data, b"/ByteRange") > 0 && count(&data, b"/Sig") > 0));
    ex.fields.push(flag("Encrypted", count(&data, b"/Encrypt") > 0));
    ex.fields.push(flag("HasJavaScript", count(&data, b"/JavaScript") > 0 || count(&data, b"/JS") > 0));
    ex.fields.push(flag("HasLaunchAction", count(&data, b"/Launch") > 0));
    ex.fields.push(flag("HasOpenAction", count(&data, b"/OpenAction") > 0));
    ex.fields.push(Field::new("PDF-Structure", "EmbeddedFileCount", count(&data, b"/EmbeddedFile").to_string(), B, Provenance::Inferred));

    // XMP packet (only discoverable when its stream is stored uncompressed).
    if let Some(s) = data.windows(10).position(|w| w == b"<x:xmpmeta") {
        if let Some(e) = data[s..].windows(12).position(|w| w == b"</x:xmpmeta>") {
            let x = String::from_utf8_lossy(&data[s..s + e + 12]).to_string();
            for (k, v) in xmp::parse(&x) {
                let name = k.split_once(':').map(|(_, n)| n.to_string()).unwrap_or(k);
                let t = if name.ends_with("Date") { time::parse_any(&v) } else { None };
                ex.fields.push(Field::new("XMP", &name, v, B, Provenance::Stored).with_time(t));
            }
        }
    }

    let doc = std::panic::catch_unwind(|| lopdf::Document::load_mem(&data));
    match doc {
        Ok(Ok(doc)) => {
            ex.fields.push(Field::new("PDF", "PageCount", doc.get_pages().len().to_string(), B, Provenance::Inferred));
            if doc.is_encrypted() {
                ex.warn(B, "PDF is encrypted; Info dictionary values may be unreadable");
            }
            let info = doc.trailer.get(b"Info").ok().and_then(|o| o.as_reference().ok()).and_then(|r| doc.get_dictionary(r).ok());
            match info {
                Some(d) => {
                    for (k, v) in d.iter() {
                        let name = String::from_utf8_lossy(k).to_string();
                        let raw = match v {
                            lopdf::Object::String(s, _) => decode_pdf_string(s),
                            lopdf::Object::Name(n) => String::from_utf8_lossy(n).to_string(),
                            lopdf::Object::Integer(i) => i.to_string(),
                            lopdf::Object::Boolean(b) => b.to_string(),
                            _ => continue,
                        };
                        let t = if name.ends_with("Date") { time::parse_pdf(&raw) } else { None };
                        ex.fields.push(Field::new("PDF", &name, raw, B, Provenance::Stored).with_time(t));
                    }
                }
                None => ex.fields.push(Field::new("PDF", "InfoDictionary", "absent", B, Provenance::Inferred)),
            }
        }
        Ok(Err(e)) => ex.warn(B, format!("PDF structure could not be parsed: {e}")),
        Err(_) => ex.warn(B, "PDF parser failed on malformed input"),
    }
}
