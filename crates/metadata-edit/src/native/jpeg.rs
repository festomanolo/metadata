//! JPEG segment editing: comment set/remove and metadata stripping. Entropy-coded data is copied verbatim.
use super::{Resolved, WProfile};
use metadata_core::{MetaError, Result};

pub fn apply(data: &[u8], ops: &[Resolved], profile: Option<WProfile>) -> Result<Vec<u8>> {
    if !data.starts_with(&[0xFF, 0xD8]) {
        return Err(MetaError::Other("not a JPEG".into()));
    }
    if matches!(profile, Some(WProfile::Gps)) {
        return Err(MetaError::Unsupported("removing only GPS requires rewriting EXIF; use exiftool or the privacy profile (removes all EXIF)".into()));
    }
    let strip = matches!(profile, Some(WProfile::Privacy | WProfile::All));
    let mut set_comment: Option<&str> = None;
    let mut remove_comment = false;
    for o in ops {
        match &o.value {
            Some(v) => set_comment = Some(v),
            None => remove_comment = true,
        }
    }
    let mut out = vec![0xFF, 0xD8];
    let mut i = 2;
    let mut wrote_comment = set_comment.is_none();
    while i + 4 <= data.len() {
        if data[i] != 0xFF {
            return Err(MetaError::Other("corrupt JPEG segment structure".into()));
        }
        let m = data[i + 1];
        if m == 0xDA || m == 0xD9 {
            break;
        }
        if m == 0xFF {
            i += 1;
            continue;
        }
        let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        let end = i + 2 + len;
        if len < 2 || end > data.len() {
            return Err(MetaError::Other("corrupt JPEG segment length".into()));
        }
        let seg = &data[i..end];
        let drop = match m {
            0xFE => strip || remove_comment || set_comment.is_some(),
            0xE1 | 0xED => strip,               // EXIF/XMP and Photoshop/IPTC
            0xE3..=0xEC | 0xEE | 0xEF => strip, // APP3..APP15 except APP2 (ICC) / APP14 (Adobe color transform)
            _ => false,
        };
        if !drop {
            out.extend(seg);
        }
        if !wrote_comment && (m == 0xE0 || m == 0xE1 || m == 0xDB || m == 0xC0) {
            // place the new comment right after the first APP0 (or before the first table/frame)
            if m == 0xE0 || out.len() == 2 {
                let c = set_comment.unwrap();
                out.extend([0xFF, 0xFE]);
                out.extend(((c.len() + 2) as u16).to_be_bytes());
                out.extend(c.as_bytes());
                wrote_comment = true;
            }
        }
        i = end;
    }
    if !wrote_comment {
        let c = set_comment.unwrap();
        out.extend([0xFF, 0xFE]);
        out.extend(((c.len() + 2) as u16).to_be_bytes());
        out.extend(c.as_bytes());
    }
    out.extend(&data[i..]);
    if let Some(c) = set_comment {
        if c.len() > 65_000 {
            return Err(MetaError::InvalidArgument(
                "comment too long (max 65000 bytes)".into(),
            ));
        }
    }
    Ok(out)
}
