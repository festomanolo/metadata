use metadata_core::{FsInfo, TimeValue};
use std::path::Path;

pub fn fs_info(path: &Path) -> FsInfo {
    let sym = std::fs::symlink_metadata(path).map(|m| m.file_type().is_symlink()).unwrap_or(false);
    let Ok(m) = std::fs::metadata(path) else { return FsInfo { is_symlink: sym, ..Default::default() } };
    #[cfg(unix)]
    let permissions = {
        use std::os::unix::fs::PermissionsExt;
        Some(format!("{:04o}", m.permissions().mode() & 0o7777))
    };
    #[cfg(not(unix))]
    let permissions = Some(if m.permissions().readonly() { "readonly" } else { "read-write" }.to_string());
    FsInfo {
        created: m.created().ok().map(TimeValue::from_system),
        modified: m.modified().ok().map(TimeValue::from_system),
        accessed: m.accessed().ok().map(TimeValue::from_system),
        permissions,
        is_symlink: sym,
    }
}
