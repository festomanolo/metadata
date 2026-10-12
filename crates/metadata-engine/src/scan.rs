//! Safe directory traversal and bounded-parallel scanning.
use crate::Engine;
use metadata_core::FileReport;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub recursive: bool,
    pub max_depth: usize,
    pub jobs: usize,
    pub include_hidden: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        ScanOptions { recursive: false, max_depth: 32, jobs: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(16), include_hidden: true }
    }
}

/// Collect regular files. Symlinks are never followed; they are returned in `skipped`.
pub struct Collected {
    pub files: Vec<PathBuf>,
    pub skipped: Vec<(PathBuf, String)>,
}

pub fn collect_files(paths: &[PathBuf], o: &ScanOptions) -> Collected {
    let mut files = vec![];
    let mut skipped = vec![];
    for p in paths {
        let md = match std::fs::symlink_metadata(p) {
            Ok(m) => m,
            Err(e) => { skipped.push((p.clone(), e.to_string())); continue; }
        };
        if md.is_file() { files.push(p.clone()); continue; }
        if md.file_type().is_symlink() { skipped.push((p.clone(), "symlink not followed".into())); continue; }
        if !md.is_dir() { skipped.push((p.clone(), "not a file or directory".into())); continue; }
        let depth = if o.recursive { o.max_depth } else { 1 };
        for e in walkdir::WalkDir::new(p).max_depth(depth).follow_links(false).sort_by_file_name() {
            match e {
                Ok(e) if e.file_type().is_file() => {
                    if !o.include_hidden && e.file_name().to_string_lossy().starts_with('.') { continue; }
                    files.push(e.into_path());
                }
                Ok(e) if e.file_type().is_symlink() => skipped.push((e.into_path(), "symlink not followed".into())),
                Ok(_) => {}
                Err(err) => skipped.push((err.path().map(Path::to_path_buf).unwrap_or_default(), err.to_string())),
            }
        }
    }
    Collected { files, skipped }
}

impl Engine {
    /// Inspect many files with a bounded rayon pool. Order of results matches input order.
    /// `progress(done, total)` is called after each file. Honors `opts.cancel`.
    pub fn inspect_many(&self, files: &[PathBuf], jobs: usize, progress: &(dyn Fn(usize, usize) + Sync)) -> Vec<FileReport> {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(jobs.max(1)).build().expect("thread pool");
        let done = AtomicUsize::new(0);
        let stop = AtomicBool::new(false);
        let total = files.len();
        let out: Vec<Option<FileReport>> = pool.install(|| {
            files.par_iter().map(|p| {
                if stop.load(Ordering::Relaxed) || self.opts.cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed)) {
                    stop.store(true, Ordering::Relaxed);
                    return None;
                }
                let r = self.inspect(p);
                progress(done.fetch_add(1, Ordering::Relaxed) + 1, total);
                Some(r)
            }).collect()
        });
        out.into_iter().flatten().collect()
    }
}
