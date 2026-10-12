//! Content-based format detection (signatures first, extension only as a hint/mismatch check).
use metadata_core::{Category, ExtensionMatch, FormatInfo};
use std::io::Read;
use std::path::Path;

pub struct Spec {
    pub id: &'static str,
    pub desc: &'static str,
    pub mime: &'static str,
    pub cat: Category,
    pub exts: &'static [&'static str],
}

use Category::*;
const fn s(
    id: &'static str,
    desc: &'static str,
    mime: &'static str,
    cat: Category,
    exts: &'static [&'static str],
) -> Spec {
    Spec {
        id,
        desc,
        mime,
        cat,
        exts,
    }
}

pub static SPECS: &[Spec] = &[
    s(
        "jpeg",
        "JPEG image",
        "image/jpeg",
        Image,
        &["jpg", "jpeg", "jpe", "jfif"],
    ),
    s("png", "PNG image", "image/png", Image, &["png"]),
    s("gif", "GIF image", "image/gif", Image, &["gif"]),
    s("bmp", "BMP image", "image/bmp", Image, &["bmp", "dib"]),
    s(
        "tiff",
        "TIFF image",
        "image/tiff",
        Image,
        &[
            "tif", "tiff", "nef", "arw", "dng", "cr2", "orf", "rw2", "pef", "srw", "raw",
        ],
    ),
    s("webp", "WebP image", "image/webp", Image, &["webp"]),
    s(
        "heic",
        "HEIC/HEIF image",
        "image/heic",
        Image,
        &["heic", "heif", "hif", "avif"],
    ),
    s(
        "raw-raf",
        "Fujifilm RAW",
        "image/x-fuji-raf",
        Image,
        &["raf"],
    ),
    s(
        "raw-cr3",
        "Canon CR3 RAW",
        "image/x-canon-cr3",
        Image,
        &["cr3"],
    ),
    s("svg", "SVG image", "image/svg+xml", Image, &["svg"]),
    s("mp4", "MP4 video", "video/mp4", Video, &["mp4", "m4v"]),
    s(
        "mov",
        "QuickTime movie",
        "video/quicktime",
        Video,
        &["mov", "qt"],
    ),
    s("m4a", "MPEG-4 audio", "audio/mp4", Audio, &["m4a", "m4b"]),
    s(
        "mkv",
        "Matroska video",
        "video/x-matroska",
        Video,
        &["mkv", "mka"],
    ),
    s("webm", "WebM video", "video/webm", Video, &["webm"]),
    s("avi", "AVI video", "video/x-msvideo", Video, &["avi"]),
    s("wav", "WAVE audio", "audio/wav", Audio, &["wav", "wave"]),
    s("flac", "FLAC audio", "audio/flac", Audio, &["flac"]),
    s(
        "ogg",
        "Ogg container",
        "audio/ogg",
        Audio,
        &["ogg", "oga", "ogv", "opus"],
    ),
    s("mp3", "MP3 audio", "audio/mpeg", Audio, &["mp3"]),
    s("aac", "AAC audio (ADTS)", "audio/aac", Audio, &["aac"]),
    s("pdf", "PDF document", "application/pdf", Document, &["pdf"]),
    s(
        "docx",
        "Word document (OOXML)",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Document,
        &["docx", "docm", "dotx"],
    ),
    s(
        "xlsx",
        "Excel workbook (OOXML)",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Document,
        &["xlsx", "xlsm", "xltx"],
    ),
    s(
        "pptx",
        "PowerPoint presentation (OOXML)",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        Document,
        &["pptx", "pptm", "potx"],
    ),
    s(
        "odt",
        "OpenDocument text",
        "application/vnd.oasis.opendocument.text",
        Document,
        &["odt", "ott"],
    ),
    s(
        "ods",
        "OpenDocument spreadsheet",
        "application/vnd.oasis.opendocument.spreadsheet",
        Document,
        &["ods", "ots"],
    ),
    s(
        "odp",
        "OpenDocument presentation",
        "application/vnd.oasis.opendocument.presentation",
        Document,
        &["odp", "otp"],
    ),
    s(
        "epub",
        "EPUB book",
        "application/epub+zip",
        Document,
        &["epub"],
    ),
    s(
        "ole2",
        "OLE2 compound document (legacy Office)",
        "application/x-ole-storage",
        Document,
        &["doc", "xls", "ppt", "msg", "dot"],
    ),
    s(
        "rtf",
        "Rich Text Format",
        "application/rtf",
        Document,
        &["rtf"],
    ),
    s(
        "zip",
        "ZIP archive",
        "application/zip",
        Archive,
        &["zip", "jar", "war", "apk", "whl"],
    ),
    s(
        "gzip",
        "GZIP compressed data",
        "application/gzip",
        Archive,
        &["gz", "tgz"],
    ),
    s("tar", "TAR archive", "application/x-tar", Archive, &["tar"]),
    s(
        "7z",
        "7-Zip archive",
        "application/x-7z-compressed",
        Archive,
        &["7z"],
    ),
    s(
        "bzip2",
        "BZIP2 compressed data",
        "application/x-bzip2",
        Archive,
        &["bz2", "tbz2"],
    ),
    s(
        "xz",
        "XZ compressed data",
        "application/x-xz",
        Archive,
        &["xz", "txz"],
    ),
    s(
        "rar",
        "RAR archive",
        "application/vnd.rar",
        Archive,
        &["rar"],
    ),
    s(
        "html",
        "HTML document",
        "text/html",
        Document,
        &["html", "htm", "xhtml"],
    ),
    s(
        "xml",
        "XML document",
        "application/xml",
        Document,
        &["xml", "xsd", "xsl", "plist"],
    ),
    s(
        "markdown",
        "Markdown text",
        "text/markdown",
        Document,
        &["md", "markdown"],
    ),
    s("csv", "CSV data", "text/csv", Document, &["csv"]),
    s(
        "tsv",
        "TSV data",
        "text/tab-separated-values",
        Document,
        &["tsv", "tab"],
    ),
    s(
        "json",
        "JSON data",
        "application/json",
        Text,
        &["json", "jsonl", "geojson"],
    ),
    s(
        "text",
        "Plain text",
        "text/plain",
        Text,
        &[
            "txt",
            "text",
            "log",
            "cfg",
            "conf",
            "ini",
            "toml",
            "yaml",
            "yml",
            "lock",
            "gitignore",
        ],
    ),
    s(
        "binary",
        "Unrecognized binary data",
        "application/octet-stream",
        Other,
        &[],
    ),
];

/// Source-code extensions → language name.
pub static SOURCE_EXTS: &[(&str, &str)] = &[
    ("rs", "Rust"),
    ("py", "Python"),
    ("js", "JavaScript"),
    ("mjs", "JavaScript"),
    ("ts", "TypeScript"),
    ("tsx", "TypeScript"),
    ("jsx", "JavaScript"),
    ("java", "Java"),
    ("kt", "Kotlin"),
    ("c", "C"),
    ("h", "C header"),
    ("cpp", "C++"),
    ("cc", "C++"),
    ("hpp", "C++ header"),
    ("cs", "C#"),
    ("go", "Go"),
    ("rb", "Ruby"),
    ("php", "PHP"),
    ("swift", "Swift"),
    ("sh", "Shell"),
    ("bash", "Shell"),
    ("sql", "SQL"),
    ("css", "CSS"),
    ("scss", "SCSS"),
    ("dart", "Dart"),
    ("lua", "Lua"),
    ("r", "R"),
    ("m", "Objective-C/MATLAB"),
    ("scala", "Scala"),
    ("vue", "Vue"),
    ("ps1", "PowerShell"),
    ("bat", "Batch"),
    ("asm", "Assembly"),
    ("v", "Verilog"),
    ("ino", "Arduino"),
];

pub fn spec(id: &str) -> &'static Spec {
    SPECS
        .iter()
        .find(|s| s.id == id)
        .unwrap_or(SPECS.last().unwrap())
}

pub struct Detected {
    pub info: FormatInfo,
    pub head: Vec<u8>,
}

fn ext_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Detect from bytes + extension hint. `zip_kind` resolves ZIP-based containers.
pub fn detect_bytes(
    head: &[u8],
    ext: &str,
    zip_kind: impl Fn() -> Option<&'static str>,
) -> &'static str {
    let h = head;
    let starts = |p: &[u8]| h.starts_with(p);
    if starts(&[0xFF, 0xD8, 0xFF]) {
        return "jpeg";
    }
    if starts(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return "png";
    }
    if starts(b"GIF87a") || starts(b"GIF89a") {
        return "gif";
    }
    if starts(b"FUJIFILMCCD-RAW") {
        return "raw-raf";
    }
    if starts(b"II*\0") || starts(b"MM\0*") || starts(b"II+\0") || starts(b"MM\0+") {
        return "tiff";
    }
    if h.len() >= 12 && &h[0..4] == b"RIFF" {
        match &h[8..12] {
            b"WEBP" => return "webp",
            b"AVI " => return "avi",
            b"WAVE" => return "wav",
            _ => {}
        }
    }
    if h.len() >= 12 && &h[4..8] == b"ftyp" {
        let brand = &h[8..12];
        return match brand {
            b"heic" | b"heix" | b"hevc" | b"mif1" | b"msf1" | b"heim" | b"heis" | b"avif" => "heic",
            b"crx " => "raw-cr3",
            b"qt  " => "mov",
            b"M4A " | b"M4B " => "m4a",
            _ => "mp4",
        };
    }
    if starts(&[0x1A, 0x45, 0xDF, 0xA3]) {
        let win = &h[..h.len().min(64)];
        return if win.windows(4).any(|w| w == b"webm") {
            "webm"
        } else {
            "mkv"
        };
    }
    if starts(b"fLaC") {
        return "flac";
    }
    if starts(b"OggS") {
        return "ogg";
    }
    if starts(b"ID3") {
        return "mp3";
    }
    if h.len() >= 2 && h[0] == 0xFF && (h[1] & 0xE0) == 0xE0 {
        // layer bits 00 => ADTS AAC, otherwise MPEG audio
        return if (h[1] & 0x06) == 0 { "aac" } else { "mp3" };
    }
    if starts(b"%PDF") {
        return "pdf";
    }
    if starts(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        return "ole2";
    }
    if starts(b"{\\rtf") {
        return "rtf";
    }
    if starts(b"PK\x03\x04") || starts(b"PK\x05\x06") {
        return zip_kind().unwrap_or("zip");
    }
    if starts(&[0x1F, 0x8B]) {
        return "gzip";
    }
    if starts(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]) {
        return "7z";
    }
    if starts(b"BZh") {
        return "bzip2";
    }
    if starts(&[0xFD, b'7', b'z', b'X', b'Z', 0x00]) {
        return "xz";
    }
    if starts(b"Rar!\x1A\x07") {
        return "rar";
    }
    if h.len() > 262 && &h[257..262] == b"ustar" {
        return "tar";
    }
    if starts(b"BM") && h.len() >= 14 && h[6..10] == [0, 0, 0, 0] {
        return "bmp";
    }

    // Text-ish content.
    if !h.contains(&0) && std::str::from_utf8(h).is_ok()
        || (h.len() == 4096 && !h.contains(&0) && is_mostly_text(h))
    {
        let lower: String = String::from_utf8_lossy(&h[..h.len().min(512)]).to_ascii_lowercase();
        let t = lower.trim_start_matches('\u{feff}').trim_start();
        if t.starts_with("<!doctype html") || t.starts_with("<html") {
            return "html";
        }
        if t.starts_with("<svg") || (t.starts_with("<?xml") && lower.contains("<svg")) {
            return "svg";
        }
        if t.starts_with("<?xml") || t.starts_with('<') && ext == "xml" {
            return "xml";
        }
        return match ext {
            "md" | "markdown" => "markdown",
            "csv" => "csv",
            "tsv" | "tab" => "tsv",
            "json" | "jsonl" | "geojson" => "json",
            "html" | "htm" => "html",
            "xml" => "xml",
            e if SOURCE_EXTS.iter().any(|(x, _)| *x == e) => "source",
            _ => "text",
        };
    }
    "binary"
}

fn is_mostly_text(h: &[u8]) -> bool {
    // A 4096-byte window may cut a UTF-8 sequence; accept if only the tail is invalid.
    match std::str::from_utf8(h) {
        Ok(_) => true,
        Err(e) => e.valid_up_to() + 4 >= h.len(),
    }
}

/// Inspect ZIP central directory names to classify OOXML/ODF/EPUB.
pub fn classify_zip(path: &Path) -> Option<&'static str> {
    let f = std::fs::File::open(path).ok()?;
    let mut z = zip::ZipArchive::new(f).ok()?;
    if z.len() > 100_000 {
        return None;
    }
    let has = |p: &str| z.file_names().any(|n| n.starts_with(p));
    let (word, xl, ppt, ct) = (
        has("word/"),
        has("xl/"),
        has("ppt/"),
        has("[Content_Types].xml"),
    );
    if ct && word {
        return Some("docx");
    }
    if ct && xl {
        return Some("xlsx");
    }
    if ct && ppt {
        return Some("pptx");
    }
    if let Ok(mut m) = z.by_name("mimetype") {
        let mut s = String::new();
        let _ = (&mut m).take(256).read_to_string(&mut s);
        return match s.trim() {
            "application/vnd.oasis.opendocument.text" => Some("odt"),
            "application/vnd.oasis.opendocument.spreadsheet" => Some("ods"),
            "application/vnd.oasis.opendocument.presentation" => Some("odp"),
            "application/epub+zip" => Some("epub"),
            _ => None,
        };
    }
    None
}

pub fn detect_file(path: &Path) -> std::io::Result<Detected> {
    let mut f = std::fs::File::open(path)?;
    let mut head = vec![0u8; 4096];
    let mut n = 0;
    while n < head.len() {
        let r = f.read(&mut head[n..])?;
        if r == 0 {
            break;
        }
        n += r;
    }
    head.truncate(n);
    let ext = ext_of(path);
    let id = detect_bytes(&head, &ext, || classify_zip(path));
    let sig: String = head.iter().take(16).map(|b| format!("{b:02x}")).collect();

    // Source-code files report the language as description.
    let (id, desc, spec_) = if id == "source" {
        let lang = SOURCE_EXTS
            .iter()
            .find(|(e, _)| *e == ext)
            .map(|(_, l)| *l)
            .unwrap_or("source");
        (
            "source".to_string(),
            format!("{lang} source code"),
            spec("text"),
        )
    } else {
        let sp = spec(id);
        (sp.id.to_string(), sp.desc.to_string(), sp)
    };
    let (cat, mime) = if id == "source" {
        (Source, "text/plain")
    } else {
        (spec_.cat, spec_.mime)
    };

    let known_text_ext = spec("text").exts.contains(&ext.as_str())
        || SOURCE_EXTS.iter().any(|(e, _)| *e == ext)
        || [
            "md", "markdown", "csv", "tsv", "json", "html", "htm", "xml", "svg",
        ]
        .contains(&ext.as_str());
    let ext_match = if ext.is_empty() || id == "binary" {
        ExtensionMatch::Unknown
    } else if id == "source" || id == "text" {
        // Text content is compatible with any text-like extension.
        if known_text_ext {
            ExtensionMatch::Match
        } else {
            ExtensionMatch::Unknown
        }
    } else if spec_.exts.contains(&ext.as_str()) {
        ExtensionMatch::Match
    } else {
        ExtensionMatch::Mismatch
    };
    Ok(Detected {
        info: FormatInfo {
            id,
            description: desc,
            mime: mime.into(),
            category: cat,
            extension_match: ext_match,
            signature_hex: sig,
        },
        head,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(bytes: &[u8], ext: &str) -> &'static str {
        detect_bytes(bytes, ext, || None)
    }
    #[test]
    fn signatures() {
        assert_eq!(d(&[0xFF, 0xD8, 0xFF, 0xE0], "x"), "jpeg");
        assert_eq!(d(b"%PDF-1.7", "x"), "pdf");
        assert_eq!(d(b"GIF89a....", "x"), "gif");
        assert_eq!(d(b"\0\0\0\x18ftypheic....", "x"), "heic");
        assert_eq!(d(b"\0\0\0\x18ftypisom....", "x"), "mp4");
        assert_eq!(d(b"\0\0\0\x14ftypqt  ....", "x"), "mov");
        assert_eq!(d(b"RIFF\0\0\0\0WEBPVP8 ", "x"), "webp");
        assert_eq!(d(&[0xFF, 0xF1, 0x50, 0x80], "x"), "aac");
        assert_eq!(d(&[0xFF, 0xFB, 0x90, 0x00], "x"), "mp3");
    }
    #[test]
    fn text_by_ext() {
        assert_eq!(d(b"fn main(){}", "rs"), "source");
        assert_eq!(d(b"# hi", "md"), "markdown");
        assert_eq!(d(b"hello", "txt"), "text");
        assert_eq!(d(b"<!DOCTYPE html><html>", "txt"), "html");
        assert_eq!(d(&[0, 1, 2, 3], "txt"), "binary");
    }
}
