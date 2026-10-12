//! Native image metadata: EXIF (kamadak-exif), JPEG segments (dims, JFIF, IPTC, XMP, ICC, comments), PNG chunks, GIF/BMP dims.
use super::xmp;
use crate::{Extraction, Options};
use metadata_core::{time, Field, Provenance, TimeValue};
use std::io::{BufReader, Read};
use std::path::Path;

const B: &str = "native";

fn f(group: &str, name: &str, v: impl Into<String>, prov: Provenance) -> Field {
    Field::new(group, name, v, B, prov)
}

pub fn extract(path: &Path, fmt_id: &str, opts: &Options, out: &mut Extraction) {
    let mut data = Vec::new();
    // Bounded read for structural parsing; EXIF uses the file reader separately.
    let limit = opts.limits.max_structural_bytes;
    match std::fs::File::open(path).and_then(|fh| fh.take(limit).read_to_end(&mut data)) {
        Ok(_) => {}
        Err(e) => return out.warn(B, format!("cannot read file: {e}")),
    }
    match fmt_id {
        "jpeg" => jpeg(&data, out),
        "png" => png(&data, out),
        "gif" => gif(&data, out),
        "bmp" => bmp(&data, out),
        "webp" => webp(&data, out),
        _ => {}
    }
    if matches!(fmt_id, "jpeg" | "tiff" | "png" | "webp" | "heic") {
        exif_fields(path, out);
    }
    if fmt_id == "tiff" || fmt_id.starts_with("raw") {
        out.fields
            .push(f("File", "Container", "TIFF-based", Provenance::Inferred));
    }
}

fn exif_fields(path: &Path, out: &mut Extraction) {
    let Ok(file) = std::fs::File::open(path) else {
        return;
    };
    let mut br = BufReader::new(file);
    let ex = match exif::Reader::new().read_from_container(&mut br) {
        Ok(e) => e,
        Err(exif::Error::NotFound(_)) => return,
        Err(e) => return out.warn(B, format!("EXIF unreadable: {e}")),
    };
    let offset_for = |tag: exif::Tag| -> Option<String> {
        let o = match tag {
            exif::Tag::DateTime => exif::Tag::OffsetTime,
            exif::Tag::DateTimeOriginal => exif::Tag::OffsetTimeOriginal,
            exif::Tag::DateTimeDigitized => exif::Tag::OffsetTimeDigitized,
            _ => return None,
        };
        ex.get_field(o, exif::In::PRIMARY)
            .map(|x| clean(&x.display_value().to_string()))
    };
    for fl in ex.fields() {
        let group = match (fl.tag.context(), fl.ifd_num) {
            (exif::Context::Gps, _) => "GPS",
            (exif::Context::Interop, _) => "Interop",
            (_, exif::In::THUMBNAIL) => "EXIF-Thumbnail",
            _ => "EXIF",
        };
        let name = fl.tag.to_string();
        let raw = clean(&fl.display_value().with_unit(&ex).to_string());
        let mut fld = f(group, &name, raw.clone(), Provenance::Stored);
        if matches!(
            fl.tag,
            exif::Tag::DateTime | exif::Tag::DateTimeOriginal | exif::Tag::DateTimeDigitized
        ) {
            fld = fld.with_time(time::parse_exif(&raw, offset_for(fl.tag).as_deref()));
        }
        out.fields.push(fld);
    }
    if ex
        .get_field(exif::Tag::JPEGInterchangeFormat, exif::In::THUMBNAIL)
        .is_some()
    {
        out.fields.push(f(
            "EXIF-Thumbnail",
            "EmbeddedThumbnail",
            "present",
            Provenance::Inferred,
        ));
    }
    // Decimal GPS position, computed from stored rationals.
    if let (Some(la), Some(lr), Some(lo), Some(lor)) = (
        gps_deg(&ex, exif::Tag::GPSLatitude),
        ex.get_field(exif::Tag::GPSLatitudeRef, exif::In::PRIMARY),
        gps_deg(&ex, exif::Tag::GPSLongitude),
        ex.get_field(exif::Tag::GPSLongitudeRef, exif::In::PRIMARY),
    ) {
        let s = if clean(&lr.display_value().to_string()).starts_with('S') {
            -1.0
        } else {
            1.0
        };
        let w = if clean(&lor.display_value().to_string()).starts_with('W') {
            -1.0
        } else {
            1.0
        };
        out.fields.push(f(
            "GPS",
            "GPSPositionDecimal",
            format!("{:.6}, {:.6}", s * la, w * lo),
            Provenance::Inferred,
        ));
    }
}

fn gps_deg(ex: &exif::Exif, tag: exif::Tag) -> Option<f64> {
    match &ex.get_field(tag, exif::In::PRIMARY)?.value {
        exif::Value::Rational(v) if v.len() >= 3 && v.iter().all(|r| r.denom != 0) => {
            Some(v[0].to_f64() + v[1].to_f64() / 60.0 + v[2].to_f64() / 3600.0)
        }
        _ => None,
    }
}

fn clean(s: &str) -> String {
    let s = s.trim();
    // ASCII values come quoted, possibly multiple: "a", "b"
    s.split("\", \"")
        .map(|p| p.trim_matches('"'))
        .collect::<Vec<_>>()
        .join(", ")
}

fn be16(d: &[u8], i: usize) -> Option<usize> {
    Some(u16::from_be_bytes([*d.get(i)?, *d.get(i + 1)?]) as usize)
}
fn be32(d: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(i..i + 4)?.try_into().ok()?))
}
fn le32(d: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(i..i + 4)?.try_into().ok()?))
}

fn jpeg(d: &[u8], out: &mut Extraction) {
    let mut i = 2;
    let mut seen_exif = false;
    let mut eoi = None;
    while i + 4 <= d.len() {
        if d[i] != 0xFF {
            i += 1;
            continue;
        }
        let m = d[i + 1];
        if m == 0xFF {
            i += 1;
            continue;
        }
        if m == 0xD9 {
            eoi = Some(i + 2);
            break;
        }
        if (0xD0..=0xD7).contains(&m) || m == 0x01 || m == 0x00 {
            i += 2;
            continue;
        }
        let Some(len) = be16(d, i + 2) else { break };
        if len < 2 {
            break;
        }
        let seg = &d[(i + 4).min(d.len())..(i + 2 + len).min(d.len())];
        match m {
            0xC0..=0xCF if !matches!(m, 0xC4 | 0xC8 | 0xCC) && seg.len() >= 6 => {
                let (h, w) = (be16(seg, 1).unwrap_or(0), be16(seg, 3).unwrap_or(0));
                out.fields.push(
                    f("File", "ImageWidth", w.to_string(), Provenance::Inferred)
                        .with_normalized(w.into()),
                );
                out.fields.push(
                    f("File", "ImageHeight", h.to_string(), Provenance::Inferred)
                        .with_normalized(h.into()),
                );
                out.fields.push(f(
                    "File",
                    "BitsPerSample",
                    seg[0].to_string(),
                    Provenance::Inferred,
                ));
                out.fields.push(f(
                    "File",
                    "ColorComponents",
                    seg[5].to_string(),
                    Provenance::Inferred,
                ));
                out.fields.push(f(
                    "File",
                    "Progressive",
                    (matches!(m, 0xC2 | 0xC6 | 0xCA | 0xCE)).to_string(),
                    Provenance::Inferred,
                ));
                if m == 0xDA {
                    break;
                }
            }
            0xE0 if seg.starts_with(b"JFIF\0") && seg.len() >= 9 => {
                out.fields.push(f(
                    "JFIF",
                    "JFIFVersion",
                    format!("{}.{:02}", seg[5], seg[6]),
                    Provenance::Stored,
                ));
                out.fields.push(f(
                    "JFIF",
                    "ResolutionUnit",
                    ["none", "inches", "cm"]
                        .get(seg[7] as usize)
                        .unwrap_or(&"unknown")
                        .to_string(),
                    Provenance::Stored,
                ));
                if let (Some(x), Some(y)) = (be16(seg, 8), be16(seg, 10)) {
                    out.fields
                        .push(f("JFIF", "XResolution", x.to_string(), Provenance::Stored));
                    out.fields
                        .push(f("JFIF", "YResolution", y.to_string(), Provenance::Stored));
                }
            }
            0xE1 if seg.starts_with(b"Exif\0\0") => {
                if seen_exif {
                    out.warn(B, "multiple EXIF segments present");
                }
                seen_exif = true;
            }
            0xE1 if seg.starts_with(b"http://ns.adobe.com/xap/1.0/\0") => {
                let x = String::from_utf8_lossy(&seg[29..]);
                for (k, v) in xmp::parse(&x) {
                    out.fields.push(
                        f(
                            "XMP",
                            k.split_once(':').map(|(_, n)| n).unwrap_or(&k),
                            v,
                            Provenance::Stored,
                        )
                        .with_xmp_time(),
                    );
                }
            }
            0xE2 if seg.starts_with(b"ICC_PROFILE\0") && seg.len() > 14 => icc(&seg[14..], out),
            0xED if seg.starts_with(b"Photoshop 3.0\0") => iptc(&seg[14..], out),
            0xFE => out.fields.push(f(
                "File",
                "Comment",
                String::from_utf8_lossy(seg).trim().to_string(),
                Provenance::Stored,
            )),
            _ => {}
        }
        if m == 0xDA {
            // start of scan: entropy-coded data follows; jump to EOI search
            let tail = &d[i + 2 + len..];
            if let Some(p) = tail.windows(2).rposition(|w| w == [0xFF, 0xD9]) {
                eoi = Some(i + 2 + len + p + 2);
            }
            break;
        }
        i += 2 + len;
    }
    if let Some(e) = eoi {
        if e < d.len() && d.len() - e > 16 {
            out.warn(
                B,
                format!(
                    "{} bytes of trailing data after JPEG end-of-image marker",
                    d.len() - e
                ),
            );
        }
    }
}

fn icc(p: &[u8], out: &mut Extraction) {
    if p.len() < 132 || &p[36..40] != b"acsp" {
        return out.warn(B, "malformed ICC profile header");
    }
    out.fields.push(f(
        "ICC",
        "ProfileVersion",
        format!("{}.{}", p[8], p[9] >> 4),
        Provenance::Stored,
    ));
    out.fields.push(f(
        "ICC",
        "ProfileClass",
        String::from_utf8_lossy(&p[12..16]).to_string(),
        Provenance::Stored,
    ));
    out.fields.push(f(
        "ICC",
        "ColorSpace",
        String::from_utf8_lossy(&p[16..20]).trim().to_string(),
        Provenance::Stored,
    ));
    out.fields.push(f(
        "ICC",
        "ConnectionSpace",
        String::from_utf8_lossy(&p[20..24]).trim().to_string(),
        Provenance::Stored,
    ));
    let n = be32(p, 128).unwrap_or(0) as usize;
    for t in 0..n.min(100) {
        let o = 132 + t * 12;
        if p.get(o..o + 4) == Some(b"desc") {
            let (off, sz) = (
                be32(p, o + 4).unwrap_or(0) as usize,
                be32(p, o + 8).unwrap_or(0) as usize,
            );
            if let Some(b) = p.get(off..off.saturating_add(sz).min(p.len())) {
                let s = if b.starts_with(b"desc") && b.len() > 12 {
                    String::from_utf8_lossy(&b[12..])
                        .trim_end_matches('\0')
                        .to_string()
                } else if b.starts_with(b"mluc") && b.len() > 28 {
                    let u: Vec<u16> = b[28..]
                        .chunks_exact(2)
                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                        .collect();
                    String::from_utf16_lossy(&u)
                        .trim_end_matches('\0')
                        .to_string()
                } else {
                    String::new()
                };
                if !s.is_empty() {
                    out.fields
                        .push(f("ICC", "ProfileDescription", s, Provenance::Stored));
                }
            }
        }
    }
}

fn iptc(mut d: &[u8], out: &mut Extraction) {
    // Photoshop image resource blocks: "8BIM" id(2) pascal-name(even) size(4) data
    while d.len() >= 12 && &d[..4] == b"8BIM" {
        let id = be16(d, 4).unwrap_or(0);
        let nl = d[6] as usize;
        let name_len = (1 + nl + 1) & !1;
        let Some(size) = be32(d, 6 + name_len) else {
            break;
        };
        let start = 10 + name_len;
        let end = start.saturating_add(size as usize);
        let Some(body) = d.get(start..end.min(d.len())) else {
            break;
        };
        if id == 0x0404 {
            iptc_records(body, out);
        }
        d = &d[(end + (end & 1)).min(d.len())..];
        if end >= d.len() + start {
            break;
        }
    }
}

fn iptc_records(b: &[u8], out: &mut Extraction) {
    let mut i = 0;
    while i + 5 <= b.len() && b[i] == 0x1C {
        let (rec, ds) = (b[i + 1], b[i + 2]);
        let len = be16(b, i + 3).unwrap_or(0);
        let Some(v) = b.get(i + 5..i + 5 + len) else {
            break;
        };
        let name = match (rec, ds) {
            (2, 5) => "ObjectName",
            (2, 25) => "Keywords",
            (2, 55) => "DateCreated",
            (2, 80) => "By-line",
            (2, 90) => "City",
            (2, 92) => "Sub-location",
            (2, 95) => "Province-State",
            (2, 101) => "Country-PrimaryLocationName",
            (2, 105) => "Headline",
            (2, 110) => "Credit",
            (2, 115) => "Source",
            (2, 116) => "CopyrightNotice",
            (2, 120) => "Caption-Abstract",
            _ => "",
        };
        if !name.is_empty() {
            let s = String::from_utf8_lossy(v).to_string();
            if let Some(e) = out
                .fields
                .iter_mut()
                .find(|x| x.group == "IPTC" && x.name == name && name == "Keywords")
            {
                e.raw.push_str(&format!(", {s}"));
            } else {
                out.fields.push(f("IPTC", name, s, Provenance::Stored));
            }
        }
        i += 5 + len;
    }
}

fn png(d: &[u8], out: &mut Extraction) {
    let mut i = 8;
    let mut idat = 0u64;
    while let (Some(len), Some(ty)) = (be32(d, i), d.get(i + 4..i + 8)) {
        let len = len as usize;
        let body = d.get(i + 8..i + 8 + len).unwrap_or(&[]);
        match ty {
            b"IHDR" if body.len() >= 13 => {
                let (w, h) = (be32(body, 0).unwrap_or(0), be32(body, 4).unwrap_or(0));
                out.fields.push(
                    f("File", "ImageWidth", w.to_string(), Provenance::Inferred)
                        .with_normalized(w.into()),
                );
                out.fields.push(
                    f("File", "ImageHeight", h.to_string(), Provenance::Inferred)
                        .with_normalized(h.into()),
                );
                out.fields.push(f(
                    "PNG",
                    "BitDepth",
                    body[8].to_string(),
                    Provenance::Stored,
                ));
                let ct = [
                    "Grayscale",
                    "",
                    "RGB",
                    "Palette",
                    "Grayscale+Alpha",
                    "",
                    "RGBA",
                ];
                out.fields.push(f(
                    "PNG",
                    "ColorType",
                    ct.get(body[9] as usize).unwrap_or(&"unknown").to_string(),
                    Provenance::Stored,
                ));
                out.fields.push(f(
                    "PNG",
                    "Interlace",
                    if body[12] == 0 { "None" } else { "Adam7" }.to_string(),
                    Provenance::Stored,
                ));
            }
            b"tEXt" => {
                if let Some(p) = body.iter().position(|&b| b == 0) {
                    let (k, v) = (latin1(&body[..p]), latin1(&body[p + 1..]));
                    png_text(&k, v, out);
                }
            }
            b"iTXt" => {
                if let Some(p) = body.iter().position(|&b| b == 0) {
                    let k = latin1(&body[..p]);
                    // flag, method, lang\0, translated\0, text
                    if body.get(p + 1) == Some(&0) {
                        let rest = &body[(p + 3).min(body.len())..];
                        let mut parts = rest.splitn(3, |&b| b == 0);
                        parts.next();
                        parts.next();
                        if let Some(t) = parts.next() {
                            png_text(&k, String::from_utf8_lossy(t).to_string(), out);
                        }
                    } else {
                        out.warn(B, format!("compressed iTXt chunk '{k}' not decoded"));
                    }
                }
            }
            b"zTXt" => {
                if let Some(p) = body.iter().position(|&b| b == 0) {
                    out.warn(
                        B,
                        format!("compressed zTXt chunk '{}' not decoded", latin1(&body[..p])),
                    );
                }
            }
            b"tIME" if body.len() >= 7 => {
                let iso = format!(
                    "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                    be16(body, 0).unwrap_or(0),
                    body[2],
                    body[3],
                    body[4],
                    body[5],
                    body[6]
                );
                out.fields.push(
                    f("PNG", "ModifyDate", iso.clone(), Provenance::Stored)
                        .with_time(time::parse_iso(&iso)),
                );
            }
            b"pHYs" if body.len() >= 9 => out.fields.push(f(
                "PNG",
                "PixelsPerUnit",
                format!(
                    "{}x{} ({})",
                    be32(body, 0).unwrap_or(0),
                    be32(body, 4).unwrap_or(0),
                    if body[8] == 1 {
                        "per metre"
                    } else {
                        "unknown unit"
                    }
                ),
                Provenance::Stored,
            )),
            b"iCCP" => {
                if let Some(p) = body.iter().position(|&b| b == 0) {
                    out.fields.push(f(
                        "ICC",
                        "ProfileName",
                        latin1(&body[..p]),
                        Provenance::Stored,
                    ));
                }
            }
            b"eXIf" => {} // handled by the EXIF reader
            b"IDAT" => idat += len as u64,
            _ => {}
        }
        i += 12 + len;
    }
    if idat > 0 {
        out.fields.push(f(
            "PNG",
            "ImageDataBytes",
            idat.to_string(),
            Provenance::Inferred,
        ));
    }
}

fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

fn png_text(k: &str, v: String, out: &mut Extraction) {
    if k == "XML:com.adobe.xmp" {
        for (n, val) in xmp::parse(&v) {
            out.fields.push(
                f(
                    "XMP",
                    n.split_once(':').map(|(_, n)| n).unwrap_or(&n),
                    val,
                    Provenance::Stored,
                )
                .with_xmp_time(),
            );
        }
    } else {
        let fld = f("PNG", k, v.clone(), Provenance::Stored);
        out.fields.push(if k.eq_ignore_ascii_case("Creation Time") {
            fld.with_time(time::parse_any(&v))
        } else {
            fld
        });
    }
}

fn gif(d: &[u8], out: &mut Extraction) {
    if d.len() < 13 {
        return;
    }
    out.fields.push(f(
        "GIF",
        "GIFVersion",
        String::from_utf8_lossy(&d[3..6]).to_string(),
        Provenance::Stored,
    ));
    let (w, h) = (
        u16::from_le_bytes([d[6], d[7]]),
        u16::from_le_bytes([d[8], d[9]]),
    );
    out.fields.push(
        f("File", "ImageWidth", w.to_string(), Provenance::Inferred).with_normalized(w.into()),
    );
    out.fields.push(
        f("File", "ImageHeight", h.to_string(), Provenance::Inferred).with_normalized(h.into()),
    );
    // count frames (image descriptors) and extract comments conservatively
    let frames = d.windows(1).filter(|w| w[0] == 0x2C).count().min(1);
    let _ = frames;
}

fn bmp(d: &[u8], out: &mut Extraction) {
    if let (Some(w), Some(h)) = (le32(d, 18), le32(d, 22)) {
        let h = (h as i32).unsigned_abs();
        out.fields.push(
            f("File", "ImageWidth", w.to_string(), Provenance::Inferred).with_normalized(w.into()),
        );
        out.fields.push(
            f("File", "ImageHeight", h.to_string(), Provenance::Inferred).with_normalized(h.into()),
        );
        if let Some(bits) = d.get(28..30) {
            out.fields.push(f(
                "BMP",
                "BitsPerPixel",
                u16::from_le_bytes([bits[0], bits[1]]).to_string(),
                Provenance::Stored,
            ));
        }
    }
}

fn webp(d: &[u8], out: &mut Extraction) {
    if d.len() < 30 {
        return;
    }
    match &d[12..16] {
        b"VP8X" => {
            let w = 1 + (d[24] as u32 | (d[25] as u32) << 8 | (d[26] as u32) << 16);
            let h = 1 + (d[27] as u32 | (d[28] as u32) << 8 | (d[29] as u32) << 16);
            out.fields.push(
                f("File", "ImageWidth", w.to_string(), Provenance::Inferred)
                    .with_normalized(w.into()),
            );
            out.fields.push(
                f("File", "ImageHeight", h.to_string(), Provenance::Inferred)
                    .with_normalized(h.into()),
            );
        }
        b"VP8 " => {
            let (w, h) = (
                u16::from_le_bytes([d[26], d[27]]) & 0x3FFF,
                u16::from_le_bytes([d[28], d[29]]) & 0x3FFF,
            );
            out.fields.push(
                f("File", "ImageWidth", w.to_string(), Provenance::Inferred)
                    .with_normalized(w.into()),
            );
            out.fields.push(
                f("File", "ImageHeight", h.to_string(), Provenance::Inferred)
                    .with_normalized(h.into()),
            );
        }
        _ => {}
    }
}

trait XmpTime {
    fn with_xmp_time(self) -> Field;
}
impl XmpTime for Field {
    fn with_xmp_time(self) -> Field {
        let n = self.name.to_ascii_lowercase();
        if n.ends_with("date") || n == "metadatadate" {
            let t: Option<TimeValue> = time::parse_any(&self.raw);
            self.with_time(t)
        } else {
            self
        }
    }
}
