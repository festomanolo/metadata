//! Git history reader (via the `git` binary, hardened, read-only). Commit dates are the *committer's*
//! clock and can be rewritten (amend, rebase, GIT_COMMITTER_DATE): never independent evidence.
use metadata_core::{time, MetaError, Result, TimeValue};
use serde::Serialize;
use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize)]
pub struct Commit {
    pub hash: String,
    pub parents: Vec<String>,
    pub author_name: String,
    pub author_email: String,
    pub author_date: TimeValue,
    pub commit_date: TimeValue,
    pub subject: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitHistory {
    pub commits: Vec<Commit>,
    pub truncated: bool,
}

pub fn has_repo(dir: &Path) -> bool {
    dir.join(".git").exists()
}

pub fn read_history(dir: &Path, max_commits: usize, timeout: Duration) -> Result<Option<GitHistory>> {
    if !has_repo(dir) {
        return Ok(None);
    }
    let git = which_git().ok_or_else(|| MetaError::BackendUnavailable { backend: "git".into(), reason: "git not found in PATH".into() })?;
    let n = (max_commits + 1).to_string();
    let args: Vec<&OsStr> = [
        "--no-pager", "-c", "core.fsmonitor=false", "-c", "core.pager=cat", "-C",
    ]
    .iter()
    .map(OsStr::new)
    .chain([dir.as_os_str()])
    .chain(
        ["log", "--all", "--date-order", "-n", &n, "--format=%H%x1f%P%x1f%an%x1f%ae%x1f%aI%x1f%cI%x1f%s%x1e"]
            .iter()
            .map(OsStr::new),
    )
    .collect();
    let mut child = Command::new(&git)
        .args(&args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| MetaError::BackendFailed { backend: "git".into(), reason: e.to_string() })?;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let t_out = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = std::io::Read::read_to_end(&mut std::io::Read::take(&mut stdout, 256 << 20), &mut b);
        b
    });
    let t_err = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = std::io::Read::read_to_end(&mut std::io::Read::take(&mut stderr, 64 << 10), &mut b);
        b
    });
    let start = Instant::now();
