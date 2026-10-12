//! PDF Info-dictionary editing via lopdf. Rewrites the file (earlier incremental revisions are not preserved).
use super::{Resolved, WProfile};
use lopdf::{Dictionary, Document, Object, StringFormat};
use metadata_core::{time, MetaError, Result};

pub const KEYS: &[&str] = &[
    "Title",
    "Author",
    "Subject",
    "Keywords",
    "Creator",
    "Producer",
    "CreationDate",
    "ModDate",
];

fn pdf_string(s: &str) -> Object {
    if s.is_ascii() {
        Object::String(s.as_bytes().to_vec(), StringFormat::Literal)
    } else {
        let mut v = vec![0xFE, 0xFF];
        for u in s.encode_utf16() {
            v.extend(u.to_be_bytes());
        }
        Object::String(v, StringFormat::Literal)
    }
}

pub fn apply(data: &[u8], ops: &[Resolved], profile: Option<WProfile>) -> Result<Vec<u8>> {
    let mut doc =
        Document::load_mem(data).map_err(|e| MetaError::Other(format!("PDF unreadable: {e}")))?;
    if doc.is_encrypted() {
        return Err(MetaError::Unsupported(
            "encrypted PDFs cannot be edited".into(),
        ));
    }
    if matches!(profile, Some(WProfile::Gps)) {
        return Err(MetaError::Unsupported(
            "PDFs have no GPS block; the gps profile does not apply".into(),
        ));
    }
    let info_id = match doc
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|o| o.as_reference().ok())
    {
        Some(id) => id,
        None => {
            let id = doc.add_object(Dictionary::new());
            doc.trailer.set("Info", Object::Reference(id));
            id
        }
    };
    let mut remove_xmp = false;
    {
        let info = doc
            .get_object_mut(info_id)
            .and_then(Object::as_dict_mut)
            .map_err(|e| MetaError::Other(format!("Info dictionary unreadable: {e}")))?;
        match profile {
            Some(WProfile::All) => {
                let keys: Vec<Vec<u8>> = info.iter().map(|(k, _)| k.clone()).collect();
                for k in keys {
                    info.remove(&k);
                }
                remove_xmp = true;
            }
            Some(WProfile::Privacy) => {
                info.remove(b"Author");
                info.remove(b"Creator");
                remove_xmp = true;
            }
            _ => {}
        }
        for o in ops {
            let name = o.key.trim_start_matches("PDF:");
            match &o.value {
                None => {
                    info.remove(name.as_bytes());
                }
                Some(v) => {
                    let val = if name.ends_with("Date") {
                        let t = time::parse_iso(v).ok_or_else(|| {
                            MetaError::InvalidArgument(format!(
                                "'{v}' is not an ISO-8601 date-time"
                            ))
                        })?;
                        pdf_string(&time::to_pdf(&t).unwrap())
                    } else {
                        pdf_string(v)
                    };
                    info.set(name.as_bytes().to_vec(), val);
                }
            }
        }
    }
    if remove_xmp {
        if let Ok(root) = doc.trailer.get(b"Root").and_then(Object::as_reference) {
            if let Ok(cat) = doc.get_object_mut(root).and_then(Object::as_dict_mut) {
                cat.remove(b"Metadata");
            }
        }
    }
    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| MetaError::Other(format!("PDF write failed: {e}")))?;
    Ok(out)
}
