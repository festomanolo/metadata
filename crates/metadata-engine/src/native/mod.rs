//! Native (always available) extractors.
#![allow(deprecated)] // quick-xml unescape_value: replacement API needs a decoder we do not have per-attribute
pub mod archive;
pub mod image;
pub mod ooxml;
pub mod pdf;
pub mod text;
pub mod xmp;

use crate::{Extraction, Options};
use metadata_core::{Category, FormatInfo};
use std::path::Path;

pub fn extract(path: &Path, fmt: &FormatInfo, opts: &Options, ex: &mut Extraction) {
    // Parsers process untrusted data; a panic in one must not take down a batch.
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut local = Extraction::default();
        dispatch(path, fmt, opts, &mut local);
        local
    }));
    match r {
        Ok(l) => {
            ex.fields.extend(l.fields);
            ex.warnings.extend(l.warnings);
            ex.unavailable.extend(l.unavailable);
        }
        Err(_) => ex.warn("native", "internal parser failure on malformed input (file may be corrupt)"),
    }
}

fn dispatch(path: &Path, fmt: &FormatInfo, opts: &Options, ex: &mut Extraction) {
    match fmt.id.as_str() {
        "jpeg" | "png" | "gif" | "bmp" | "webp" | "tiff" | "heic" | "raw-raf" | "raw-cr3" => image::extract(path, &fmt.id, opts, ex),
        "docx" | "xlsx" | "pptx" | "odt" | "ods" | "odp" | "epub" => {
            ooxml::extract(path, &fmt.id, opts, ex);
            // OOXML/ODF/EPUB are ZIP containers; surface container-level indicators too.
            archive::extract(path, "zip", opts, ex, true);
        }
        "pdf" => pdf::extract(path, opts, ex),
        "zip" | "gzip" | "tar" => archive::extract(path, &fmt.id, opts, ex, false),
        "7z" | "rar" | "bzip2" | "xz" => ex.unavailable.push(format!("{} archive listing is not implemented", fmt.id)),
        "ole2" => ex.unavailable.push("legacy Office (OLE2) properties need exiftool".into()),
        _ => {}
    }
    if matches!(fmt.category, Category::Text | Category::Source) || matches!(fmt.id.as_str(), "html" | "xml" | "markdown" | "csv" | "tsv" | "json" | "svg" | "rtf") {
        text::extract(path, &fmt.id, opts, ex);
    }
}
