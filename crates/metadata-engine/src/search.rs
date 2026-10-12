//! Field/value search over reports. Distinguishes found / absent / unsupported / unavailable.
use metadata_core::{Field, FileReport};
use serde::Serialize;

/// Canonical query names expand to the field names different formats use for the same concept.
pub fn aliases(query: &str) -> Vec<String> {
    let q = query.to_ascii_lowercase();
    let set: &[&str] = match q.as_str() {
        "author" | "creator" | "artist" => &["author", "creator", "artist", "by-line", "initialcreator", "initial-creator"],
        "created" | "creationdate" | "createdate" => &["created", "createdate", "creationdate", "creationtime", "creation_time", "datetimeoriginal", "datecreated", "creatim"],
        "modified" | "modifydate" | "moddate" => &["modified", "modifydate", "moddate", "date", "datetime", "revtim"],
        "software" | "producer" | "application" | "generator" => &["software", "producer", "application", "creatortool", "generator", "encoder"],
        "gps" => &["gpslatitude", "gpslongitude", "gpsaltitude", "gpspositiondecimal", "gpsposition"],
        _ => return vec![q],
    };
    set.iter().map(|s| s.to_string()).collect()
}

/// Can this format family carry the queried concept at all?
fn format_can_hold(format_id: &str, query: &str) -> Option<bool> {
    let q = query.to_ascii_lowercase();
    let image = ["jpeg", "tiff", "heic", "webp", "png", "raw-raf", "raw-cr3"];
    let av = ["mp4", "mov", "m4a", "mkv", "webm", "avi", "mp3", "wav", "flac", "ogg", "aac"];
    if q.starts_with("gps") {
        return Some(["jpeg", "tiff", "heic", "webp", "raw-raf", "raw-cr3", "mp4", "mov"].contains(&format_id));
    }
    if ["exposuretime", "fnumber", "iso", "make", "model", "lensmodel", "focallength", "orientation"].contains(&q.as_str()) {
        return Some(image.contains(&format_id) || ["mp4", "mov"].contains(&format_id));
    }
    if ["pagecount", "revisions", "lastmodifiedby", "revision", "company"].contains(&q.as_str()) {
        return Some(["pdf", "docx", "xlsx", "pptx", "odt", "ods", "odp", "ole2"].contains(&format_id) || av.is_empty());
    }
    None
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    Found,
    /// The format can hold the field and extraction ran, but the file does not contain it.
    Absent,
    /// The format cannot store this field.
    Unsupported,
    /// A backend needed to read it is not installed, so absence cannot be concluded.
    Unavailable,
    Error,
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub path: String,
    pub presence: Presence,
    pub matches: Vec<Field>,
    pub detail: Option<String>,
}

pub struct Query {
    pub field: Option<String>,
    pub value: Option<String>,
}

pub fn search(r: &FileReport, q: &Query) -> SearchResult {
    let value_ok = |f: &Field| q.value.as_ref().is_none_or(|v| f.raw.to_ascii_lowercase().contains(&v.to_ascii_lowercase()));
    let names = q.field.as_deref().map(aliases);
    let matches: Vec<Field> = r.fields.iter().filter(|f| {
        let name_ok = match (&names, q.field.as_deref()) {
            (Some(ns), Some(orig)) => {
                let grp = orig.split_once(':');
                match grp {
                    Some((g, n)) => f.group.eq_ignore_ascii_case(g) && f.name.eq_ignore_ascii_case(n),
                    None => ns.iter().any(|n| f.name.to_ascii_lowercase() == *n),
                }
            }
            _ => true,
        };
        name_ok && value_ok(f)
    }).cloned().collect();
    let (presence, detail) = if let Some(e) = &r.error {
        (Presence::Error, Some(e.clone()))
    } else if !matches.is_empty() {
        (Presence::Found, None)
    } else if let Some(field) = &q.field {
        let bare = field.rsplit(':').next().unwrap_or(field);
        if format_can_hold(&r.format.id, bare) == Some(false) {
            (Presence::Unsupported, Some(format!("{} files cannot store '{bare}'", r.format.id)))
        } else if !r.unavailable.is_empty() {
            (Presence::Unavailable, Some(r.unavailable.join("; ")))
        } else {
            (Presence::Absent, None)
        }
    } else {
        (Presence::Absent, None)
    };
    SearchResult { path: r.path.clone(), presence, matches, detail }
}

#[cfg(test)]
mod tests {
    use super::*;
    use metadata_core::*;
    fn rep(fmt: &str, fields: &[(&str, &str)], unavailable: &[&str]) -> FileReport {
        FileReport {
            schema_version: "1".into(), path: "p".into(), size: 0, sha256: None,
            format: FormatInfo { id: fmt.into(), description: "".into(), mime: "".into(), category: Category::Other, extension_match: ExtensionMatch::Unknown, signature_hex: "".into() },
            filesystem: FsInfo::default(),
            fields: fields.iter().map(|(n, v)| Field::new("G", n, *v, "t", Provenance::Stored)).collect(),
            warnings: vec![], backends: vec![], unavailable: unavailable.iter().map(|s| s.to_string()).collect(), error: None,
        }
    }
    #[test]
    fn found_absent_unsupported_unavailable() {
        let q = Query { field: Some("Author".into()), value: None };
        assert_eq!(search(&rep("docx", &[("Creator", "Ann")], &[]), &q).presence, Presence::Found);
        assert_eq!(search(&rep("docx", &[], &[]), &q).presence, Presence::Absent);
        let g = Query { field: Some("GPSLatitude".into()), value: None };
        assert_eq!(search(&rep("pdf", &[], &[]), &g).presence, Presence::Unsupported);
        assert_eq!(search(&rep("jpeg", &[], &["needs exiftool"]), &g).presence, Presence::Unavailable);
        assert_eq!(search(&rep("jpeg", &[], &[]), &g).presence, Presence::Absent);
    }
    #[test]
    fn value_search() {
        let q = Query { field: None, value: Some("ann".into()) };
        assert_eq!(search(&rep("docx", &[("Creator", "Ann Lee")], &[]), &q).matches.len(), 1);
    }
}
