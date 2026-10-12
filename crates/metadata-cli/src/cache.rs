//! Opt-in extraction cache keyed by (content SHA-256, extraction settings).
use crate::config;
use metadata_core::hash::sha256_file;
use metadata_core::FileReport;
use metadata_engine::Engine;
use metadata_storage::Store;
use rayon::prelude::*;
use std::path::PathBuf;

fn config_key(e: &Engine) -> String {
    let b = e.backends.status();
    let vers: Vec<String> = b
        .iter()
        .map(|s| format!("{}={}", s.name, s.version.clone().unwrap_or_default()))
        .collect();
    format!(
        "{}|all={}|deep={}|ext={}|{}",
        env!("CARGO_PKG_VERSION"),
        e.opts.all,
        e.opts.deep,
        e.opts.use_external,
        vers.join(",")
    )
}

/// Like `Engine::inspect_many`, but serves unchanged-content files from the cache. Falls back silently
/// (with a warning) if the cache cannot be opened.
pub fn inspect_many_cached(
    engine: &Engine,
    files: &[PathBuf],
    jobs: usize,
    progress: &(dyn Fn(usize, usize) + Sync),
) -> Vec<FileReport> {
    let path = config::config_path().with_file_name("cache.sqlite");
    let store = match Store::open(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("warning: cache unavailable ({e}); continuing without it");
            return engine.inspect_many(files, jobs, progress);
        }
    };
    let key = config_key(engine);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs.max(1))
        .build()
        .expect("pool");
    let hashes: Vec<Option<String>> =
        pool.install(|| files.par_iter().map(|p| sha256_file(p).ok()).collect());
    let mut out: Vec<Option<FileReport>> = vec![None; files.len()];
    let mut misses = vec![];
    for (i, (p, h)) in files.iter().zip(&hashes).enumerate() {
        let hit = h.as_ref().and_then(|h| store.cache_get(h, &key));
        match hit {
            Some(mut r) => {
                // content is identical; location and filesystem facts are not
                let fresh_fs = metadata_engine::fsinfo::fs_info(p);
                r.path = p.display().to_string();
                r.filesystem = fresh_fs;
                out[i] = Some(r);
            }
            None => misses.push(i),
        }
    }
    let total = files.len();
    let hits = total - misses.len();
    eprintln!("cache: {hits} hit(s), {} miss(es)", misses.len());
    let miss_files: Vec<PathBuf> = misses.iter().map(|&i| files[i].clone()).collect();
    let done_before = hits;
    let fresh = engine.inspect_many(&miss_files, jobs, &|d, _| progress(done_before + d, total));
    for (&i, r) in misses.iter().zip(fresh) {
        if r.error.is_none() {
            if let Some(h) = &r.sha256 {
                let _ = store.cache_put(h, &key, &r);
            }
        }
        out[i] = Some(r);
    }
    out.into_iter().flatten().collect()
}
