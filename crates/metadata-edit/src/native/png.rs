//! PNG text-chunk editing (tEXt/iTXt). Chunks are rebuilt with fresh CRCs; image data is untouched.
use super::{Resolved, WProfile};
use metadata_core::{MetaError, Result};

fn crc(ty: &[u8], body: &[u8]) -> u32 {
    let mut c = flate2::Crc::new();
    c.update(ty);
    c.update(body);
    c.sum()
}

fn chunk(o: &mut Vec<u8>, ty: &[u8; 4], body: &[u8]) {
    o.extend((body.len() as u32).to_be_bytes());
    o.extend(ty);
    o.extend(body);
    o.extend(crc(ty, body).to_be_bytes());
}

pub fn key_ok(name: &str) -> bool {
    (1..=79).contains(&name.len())
        && name.chars().all(|c| (' '..='~').contains(&c))
        && !name.starts_with(' ')
        && !name.ends_with(' ')
}

pub fn apply(data: &[u8], ops: &[Resolved], profile: Option<WProfile>) -> Result<Vec<u8>> {
    if data.len() < 8 {
        return Err(MetaError::Other("truncated PNG".into()));
    }
    let strip_all = matches!(profile, Some(WProfile::Privacy | WProfile::All));
    if matches!(profile, Some(WProfile::Gps)) {
        return Err(MetaError::Unsupported(
            "native PNG editing cannot target GPS only; use the privacy profile or exiftool".into(),
        ));
    }
    let targets: Vec<String> = ops
        .iter()
        .map(|o| o.key.trim_start_matches("PNG:").to_string())
        .collect();
    let mut out = data[..8].to_vec();
    let mut i = 8;
    let mut inserted = false;
    let insert_new = |out: &mut Vec<u8>| {
        for o in ops.iter().filter(|o| o.value.is_some()) {
            let k = o.key.trim_start_matches("PNG:");
            let v = o.value.as_deref().unwrap();
            let mut b = k.as_bytes().to_vec();
            if v.chars().all(|c| (c as u32) < 256) {
                b.push(0);
                b.extend(v.chars().map(|c| c as u8));
                chunk(out, b"tEXt", &b);
            } else {
                b.extend([0, 0, 0, 0, 0]);
                b.extend(v.as_bytes());
                chunk(out, b"iTXt", &b);
            }
        }
    };
    while i + 12 <= data.len() {
        let len = u32::from_be_bytes(data[i..i + 4].try_into().unwrap()) as usize;
        let ty: [u8; 4] = data[i + 4..i + 8].try_into().unwrap();
        let end = i + 12 + len;
        if end > data.len() {
            return Err(MetaError::Other(
                "PNG chunk exceeds file length (corrupt)".into(),
            ));
        }
        let body = &data[i + 8..i + 8 + len];
        let key = matches!(&ty, b"tEXt" | b"iTXt" | b"zTXt")
            .then(|| {
                body.split(|&b| b == 0)
                    .next()
                    .map(|k| String::from_utf8_lossy(k).to_string())
            })
            .flatten();
        let drop = (strip_all && matches!(&ty, b"tEXt" | b"iTXt" | b"zTXt" | b"eXIf" | b"tIME"))
            || key.as_ref().is_some_and(|k| targets.contains(k));
        if &ty == b"IDAT" && !inserted {
            insert_new(&mut out);
            inserted = true;
        }
        if !drop {
            out.extend(&data[i..end]);
        }
        i = end;
    }
    if !inserted {
        return Err(MetaError::Other("PNG has no image data chunk".into()));
    }
    Ok(out)
}
