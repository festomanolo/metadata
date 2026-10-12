//! Lightweight source-project summary: manifests, languages, docs. Never executes anything.
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Default)]
pub struct ProjectSummary {
    pub manifests: Vec<String>,
    pub languages: BTreeMap<String, usize>,
    pub has_readme: bool,
    pub has_git: bool,
    pub source_file_count: usize,
}

const MANIFESTS: &[&str] = &[
    "Cargo.toml", "package.json", "requirements.txt", "pyproject.toml", "setup.py", "pom.xml", "build.gradle", "build.gradle.kts",
    "go.mod", "Gemfile", "composer.json", "CMakeLists.txt", "Makefile", "pubspec.yaml", "Package.swift", "environment.yml",
];

const SKIP_DIRS: &[&str] = &[".git", "node_modules", "target", "__pycache__", ".venv", "venv", "build", "dist"];

/// `files` are paths from a scan; `root` is used to relativize and detect skipped dirs.
pub fn summarize(root: &Path, files: &[std::path::PathBuf]) -> ProjectSummary {
    let mut s = ProjectSummary { has_git: crate::git::has_repo(root), ..Default::default() };
    for f in files {
        let rel = f.strip_prefix(root).unwrap_or(f);
        if rel.components().any(|c| SKIP_DIRS.contains(&c.as_os_str().to_string_lossy().as_ref())) {
            continue;
        }
        let name = rel.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if MANIFESTS.contains(&name.as_str()) {
            s.manifests.push(rel.display().to_string());
        }
        if name.to_ascii_lowercase().starts_with("readme") {
            s.has_readme = true;
        }
        if let Some(ext) = f.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()) {
            if let Some((_, lang)) = metadata_engine::detect::SOURCE_EXTS.iter().find(|(e, _)| *e == ext) {
                *s.languages.entry(lang.to_string()).or_default() += 1;
                s.source_file_count += 1;
            }
        }
    }
    s
}
