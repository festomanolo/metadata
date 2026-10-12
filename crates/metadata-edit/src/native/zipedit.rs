//! Rewrite a ZIP replacing selected parts while raw-copying every other entry (keeps compression, order, timestamps).
use metadata_core::{MetaError, Result};
use std::collections::HashMap;
use std::io::{Cursor, Read, Seek, Write};

pub fn read_part<R: Read + Seek>(
    z: &mut zip::ZipArchive<R>,
    name: &str,
    max: u64,
) -> Option<String> {
    let mut e = z.by_name(name).ok()?;
    if e.size() > max {
        return None;
    }
    let mut s = String::new();
    (&mut e).take(max).read_to_string(&mut s).ok()?;
    Some(s)
}

pub fn rewrite(
    data: &[u8],
    replace: &HashMap<String, Vec<u8>>,
    skip: &[String],
) -> Result<Vec<u8>> {
    let err = |e: zip::result::ZipError| MetaError::Other(format!("ZIP rewrite failed: {e}"));
    let mut z = zip::ZipArchive::new(Cursor::new(data)).map_err(err)?;
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..z.len() {
        let name = z.by_index_raw(i).map_err(err)?.name().to_string();
        if skip.contains(&name) {
            continue;
        }
        if let Some(new) = replace.get(&name) {
            let old = z.by_index_raw(i).map_err(err)?;
            let mut o =
                zip::write::SimpleFileOptions::default().compression_method(old.compression());
            if let Some(t) = old.last_modified() {
                o = o.last_modified_time(t);
            }
            if let Some(m) = old.unix_mode() {
                o = o.unix_permissions(m);
            }
            let _ = &old;
            out.start_file(&name, o).map_err(err)?;
            out.write_all(new)
                .map_err(|e| MetaError::Other(e.to_string()))?;
        } else {
            let f = z.by_index_raw(i).map_err(err)?;
            out.raw_copy_file(f).map_err(err)?;
        }
    }
    Ok(out.finish().map_err(err)?.into_inner())
}
