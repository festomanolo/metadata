//! OOXML (docx/xlsx/pptx) and ODF (odt/ods/odp) property editing.
use super::xmlprops::{remove_element, set_element};
use super::zipedit::{read_part, rewrite};
use super::{Resolved, WProfile};
use metadata_core::{time, MetaError, Result};
use std::collections::HashMap;
use std::io::Cursor;

struct Spec {
    key: &'static str,
    part: &'static str,
    q: &'static str,
    date: bool,
    personal: bool,
}

const fn sp(
    key: &'static str,
    part: &'static str,
    q: &'static str,
    date: bool,
    personal: bool,
) -> Spec {
    Spec {
        key,
        part,
        q,
        date,
        personal,
    }
}

const OOXML: &[Spec] = &[
    sp(
        "OOXML-Core:Title",
        "docProps/core.xml",
        "dc:title",
        false,
        false,
    ),
    sp(
        "OOXML-Core:Subject",
        "docProps/core.xml",
        "dc:subject",
        false,
        false,
    ),
    sp(
        "OOXML-Core:Creator",
        "docProps/core.xml",
        "dc:creator",
        false,
        true,
    ),
    sp(
        "OOXML-Core:Keywords",
        "docProps/core.xml",
        "cp:keywords",
        false,
        false,
    ),
    sp(
        "OOXML-Core:Description",
        "docProps/core.xml",
        "dc:description",
        false,
        false,
    ),
    sp(
        "OOXML-Core:LastModifiedBy",
        "docProps/core.xml",
        "cp:lastModifiedBy",
        false,
        true,
    ),
    sp(
        "OOXML-Core:Category",
        "docProps/core.xml",
        "cp:category",
        false,
        false,
    ),
    sp(
        "OOXML-Core:Revision",
        "docProps/core.xml",
        "cp:revision",
        false,
        false,
    ),
    sp(
        "OOXML-Core:Created",
        "docProps/core.xml",
        "dcterms:created",
        true,
        false,
    ),
    sp(
        "OOXML-Core:Modified",
        "docProps/core.xml",
        "dcterms:modified",
        true,
        false,
    ),
    sp(
        "OOXML-App:Company",
        "docProps/app.xml",
        "Company",
        false,
        true,
    ),
    sp(
        "OOXML-App:Manager",
        "docProps/app.xml",
        "Manager",
        false,
        true,
    ),
    sp(
        "OOXML-App:Application",
        "docProps/app.xml",
        "Application",
        false,
        false,
    ),
];

const ODF: &[Spec] = &[
    sp("ODF-Meta:Title", "meta.xml", "dc:title", false, false),
    sp("ODF-Meta:Subject", "meta.xml", "dc:subject", false, false),
    sp(
        "ODF-Meta:Description",
        "meta.xml",
        "dc:description",
        false,
        false,
    ),
    sp(
        "ODF-Meta:InitialCreator",
        "meta.xml",
        "meta:initial-creator",
        false,
        true,
    ),
    sp("ODF-Meta:Creator", "meta.xml", "dc:creator", false, true),
    sp(
        "ODF-Meta:Generator",
        "meta.xml",
        "meta:generator",
        false,
        false,
    ),
    sp("ODF-Meta:Keyword", "meta.xml", "meta:keyword", false, false),
    sp(
        "ODF-Meta:CreationDate",
        "meta.xml",
        "meta:creation-date",
        true,
        false,
    ),
    sp("ODF-Meta:Date", "meta.xml", "dc:date", true, false),
];

fn table(format: &str) -> Option<&'static [Spec]> {
    match format {
        "docx" | "xlsx" | "pptx" => Some(OOXML),
        "odt" | "ods" | "odp" => Some(ODF),
        _ => None,
    }
}

pub fn keys(format: &str) -> Vec<&'static str> {
    table(format)
        .map(|t| t.iter().map(|s| s.key).collect())
        .unwrap_or_default()
}

pub fn has_signature(data: &[u8]) -> bool {
    zip::ZipArchive::new(Cursor::new(data)).is_ok_and(|z| {
        z.file_names()
            .any(|n| n.starts_with("_xmlsignatures/") || n == "META-INF/documentsignatures.xml")
    })
}

/// Date values must carry an explicit timezone for OOXML (W3CDTF `…Z`) to avoid silently inventing one.
fn format_date(format: &str, v: &str) -> Result<String> {
    let t = time::parse_iso(v)
        .ok_or_else(|| MetaError::InvalidArgument(format!("'{v}' is not an ISO-8601 date-time")))?;
    if format.starts_with("od") {
        return Ok(t.iso.clone());
    }
    if !t.offset_known {
        return Err(MetaError::InvalidArgument(format!(
            "'{v}' has no timezone; OOXML dates need one (e.g. 2024-03-01T10:00:00Z)"
        )));
    }
    Ok(t.utc.clone().unwrap_or(t.iso))
}

pub fn apply(
    format: &str,
    data: &[u8],
    ops: &[Resolved],
    profile: Option<WProfile>,
) -> Result<Vec<u8>> {
    let t = table(format)
        .ok_or_else(|| MetaError::Unsupported(format!("no office writer for {format}")))?;
    let mut z = zip::ZipArchive::new(Cursor::new(data))
        .map_err(|e| MetaError::Other(format!("not a readable ZIP: {e}")))?;
    let mut parts: HashMap<String, String> = HashMap::new();
    let mut all_ops: Vec<(&Spec, Option<String>)> = vec![];
    if let Some(p) = profile {
        for s in t.iter().filter(|s| {
            matches!(p, WProfile::All) || (matches!(p, WProfile::Privacy) && s.personal)
        }) {
            all_ops.push((s, None));
        }
        if matches!(p, WProfile::Gps) {
            return Err(MetaError::Unsupported(
                "documents carry no GPS block; the gps profile does not apply".into(),
            ));
        }
    }
    for o in ops {
        let s = t.iter().find(|s| s.key == o.key).ok_or_else(|| {
            MetaError::Unsupported(format!("{} is not writable for {format}", o.key))
        })?;
        let v = match &o.value {
            Some(v) if s.date => Some(format_date(format, v)?),
            other => other.clone(),
        };
        all_ops.push((s, v));
    }
    for (s, v) in all_ops {
        let xml = match parts.get(s.part) {
            Some(x) => x.clone(),
            None => read_part(&mut z, s.part, 8 << 20).ok_or_else(|| {
                MetaError::Unsupported(format!("document has no {} part", s.part))
            })?,
        };
        let attrs = if s.date && !format.starts_with("od") {
            " xsi:type=\"dcterms:W3CDTF\""
        } else {
            ""
        };
        let new = match v {
            Some(v) => set_element(&xml, s.q, &v, attrs)?,
            None => remove_element(&xml, s.q).0,
        };
        parts.insert(s.part.to_string(), new);
    }
    let replace: HashMap<String, Vec<u8>> = parts
        .into_iter()
        .map(|(k, v)| (k, v.into_bytes()))
        .collect();
    rewrite(data, &replace, &[])
}
