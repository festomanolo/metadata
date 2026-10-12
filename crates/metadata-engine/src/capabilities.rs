//! Format capability matrix: what can be read/written, by which backend. Update when features land.
use crate::Backends;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct FormatCap {
    pub format: &'static str,
    pub category: &'static str,
    pub read: Vec<&'static str>,
    pub write: Vec<&'static str>,
    pub notes: &'static str,
}

pub fn matrix(b: &Backends) -> Vec<FormatCap> {
    let ex = b.exiftool.is_some();
    let ff = b.ffprobe.is_some();
    let r = |native: bool, exif: bool, ffp: bool| {
        let mut v = vec![];
        if native {
            v.push("native");
        }
        if exif && ex {
            v.push("exiftool");
        }
        if ffp && ff {
            v.push("ffprobe");
        }
        v
    };
    let w = |_exif: bool| -> Vec<&'static str> { vec![] }; // write support is claimed only once editing is implemented+verified (M4)
    vec![
        FormatCap {
            format: "JPEG",
            category: "image",
            read: r(true, true, false),
            write: w(true),
            notes: "native: EXIF, IPTC, XMP, ICC, JFIF, comments",
        },
        FormatCap {
            format: "PNG",
            category: "image",
            read: r(true, true, false),
            write: w(true),
            notes: "native: text chunks, tIME, XMP, eXIf",
        },
        FormatCap {
            format: "TIFF/RAW",
            category: "image",
            read: r(true, true, false),
            write: w(true),
            notes: "native: EXIF only; RAW maker notes need exiftool",
        },
        FormatCap {
            format: "WebP",
            category: "image",
            read: r(true, true, false),
            write: w(true),
            notes: "native: dimensions + EXIF",
        },
        FormatCap {
            format: "GIF/BMP",
            category: "image",
            read: r(true, true, false),
            write: w(true),
            notes: "native: dimensions/version",
        },
        FormatCap {
            format: "HEIC/HEIF",
            category: "image",
            read: r(true, true, false),
            write: w(true),
            notes: "native: EXIF via container; full coverage needs exiftool",
        },
        FormatCap {
            format: "PDF",
            category: "document",
            read: r(true, true, false),
            write: w(true),
            notes: "native: Info dict, XMP, revisions, signatures, JS/embedded-file indicators",
        },
        FormatCap {
            format: "DOCX/XLSX/PPTX",
            category: "document",
            read: r(true, true, false),
            write: w(true),
            notes: "native: core/app/custom props, comments, tracked changes, macros, signatures",
        },
        FormatCap {
            format: "ODT/ODS/ODP/EPUB",
            category: "document",
            read: r(true, true, false),
            write: w(true),
            notes: "native: meta.xml / OPF",
        },
        FormatCap {
            format: "DOC/XLS/PPT (OLE2)",
            category: "document",
            read: r(false, true, false),
            write: w(true),
            notes: "detected natively; properties need exiftool",
        },
        FormatCap {
            format: "RTF/TXT/MD/HTML/XML/CSV/JSON/source",
            category: "text",
            read: r(true, true, false),
            write: vec![],
            notes: "native: encoding, line endings, counts, HTML meta",
        },
        FormatCap {
            format: "MP4/MOV/MKV/AVI/WebM",
            category: "video",
            read: r(false, true, true),
            write: w(true),
            notes: "requires ffprobe and/or exiftool",
        },
        FormatCap {
            format: "MP3/WAV/FLAC/AAC/M4A/OGG",
            category: "audio",
            read: r(false, true, true),
            write: w(true),
            notes: "requires ffprobe and/or exiftool",
        },
        FormatCap {
            format: "ZIP/JAR",
            category: "archive",
            read: r(true, false, false),
            write: vec![],
            notes: "native listing with safety limits; never extracts",
        },
        FormatCap {
            format: "TAR/TGZ/GZIP",
            category: "archive",
            read: r(true, false, false),
            write: vec![],
            notes: "native listing with safety limits",
        },
        FormatCap {
            format: "7z/RAR/BZ2/XZ",
            category: "archive",
            read: vec![],
            write: vec![],
            notes: "detected only; listing PLANNED",
        },
    ]
}
