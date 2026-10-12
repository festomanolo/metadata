//! Local SQLite store: assessment cases, submissions, reviewer dispositions and a metadata cache.
//!
//! The schema is versioned with `PRAGMA user_version`. Evidence rows (submissions) are append-only through
//! this API: reviewer decisions live in separate tables so original assessments are never rewritten.
use chrono::{SecondsFormat, Utc};
use metadata_core::{FileReport, MetaError, Result};
use metadata_forensics::assess::{Assessment, FindingState, FindingStatus};
use metadata_forensics::claims::Claims;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

pub const SCHEMA_VERSION: i32 = 1;

fn db<T>(r: rusqlite::Result<T>) -> Result<T> {
    r.map_err(|e| MetaError::Other(format!("database error: {e}")))
}
fn js<T: Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|e| MetaError::Other(e.to_string()))
}
fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub struct Store {
    conn: Connection,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseRow {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub claims: Claims,
    pub submissions: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubmissionRow {
    pub id: i64,
    pub case_id: String,
    pub label: String,
    pub root: String,
    pub ingested_at: String,
    pub files: i64,
    pub findings: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct NoteRow {
    pub at: String,
    pub text: String,
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(d) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(d).map_err(|e| MetaError::io(d, e))?;
        }
        Self::init(db(Connection::open(path))?)
    }

    pub fn open_memory() -> Result<Store> {
        Self::init(db(Connection::open_in_memory())?)
    }

    fn init(conn: Connection) -> Result<Store> {
        db(conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;"))?;
        let v: i32 = db(conn.query_row("PRAGMA user_version", [], |r| r.get(0)))?;
        if v > SCHEMA_VERSION {
            return Err(MetaError::Unsupported(format!("database schema v{v} is newer than this build (v{SCHEMA_VERSION}); upgrade metadata")));
        }
        if v < 1 {
            db(conn.execute_batch(
                "BEGIN;
                 CREATE TABLE cases (id TEXT PRIMARY KEY, title TEXT NOT NULL, created_at TEXT NOT NULL, claims_json TEXT NOT NULL);
                 CREATE TABLE submissions (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    case_id TEXT NOT NULL REFERENCES cases(id),
                    label TEXT NOT NULL, root TEXT NOT NULL, ingested_at TEXT NOT NULL,
                    assessment_json TEXT NOT NULL, reports_json TEXT NOT NULL, tool TEXT NOT NULL);
                 CREATE TABLE finding_states (
                    case_id TEXT NOT NULL REFERENCES cases(id), finding_id TEXT NOT NULL,
                    status TEXT NOT NULL, note TEXT, reason TEXT, updated_at TEXT NOT NULL,
                    PRIMARY KEY (case_id, finding_id));
                 CREATE TABLE notes (id INTEGER PRIMARY KEY AUTOINCREMENT, case_id TEXT NOT NULL REFERENCES cases(id), at TEXT NOT NULL, text TEXT NOT NULL);
                 CREATE TABLE cache (sha256 TEXT NOT NULL, config_key TEXT NOT NULL, report_json TEXT NOT NULL, PRIMARY KEY (sha256, config_key));
                 PRAGMA user_version = 1;
                 COMMIT;",
            ))?;
        }
        Ok(Store { conn })
    }

    // ---------- cases ----------

    pub fn create_case(&self, id: &str, title: &str, claims: &Claims) -> Result<()> {
        if id.is_empty()
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        {
            return Err(MetaError::InvalidArgument(
                "case id may contain letters, digits, '-', '_' and '.' only".into(),
            ));
        }
        let r = self.conn.execute(
            "INSERT INTO cases (id, title, created_at, claims_json) VALUES (?1, ?2, ?3, ?4)",
            params![id, title, now(), js(claims)?],
        );
        match r {
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Err(MetaError::InvalidArgument(format!(
                    "case '{id}' already exists"
                )))
            }
            other => db(other).map(|_| ()),
        }
    }

    pub fn get_case(&self, id: &str) -> Result<Option<CaseRow>> {
        db(self
            .conn
            .query_row(
                "SELECT id, title, created_at, claims_json, (SELECT COUNT(*) FROM submissions WHERE case_id = cases.id) FROM cases WHERE id = ?1",
                [id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, i64>(4)?)),
            )
            .optional())
        .map(|o| o.map(|(id, title, created_at, c, n)| CaseRow { id, title, created_at, claims: serde_json::from_str(&c).unwrap_or_default(), submissions: n }))
    }

    pub fn list_cases(&self, search: Option<&str>) -> Result<Vec<CaseRow>> {
        let like = format!("%{}%", search.unwrap_or(""));
        let mut st = db(self.conn.prepare(
            "SELECT id FROM cases WHERE id LIKE ?1 OR title LIKE ?1 ORDER BY created_at DESC, id",
        ))?;
        let ids: Vec<String> = db(st.query_map([like], |r| r.get(0)))?
            .filter_map(|x| x.ok())
            .collect();
        let mut out = vec![];
        for id in ids {
            if let Some(c) = self.get_case(&id)? {
                out.push(c);
            }
        }
        Ok(out)
    }

    pub fn update_claims(&self, id: &str, claims: &Claims) -> Result<()> {
        let n = db(self.conn.execute(
            "UPDATE cases SET claims_json = ?2 WHERE id = ?1",
            params![id, js(claims)?],
        ))?;
        if n == 0 {
            Err(MetaError::InvalidArgument(format!("no such case '{id}'")))
        } else {
            Ok(())
        }
    }

    // ---------- submissions ----------

    pub fn add_submission(
        &self,
        case: &str,
        label: &str,
        a: &Assessment,
        reports: &[FileReport],
    ) -> Result<i64> {
        if self.get_case(case)?.is_none() {
            return Err(MetaError::InvalidArgument(format!("no such case '{case}'")));
        }
        db(self.conn.execute(
            "INSERT INTO submissions (case_id, label, root, ingested_at, assessment_json, reports_json, tool) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![case, label, a.root, now(), js(a)?, js(&reports)?, a.tool],
        ))?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn submissions(&self, case: &str) -> Result<Vec<SubmissionRow>> {
        let mut st = db(self.conn.prepare("SELECT id, case_id, label, root, ingested_at, assessment_json FROM submissions WHERE case_id = ?1 ORDER BY id"))?;
        let rows = db(st.query_map([case], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        }))?;
        let mut out = vec![];
        for r in rows {
            let (id, case_id, label, root, ingested_at, aj) = db(r)?;
            let a: Option<Assessment> = serde_json::from_str(&aj).ok();
            out.push(SubmissionRow {
                id,
                case_id,
                label,
                root,
                ingested_at,
                files: a.as_ref().map(|a| a.inventory.len() as i64).unwrap_or(0),
                findings: a.as_ref().map(|a| a.findings.len() as i64).unwrap_or(0),
            });
        }
        Ok(out)
    }

    pub fn assessment(&self, submission: i64) -> Result<Assessment> {
        let s: String = db(self.conn.query_row(
            "SELECT assessment_json FROM submissions WHERE id = ?1",
            [submission],
            |r| r.get(0),
        ))?;
        serde_json::from_str(&s)
            .map_err(|e| MetaError::Other(format!("stored assessment unreadable: {e}")))
    }

    pub fn reports(&self, submission: i64) -> Result<Vec<FileReport>> {
        let s: String = db(self.conn.query_row(
            "SELECT reports_json FROM submissions WHERE id = ?1",
            [submission],
            |r| r.get(0),
        ))?;
        serde_json::from_str(&s)
            .map_err(|e| MetaError::Other(format!("stored metadata unreadable: {e}")))
    }

    pub fn latest_submission(&self, case: &str) -> Result<Option<i64>> {
        db(self.conn.query_row(
            "SELECT MAX(id) FROM submissions WHERE case_id = ?1",
            [case],
            |r| r.get::<_, Option<i64>>(0),
        ))
    }

    // ---------- reviewer decisions ----------

    pub fn set_finding_state(
        &self,
        case: &str,
        finding: &str,
        status: FindingStatus,
        note: Option<&str>,
        reason: Option<&str>,
    ) -> Result<()> {
        if status == FindingStatus::Dismissed && reason.map(str::trim).unwrap_or("").is_empty() {
            return Err(MetaError::InvalidArgument(
                "dismissing a finding requires a reason".into(),
            ));
        }
        if self.get_case(case)?.is_none() {
            return Err(MetaError::InvalidArgument(format!("no such case '{case}'")));
        }
        db(self.conn.execute(
            "INSERT INTO finding_states (case_id, finding_id, status, note, reason, updated_at) VALUES (?1,?2,?3,?4,?5,?6)
             ON CONFLICT(case_id, finding_id) DO UPDATE SET status=excluded.status, note=excluded.note, reason=excluded.reason, updated_at=excluded.updated_at",
            params![case, finding, status.as_str(), note, reason, now()],
        ))?;
        Ok(())
    }

    pub fn finding_states(&self, case: &str) -> Result<BTreeMap<String, FindingState>> {
        let mut st = db(self.conn.prepare("SELECT finding_id, status, note, reason, updated_at FROM finding_states WHERE case_id = ?1"))?;
        let rows = db(st.query_map([case], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
            ))
        }))?;
        let mut m = BTreeMap::new();
        for r in rows {
            let (id, status, note, reason, updated_at) = db(r)?;
            m.insert(id, FindingState { status: FindingStatus::parse(&status).unwrap_or(FindingStatus::Open), note, reason, updated_at });
        }
        Ok(m)
    }

    pub fn add_note(&self, case: &str, text: &str) -> Result<()> {
        if self.get_case(case)?.is_none() {
            return Err(MetaError::InvalidArgument(format!("no such case '{case}'")));
        }
        db(self.conn.execute("INSERT INTO notes (case_id, at, text) VALUES (?1,?2,?3)", params![case, now(), text]))?;
        Ok(())
    }

    pub fn notes(&self, case: &str) -> Result<Vec<NoteRow>> {
        let mut st = db(self.conn.prepare("SELECT at, text FROM notes WHERE case_id = ?1 ORDER BY id"))?;
        let rows = db(st.query_map([case], |r| Ok(NoteRow { at: r.get(0)?, text: r.get(1)? })))?;
        Ok(rows.filter_map(|x| x.ok()).collect())
    }

    // ---------- metadata cache (keyed by content hash + extraction configuration) ----------

    pub fn cache_get(&self, sha256: &str, config_key: &str) -> Option<FileReport> {
        let s: String = self.conn.query_row("SELECT report_json FROM cache WHERE sha256 = ?1 AND config_key = ?2", params![sha256, config_key], |r| r.get(0)).ok()?;
        serde_json::from_str(&s).ok()
    }

    pub fn cache_put(&self, sha256: &str, config_key: &str, r: &FileReport) -> Result<()> {
        db(self.conn.execute("INSERT OR REPLACE INTO cache (sha256, config_key, report_json) VALUES (?1,?2,?3)", params![sha256, config_key, js(r)?]))?;
        Ok(())
    }

    pub fn cache_clear(&self) -> Result<usize> {
        db(self.conn.execute("DELETE FROM cache", []))
    }

    pub fn schema_version(&self) -> i32 {
        self.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use metadata_forensics::assess::{assess, Input};
    use metadata_forensics::claims::Thresholds;
    use metadata_forensics::project::ProjectSummary;

    fn empty_assessment(case: &str) -> Assessment {
        let c = Claims::default();
        assess(&Input { case_id: case, root: Path::new("/x"), reports: &[], claims: &c, thresholds: &Thresholds::default(), git: None, previous: None, project: ProjectSummary::default(), now: metadata_core::TimeValue::naive(chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap().and_hms_opt(0, 0, 0).unwrap()) })
    }

    #[test]
    fn case_lifecycle_and_search() {
        let s = Store::open_memory().unwrap();
        s.create_case("s-001", "Alice final project", &Claims { start: Some("2024-01-10".into()), ..Default::default() }).unwrap();
        assert!(s.create_case("s-001", "dup", &Claims::default()).is_err());
        assert!(s.create_case("bad id!", "x", &Claims::default()).is_err());
        s.create_case("s-002", "Bob", &Claims::default()).unwrap();
        assert_eq!(s.list_cases(None).unwrap().len(), 2);
        assert_eq!(s.list_cases(Some("alice")).unwrap()[0].id, "s-001");
        assert_eq!(s.get_case("s-001").unwrap().unwrap().claims.start.as_deref(), Some("2024-01-10"));
        assert!(s.get_case("nope").unwrap().is_none());
    }

    #[test]
    fn submissions_roundtrip() {
        let s = Store::open_memory().unwrap();
        s.create_case("c", "t", &Claims::default()).unwrap();
        let a = empty_assessment("c");
        let id = s.add_submission("c", "milestone 1", &a, &[]).unwrap();
        assert_eq!(s.latest_submission("c").unwrap(), Some(id));
        assert_eq!(s.assessment(id).unwrap().case_id, "c");
        assert_eq!(s.submissions("c").unwrap()[0].label, "milestone 1");
        assert!(s.add_submission("ghost", "x", &a, &[]).is_err());
    }

    #[test]
    fn dismissal_requires_reason_and_states_persist() {
        let s = Store::open_memory().unwrap();
        s.create_case("c", "t", &Claims::default()).unwrap();
        assert!(s.set_finding_state("c", "TS-1", FindingStatus::Dismissed, None, None).is_err());
        assert!(s.set_finding_state("c", "TS-1", FindingStatus::Dismissed, None, Some("  ")).is_err());
        s.set_finding_state("c", "TS-1", FindingStatus::Dismissed, Some("checked"), Some("template file")).unwrap();
        s.set_finding_state("c", "TS-2", FindingStatus::Reviewed, Some("ok"), None).unwrap();
        s.set_finding_state("c", "TS-2", FindingStatus::Open, None, None).unwrap();
        let m = s.finding_states("c").unwrap();
        assert_eq!(m["TS-1"].status, FindingStatus::Dismissed);
        assert_eq!(m["TS-1"].reason.as_deref(), Some("template file"));
        assert_eq!(m["TS-2"].status, FindingStatus::Open);
        s.add_note("c", "spoke to student").unwrap();
        assert_eq!(s.notes("c").unwrap()[0].text, "spoke to student");
    }

    #[test]
    fn persists_on_disk_and_rejects_newer_schema() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("sub/cases.sqlite");
        {
            let s = Store::open(&p).unwrap();
            assert_eq!(s.schema_version(), SCHEMA_VERSION);
            s.create_case("c", "t", &Claims::default()).unwrap();
        }
        assert!(Store::open(&p).unwrap().get_case("c").unwrap().is_some());
        let c = Connection::open(&p).unwrap();
        c.execute_batch("PRAGMA user_version = 99;").unwrap();
        drop(c);
        assert!(Store::open(&p).is_err());
    }

    #[test]
    fn cache_is_keyed_by_hash_and_config() {
        let s = Store::open_memory().unwrap();
        let r = serde_json::from_str::<FileReport>(&js(&sample_report()).unwrap()).unwrap();
        s.cache_put("abc", "k1", &r).unwrap();
        assert!(s.cache_get("abc", "k1").is_some());
        assert!(s.cache_get("abc", "k2").is_none());
        assert!(s.cache_get("zzz", "k1").is_none());
        assert_eq!(s.cache_clear().unwrap(), 1);
    }

    fn sample_report() -> FileReport {
        use metadata_core::*;
        FileReport { schema_version: SCHEMA_VERSION.into(), path: "a".into(), size: 1, sha256: Some("abc".into()), format: FormatInfo { id: "text".into(), description: "".into(), mime: "".into(), category: Category::Text, extension_match: ExtensionMatch::Match, signature_hex: "".into() }, filesystem: FsInfo::default(), fields: vec![], warnings: vec![], backends: vec![], unavailable: vec![], error: None }
    }
}
