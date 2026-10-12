//! Builders for synthetic, license-free test fixtures. Dev-only.
use std::io::Write;

struct Ent {
    tag: u16,
    ty: u16,
    count: u32,
    data: Vec<u8>,
}

fn ascii(tag: u16, s: &str) -> Ent {
    let mut d = s.as_bytes().to_vec();
    d.push(0);
    Ent {
        tag,
        ty: 2,
        count: d.len() as u32,
        data: d,
    }
}
fn long(tag: u16, v: u32) -> Ent {
    Ent {
        tag,
        ty: 4,
        count: 1,
        data: v.to_le_bytes().to_vec(),
    }
}
fn rationals(tag: u16, v: &[(u32, u32)]) -> Ent {
    Ent {
        tag,
        ty: 5,
        count: v.len() as u32,
        data: v
            .iter()
            .flat_map(|(n, d)| n.to_le_bytes().into_iter().chain(d.to_le_bytes()))
            .collect(),
    }
}
fn ifd_size(n: usize) -> u32 {
    (2 + 12 * n + 4) as u32
}

fn write_ifd(ents: &[Ent], blob: &mut Vec<u8>, blob_base: u32) -> Vec<u8> {
    let mut o = (ents.len() as u16).to_le_bytes().to_vec();
    for e in ents {
        o.extend(e.tag.to_le_bytes());
        o.extend(e.ty.to_le_bytes());
        o.extend(e.count.to_le_bytes());
        if e.data.len() <= 4 {
            let mut v = e.data.clone();
            v.resize(4, 0);
            o.extend(v);
        } else {
            o.extend((blob_base + blob.len() as u32).to_le_bytes());
            blob.extend(&e.data);
            if blob.len() % 2 == 1 {
                blob.push(0);
            }
        }
    }
    o.extend([0, 0, 0, 0]);
    o
}

/// Little-endian TIFF/EXIF block.
#[derive(Default)]
pub struct ExifSpec<'a> {
    pub artist: Option<&'a str>,
    pub software: Option<&'a str>,
    pub date_time_original: Option<&'a str>,
    pub offset_time_original: Option<&'a str>,
    /// (lat, lon) in decimal degrees; positive = N/E, negative = S/W.
    pub gps: Option<(f64, f64)>,
}

fn deg_rat(v: f64) -> Vec<(u32, u32)> {
    let v = v.abs();
    let d = v.floor();
    let m = ((v - d) * 60.0).floor();
    let s = ((v - d) * 60.0 - m) * 60.0;
    vec![
        (d as u32, 1),
        (m as u32, 1),
        ((s * 100.0).round() as u32, 100),
    ]
}

pub fn tiff_exif(s: &ExifSpec) -> Vec<u8> {
    let mut exif_ents = vec![];
    if let Some(d) = s.date_time_original {
        exif_ents.push(ascii(0x9003, d));
    }
    if let Some(o) = s.offset_time_original {
        exif_ents.push(ascii(0x9011, o));
    }
    let mut gps_ents = vec![];
    if let Some((la, lo)) = s.gps {
        gps_ents.push(ascii(0x0001, if la >= 0.0 { "N" } else { "S" }));
        gps_ents.push(rationals(0x0002, &deg_rat(la)));
        gps_ents.push(ascii(0x0003, if lo >= 0.0 { "E" } else { "W" }));
        gps_ents.push(rationals(0x0004, &deg_rat(lo)));
    }
    let mut n0 = 0;
    if s.artist.is_some() {
        n0 += 1;
    }
    if s.software.is_some() {
        n0 += 1;
    }
    if !exif_ents.is_empty() {
        n0 += 1;
    }
    if !gps_ents.is_empty() {
        n0 += 1;
    }
    let s0 = ifd_size(n0);
    let s1 = if exif_ents.is_empty() {
        0
    } else {
        ifd_size(exif_ents.len())
    };
    let s2 = if gps_ents.is_empty() {
        0
    } else {
        ifd_size(gps_ents.len())
    };
    let (off_exif, off_gps, blob_base) = (8 + s0, 8 + s0 + s1, 8 + s0 + s1 + s2);
    let mut ifd0 = vec![];
    if let Some(a) = s.software {
        ifd0.push(ascii(0x0131, a));
    }
    if let Some(a) = s.artist {
        ifd0.push(ascii(0x013B, a));
    }
    if !exif_ents.is_empty() {
        ifd0.push(long(0x8769, off_exif));
    }
    if !gps_ents.is_empty() {
        ifd0.push(long(0x8825, off_gps));
    }
    let mut blob = vec![];
    let mut out = vec![b'I', b'I', 0x2A, 0, 8, 0, 0, 0];
    out.extend(write_ifd(&ifd0, &mut blob, blob_base));
    if !exif_ents.is_empty() {
        out.extend(write_ifd(&exif_ents, &mut blob, blob_base));
    }
    if !gps_ents.is_empty() {
        out.extend(write_ifd(&gps_ents, &mut blob, blob_base));
    }
    out.extend(blob);
    out
}

/// Minimal valid-enough JPEG (SOI, optional EXIF APP1, optional comment, SOF0 w×h, SOS, EOI).
pub fn jpeg(exif: Option<&ExifSpec>, comment: Option<&str>, w: u16, h: u16) -> Vec<u8> {
    let mut o = vec![0xFF, 0xD8];
    if let Some(e) = exif {
        let t = tiff_exif(e);
        o.extend([0xFF, 0xE1]);
        o.extend(((t.len() + 8) as u16).to_be_bytes());
        o.extend(b"Exif\0\0");
        o.extend(t);
    }
    if let Some(c) = comment {
        o.extend([0xFF, 0xFE]);
        o.extend(((c.len() + 2) as u16).to_be_bytes());
        o.extend(c.as_bytes());
    }
    o.extend([0xFF, 0xC0, 0, 11, 8]);
    o.extend(h.to_be_bytes());
    o.extend(w.to_be_bytes());
    o.extend([1, 1, 0x11, 0]);
    o.extend([0xFF, 0xDA, 0, 8, 1, 1, 0, 0, 0x3F, 0]);
    o.extend([0x12, 0x34, 0x56]);
    o.extend([0xFF, 0xD9]);
    o
}

pub fn png(w: u32, h: u32, texts: &[(&str, &str)]) -> Vec<u8> {
    fn chunk(o: &mut Vec<u8>, ty: &[u8; 4], body: &[u8]) {
        o.extend((body.len() as u32).to_be_bytes());
        o.extend(ty);
        o.extend(body);
        o.extend([0, 0, 0, 0]); // CRC not validated by the reader
    }
    let mut o = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = vec![];
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    chunk(&mut o, b"IHDR", &ihdr);
    for (k, v) in texts {
        let mut b = k.as_bytes().to_vec();
        b.push(0);
        b.extend(v.as_bytes());
        chunk(&mut o, b"tEXt", &b);
    }
    chunk(
        &mut o,
        b"IDAT",
        &[0x78, 0x9C, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01],
    );
    chunk(&mut o, b"IEND", &[]);
    o
}

pub fn zip_with(files: &[(&str, &[u8])], mtime: Option<(u16, u8, u8, u8, u8, u8)>) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        for (name, data) in files {
            let mut opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            if let Some((y, mo, d, h, mi, s)) = mtime {
                opts = opts.last_modified_time(
                    zip::DateTime::from_date_and_time(y, mo, d, h, mi, s).unwrap(),
                );
            }
            z.start_file(*name, opts).unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
    }
    buf.into_inner()
}

pub struct DocxSpec<'a> {
    pub creator: &'a str,
    pub last_modified_by: &'a str,
    pub created: &'a str,
    pub modified: &'a str,
    pub revision: u32,
    pub tracked_insertions: u32,
    pub application: &'a str,
}

pub fn docx(s: &DocxSpec) -> Vec<u8> {
    let core = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/"><dc:title>Thesis</dc:title><dc:creator>{}</dc:creator><cp:lastModifiedBy>{}</cp:lastModifiedBy><cp:revision>{}</cp:revision><dcterms:created xsi:type="dcterms:W3CDTF" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">{}</dcterms:created><dcterms:modified xsi:type="dcterms:W3CDTF" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">{}</dcterms:modified></cp:coreProperties>"#,
        s.creator, s.last_modified_by, s.revision, s.created, s.modified
    );
    let app = format!(
        r#"<?xml version="1.0"?><Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>{}</Application><Pages>3</Pages><TotalTime>42</TotalTime></Properties>"#,
        s.application
    );
    let ins: String = (0..s.tracked_insertions)
        .map(|i| format!(r#"<w:ins w:id="{i}" w:author="x"><w:r><w:t>a</w:t></w:r></w:ins>"#))
        .collect();
    let doc = format!(
        r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p>{ins}</w:p></w:body></w:document>"#
    );
    zip_with(
        &[
            ("[Content_Types].xml", br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>"#),
            ("word/document.xml", doc.as_bytes()),
            ("docProps/core.xml", core.as_bytes()),
            ("docProps/app.xml", app.as_bytes()),
        ],
        Some((2024, 3, 1, 10, 0, 0)),
    )
}

pub fn odt(creator: &str, created: &str) -> Vec<u8> {
    let meta = format!(
        r#"<?xml version="1.0"?><office:document-meta xmlns:office="o" xmlns:meta="m" xmlns:dc="d"><office:meta><meta:generator>TestWriter/1.0</meta:generator><dc:title>T</dc:title><meta:initial-creator>{creator}</meta:initial-creator><meta:creation-date>{created}</meta:creation-date><meta:editing-cycles>3</meta:editing-cycles><meta:document-statistic meta:page-count="2" meta:word-count="10"/></office:meta></office:document-meta>"#
    );
    zip_with(
        &[
            ("mimetype", b"application/vnd.oasis.opendocument.text"),
            ("meta.xml", meta.as_bytes()),
            ("content.xml", b"<x/>"),
        ],
        None,
    )
}

/// Hand-built PDF with a correct xref. `incremental` appends a second revision.
pub fn pdf(
    title: &str,
    author: &str,
    producer: &str,
    creation_date: &str,
    incremental: bool,
) -> Vec<u8> {
    let mut o = b"%PDF-1.4\n".to_vec();
    let mut offs = vec![];
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>".to_string(),
        format!("<< /Title ({title}) /Author ({author}) /Producer ({producer}) /CreationDate ({creation_date}) >>"),
    ];
    for (i, b) in objs.iter().enumerate() {
        offs.push(o.len());
        o.extend(format!("{} 0 obj\n{}\nendobj\n", i + 1, b).bytes());
    }
    let xref = o.len();
    o.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
    for off in &offs {
        o.extend(format!("{off:010} 00000 n \n").bytes());
    }
    o.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 4 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        )
        .bytes(),
    );
    if incremental {
        let new4 = o.len();
        o.extend(format!("4 0 obj\n<< /Title ({title}) /Author ({author}) /Producer (Edited) /CreationDate ({creation_date}) >>\nendobj\n").bytes());
        let x2 = o.len();
        o.extend(format!("xref\n4 1\n{new4:010} 00000 n \ntrailer\n<< /Size {} /Root 1 0 R /Info 4 0 R /Prev {xref} >>\nstartxref\n{x2}\n%%EOF\n", objs.len() + 1).bytes());
    }
    o
}

pub fn tar_gz(files: &[(&str, &[u8], u64)]) -> Vec<u8> {
    let enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut b = tar::Builder::new(enc);
    for (name, data, mtime) in files {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o644);
        h.set_mtime(*mtime);
        h.set_cksum();
        b.append_data(&mut h, name, *data).unwrap();
    }
    b.into_inner().unwrap().finish().unwrap()
}
