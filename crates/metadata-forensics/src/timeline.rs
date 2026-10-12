//! Timeline construction. Sources stay separate and labeled; none is independent historical proof.
use crate::claims::{parse_claim_time, Claims};
use crate::git::GitHistory;
use metadata_core::{FileReport, Provenance, TimeValue};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Declared,
    Embedded,
    Archive,
    Filesystem,
    Git,
    Intake,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Declared => "declared",
            Source::Embedded => "embedded",
            Source::Archive => "archive",
            Source::Filesystem => "filesystem",
            Source::Git => "git",
            Source::Intake => "intake",
        }
    }
    pub fn reliability(self) -> &'static str {
        match self {
            Source::Declared => "Claimed by the assessor or student; not evidence.",
            Source::Embedded => "Stored in the file by an application or device clock; editable and carried across copies/templates.",
            Source::Archive => "Timestamp recorded by the archiving tool from the source file; no timezone.",
            Source::Filesystem => "Changed by copying, downloading, extracting or syncing; weak evidence alone.",
            Source::Git => "Recorded by the committer's clock; can be rewritten (amend, rebase, environment overrides).",
            Source::Intake => "Recorded by this tool's host clock at intake; not independently verified.",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Created,
    Modified,
    Other,
}

#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub time: TimeValue,
    pub source: Source,
    pub role: Role,
    /// Path relative to the analysis root, or a label such as `git:<hash>`.
    pub file: String,
    pub field: String,
    pub value: String,
}

pub fn role_of(name: &str) -> Role {
    let n = name.to_ascii_lowercase().replace([' ', '_', '-'], "");
    if [
        "created",
        "createdate",
        "creationdate",
        "creationtime",
        "datetimeoriginal",
        "datetimedigitized",
        "datecreated",
        "creatim",
        "mediacreatedate",
        "trackcreatedate",
    ]
    .contains(&n.as_str())
    {
        Role::Created
    } else if [
        "modified",
        "modifydate",
        "moddate",
        "date",
        "datetime",
        "lastmodified",
        "revtim",
        "metadatadate",
        "mediamodifydate",
        "trackmodifydate",
    ]
    .contains(&n.as_str())
    {
        Role::Modified
    } else {
        Role::Other
    }
}

pub fn rel(root: &Path, p: &str) -> String {
    Path::new(p)
        .strip_prefix(root)
        .unwrap_or(Path::new(p))
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Events from files only (no git/declared).
pub fn file_events(root: &Path, reports: &[FileReport]) -> Vec<Event> {
    let mut v = vec![];
    for r in reports {
        let file = rel(root, &r.path);
        for (name, t) in [
            ("created", &r.filesystem.created),
            ("modified", &r.filesystem.modified),
        ] {
            if let Some(t) = t {
                v.push(Event {
                    time: t.clone(),
                    source: Source::Filesystem,
                    role: if name == "created" {
                        Role::Created
                    } else {
                        Role::Modified
                    },
                    file: file.clone(),
                    field: format!("Filesystem:{name}"),
                    value: t.iso.clone(),
                });
            }
        }
        for f in &r.fields {
            let Some(t) = &f.time else { continue };
            let (source, role) = if f.group == "Archive" {
                if f.name == "Entry" {
                    (Source::Archive, Role::Modified)
                } else {
                    continue;
                } // min/max summaries duplicate entries
            } else if f.provenance == Provenance::Stored {
                (Source::Embedded, role_of(&f.name))
            } else {
                continue;
            };
            let field = if source == Source::Archive {
                format!("Archive:{}", f.raw)
            } else {
                f.key()
            };
            v.push(Event {
                time: t.clone(),
                source,
                role,
                file: file.clone(),
                field,
                value: f.raw.clone(),
            });
        }
    }
    v
}

pub fn build(
    root: &Path,
    reports: &[FileReport],
    git: Option<&GitHistory>,
    intake: Option<(&str, &TimeValue)>,
    claims: Option<&Claims>,
) -> Vec<Event> {
    let mut v = file_events(root, reports);
    if let Some(g) = git {
        for c in &g.commits {
            v.push(Event {
                time: c.author_date.clone(),
                source: Source::Git,
                role: Role::Created,
                file: format!("git:{}", &c.hash[..c.hash.len().min(10)]),
                field: "author_date".into(),
                value: c.subject.clone(),
            });
        }
    }
    if let Some((label, t)) = intake {
        v.push(Event {
            time: t.clone(),
            source: Source::Intake,
            role: Role::Other,
            file: "(intake)".into(),
            field: label.into(),
            value: t.iso.clone(),
        });
    }
    if let Some(c) = claims {
        let mut push = |name: &str, d: &Option<String>| {
            if let Some(Ok(t)) = d.as_deref().map(parse_claim_time) {
                v.push(Event {
                    value: t.iso.clone(),
                    time: t,
                    source: Source::Declared,
                    role: Role::Other,
                    file: "(claim)".into(),
                    field: name.into(),
                });
            }
        };
        push("claimed start", &c.start);
        push("claimed completion", &c.completion);
        push("deadline", &c.deadline);
        for m in &c.milestones {
            push(&format!("milestone: {}", m.name), &Some(m.date.clone()));
        }
    }
    v.sort_by(|a, b| {
        a.time
            .sort_key()
            .cmp(&b.time.sort_key())
            .then(a.source.cmp(&b.source))
            .then(a.file.cmp(&b.file))
    });
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roles() {
        assert_eq!(role_of("DateTimeOriginal"), Role::Created);
        assert_eq!(role_of("creation_time"), Role::Created);
        assert_eq!(role_of("Creation Time"), Role::Created);
        assert_eq!(role_of("ModifyDate"), Role::Modified);
        assert_eq!(role_of("Pages"), Role::Other);
    }
}
