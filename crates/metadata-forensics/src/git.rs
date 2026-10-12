//! Git history reader (via the `git` binary, hardened, read-only). Commit dates are the *committer's*
//! clock and can be rewritten (amend, rebase, GIT_COMMITTER_DATE): never independent evidence.
use metadata_core::{time, MetaError, Result, TimeValue};
use serde::Serialize;
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

pub fn read_history(
    dir: &Path,
    max_commits: usize,
    timeout: Duration,
) -> Result<Option<GitHistory>> {
    if !has_repo(dir) {
        return Ok(None);
    }
    let git = which_git().ok_or_else(|| MetaError::BackendUnavailable {
        backend: "git".into(),
        reason: "git not found in PATH".into(),
    })?;
    let n = (max_commits + 1).to_string();
    let fmt = "--format=%H%x1f%P%x1f%an%x1f%ae%x1f%aI%x1f%cI%x1f%s%x1e";
    let mut args: Vec<std::ffi::OsString> = [
        "--no-pager",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.pager=cat",
        "-C",
    ]
    .iter()
    .map(Into::into)
    .collect();
    args.push(dir.as_os_str().to_owned());
    args.extend(
        ["log", "--all", "--date-order", "-n", n.as_str(), fmt]
            .iter()
            .map(std::ffi::OsString::from),
    );
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
    let status = loop {
        if let Some(s) = child.try_wait().map_err(|e| MetaError::BackendFailed { backend: "git".into(), reason: e.to_string() })? {
            break s;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(MetaError::BackendFailed { backend: "git".into(), reason: "timed out".into() });
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let out = t_out.join().unwrap_or_default();
    let err = t_err.join().unwrap_or_default();
    if !status.success() {
        let msg = String::from_utf8_lossy(&err).trim().to_string();
        // An empty repository (no commits) is not an error.
        if msg.contains("does not have any commits") {
            return Ok(Some(GitHistory { commits: vec![], truncated: false }));
        }
        return Err(MetaError::BackendFailed { backend: "git".into(), reason: msg });
    }
    let mut commits = parse_log(&String::from_utf8_lossy(&out));
    let truncated = commits.len() > max_commits;
    commits.truncate(max_commits);
    Ok(Some(GitHistory { commits, truncated }))
}

fn which_git() -> Option<std::path::PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths).map(|d| d.join(if cfg!(windows) { "git.exe" } else { "git" })).find(|p| p.is_file())
}

pub fn parse_log(s: &str) -> Vec<Commit> {
    s.split('\x1e')
        .filter_map(|rec| {
            let rec = rec.trim_matches(['\n', '\r']);
            let f: Vec<&str> = rec.split('\x1f').collect();
            if f.len() < 7 {
                return None;
            }
            Some(Commit {
                hash: f[0].to_string(),
                parents: f[1].split_whitespace().map(String::from).collect(),
                author_name: f[2].to_string(),
                author_email: f[3].to_string(),
                author_date: time::parse_iso(f[4])?,
                commit_date: time::parse_iso(f[5])?,
                subject: f[6].to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_records() {
        let log = "aaa\x1fbbb ccc\x1fAnn\x1fa@x\x1f2024-01-02T03:04:05+02:00\x1f2024-01-02T03:04:06+02:00\x1ffix: thing\x1e\nbbb\x1f\x1fBob\x1fb@x\x1f2024-01-01T00:00:00Z\x1f2024-01-01T00:00:00Z\x1finit\x1e";
        let c = parse_log(log);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].parents, vec!["bbb", "ccc"]);
        assert_eq!(c[0].author_date.utc.as_deref(), Some("2024-01-02T01:04:05Z"));
        assert!(c[1].parents.is_empty());
    }
    #[test]
    fn non_repo_is_none() {
        let d = tempfile::tempdir().unwrap();
        assert!(read_history(d.path(), 10, Duration::from_secs(5)).unwrap().is_none());
    }
}
