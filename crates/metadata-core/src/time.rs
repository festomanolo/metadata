//! Timezone-honest time model. A timestamp without an offset is never treated as UTC.
use chrono::{DateTime, FixedOffset, NaiveDateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeValue {
    /// ISO-8601 as stored: with offset if known, otherwise naive local-unknown.
    pub iso: String,
    /// True only when the source carried an explicit offset or Z.
    pub offset_known: bool,
    /// UTC equivalent; present only when `offset_known`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utc: Option<String>,
}

impl TimeValue {
    pub fn from_datetime(dt: DateTime<FixedOffset>) -> Self {
        TimeValue {
            iso: dt.to_rfc3339_opts(SecondsFormat::Secs, false),
            offset_known: true,
            utc: Some(dt.with_timezone(&Utc).to_rfc3339_opts(SecondsFormat::Secs, true)),
        }
    }
    pub fn naive(n: NaiveDateTime) -> Self {
        TimeValue {
            iso: n.format("%Y-%m-%dT%H:%M:%S").to_string(),
            offset_known: false,
            utc: None,
        }
    }
    pub fn from_system(t: std::time::SystemTime) -> Self {
        // Filesystem times are absolute instants.
        Self::from_datetime(DateTime::<Utc>::from(t).fixed_offset())
    }
    /// A comparable instant. Naive values are interpreted as UTC **only for ordering**; callers
    /// must check `offset_known` before presenting this as a true instant.
    pub fn sort_key(&self) -> Option<i64> {
        if let Some(u) = &self.utc {
            return DateTime::parse_from_rfc3339(u).ok().map(|d| d.timestamp());
        }
        NaiveDateTime::parse_from_str(&self.iso, "%Y-%m-%dT%H:%M:%S")
            .ok()
            .map(|n| n.and_utc().timestamp())
    }
}

fn parse_offset(s: &str) -> Option<FixedOffset> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("z") {
        return FixedOffset::east_opt(0);
    }
    let sign = match s.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let digits: String = s[1..].chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 2 {
        return None;
    }
    let h: i32 = digits[..2].parse().ok()?;
    let m: i32 = if digits.len() >= 4 { digits[2..4].parse().ok()? } else { 0 };
    FixedOffset::east_opt(sign * (h * 3600 + m * 60))
}

/// EXIF-style `YYYY:MM:DD HH:MM:SS` (also `-` separated), optional separate offset (`+02:00`).
pub fn parse_exif(s: &str, offset: Option<&str>) -> Option<TimeValue> {
    let s = s.trim().trim_matches('"');
    let fmts = ["%Y:%m:%d %H:%M:%S", "%Y-%m-%d %H:%M:%S", "%Y:%m:%d %H:%M:%S%.f"];
    let n = fmts.iter().find_map(|f| NaiveDateTime::parse_from_str(s, f).ok())?;
    match offset.and_then(parse_offset) {
        Some(o) => n.and_local_timezone(o).single().map(TimeValue::from_datetime),
        None => Some(TimeValue::naive(n)),
    }
}

/// PDF date `D:YYYYMMDDHHmmSS[Z|+HH'mm']`.
pub fn parse_pdf(s: &str) -> Option<TimeValue> {
    let s = s.trim().strip_prefix("D:").unwrap_or(s.trim());
    if s.len() < 4 || !s.as_bytes()[..4].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
    let rest = &s[digits.len()..];
    if !(rest.is_empty() || rest.starts_with(['Z', 'z', '+', '-'])) {
        return None;
    }
    let pad = |i: usize, d: &str| digits.get(i..i + 2).unwrap_or(d).to_string();
    let n = NaiveDateTime::parse_from_str(
        &format!(
            "{}{}{}{}{}{}",
            digits.get(0..4)?,
            pad(4, "01"),
            pad(6, "01"),
            pad(8, "00"),
            pad(10, "00"),
            pad(12, "00")
        ),
        "%Y%m%d%H%M%S",
    )
    .ok()?;
    match parse_offset(rest) {
        Some(o) => n.and_local_timezone(o).single().map(TimeValue::from_datetime),
        None => Some(TimeValue::naive(n)),
    }
}

/// RFC 3339 / ISO-8601 (with or without offset).
pub fn parse_iso(s: &str) -> Option<TimeValue> {
    let s = s.trim();
    if let Ok(d) = DateTime::parse_from_rfc3339(s) {
        return Some(TimeValue::from_datetime(d));
    }
    for f in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S"] {
        if let Ok(n) = NaiveDateTime::parse_from_str(s, f) {
            return Some(TimeValue::naive(n));
        }
    }
    // ISO with compact offset (+0200)
    DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%z").ok().map(TimeValue::from_datetime)
}

/// Best-effort parse of any known textual date.
pub fn parse_any(s: &str) -> Option<TimeValue> {
    parse_iso(s).or_else(|| parse_pdf(s)).or_else(|| parse_exif(s, None))
        .or_else(|| {
            // exiftool style "2023:05:01 12:00:00+02:00"
            let s = s.trim();
            if s.len() > 19 {
                parse_exif(&s[..19], Some(&s[19..]))
            } else {
                None
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exif_naive_is_not_utc() {
        let t = parse_exif("2023:05:01 12:00:00", None).unwrap();
        assert!(!t.offset_known);
        assert!(t.utc.is_none());
        assert_eq!(t.iso, "2023-05-01T12:00:00");
    }
    #[test]
    fn exif_with_offset() {
        let t = parse_exif("2023:05:01 12:00:00", Some("+02:00")).unwrap();
        assert!(t.offset_known);
        assert_eq!(t.iso, "2023-05-01T12:00:00+02:00");
        assert_eq!(t.utc.as_deref(), Some("2023-05-01T10:00:00Z"));
    }
    #[test]
    fn pdf_dates() {
        let t = parse_pdf("D:20230501120000+02'00'").unwrap();
        assert_eq!(t.utc.as_deref(), Some("2023-05-01T10:00:00Z"));
        let z = parse_pdf("D:20230501120000Z").unwrap();
        assert!(z.offset_known);
        let n = parse_pdf("D:2023").unwrap();
        assert_eq!(n.iso, "2023-01-01T00:00:00");
        assert!(!n.offset_known);
    }
    #[test]
    fn iso_variants() {
        assert!(parse_iso("2023-05-01T12:00:00Z").unwrap().offset_known);
        assert!(!parse_iso("2023-05-01T12:00:00").unwrap().offset_known);
        assert!(parse_any("2023:05:01 12:00:00+02:00").unwrap().offset_known);
        assert!(parse_any("garbage").is_none());
    }
    #[test]
    fn ambiguous_ordering_key_present() {
        assert!(parse_exif("2023:05:01 12:00:00", None).unwrap().sort_key().is_some());
    }
}
