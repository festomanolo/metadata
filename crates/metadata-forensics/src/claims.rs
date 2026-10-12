//! Declarations supplied by the assessor and thresholds for the rules. Everything here is *claimed*, not evidence.
use metadata_core::{time, MetaError, Result, TimeValue};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Claims {
    pub student: Option<String>,
    /// `YYYY-MM-DD` or RFC 3339.
    pub start: Option<String>,
    pub completion: Option<String>,
    pub deadline: Option<String>,
    pub milestones: Vec<Milestone>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Milestone {
    pub name: String,
    pub date: String,
    /// Only files whose relative path contains this text are checked against the milestone.
    pub path_contains: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Thresholds {
    /// Slack applied when either side of a comparison lacks a timezone (local time vs date-only claims).
    pub tolerance_hours: i64,
    /// Minimum number of distinct files sharing one timestamp before it is reported.
    pub uniform_min_files: usize,
    /// Gap in available history (days) worth noting.
    pub gap_days: i64,
    /// Max commits read from git.
    pub max_commits: usize,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            tolerance_hours: 24,
            uniform_min_files: 5,
            gap_days: 21,
            max_commits: 20_000,
        }
    }
}

/// A claimed date as an interval so date-only claims are not read as midnight instants.
#[derive(Debug, Clone, Copy)]
pub struct DayRange {
    pub start: i64,
    pub end: i64,
}

pub fn parse_claim(s: &str) -> Result<DayRange> {
    let s = s.trim();
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let st = d.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();
        return Ok(DayRange {
            start: st,
            end: st + 86_399,
        });
    }
    time::parse_iso(s)
        .and_then(|t| t.sort_key())
        .map(|k| DayRange { start: k, end: k })
        .ok_or_else(|| {
            MetaError::InvalidArgument(format!(
                "cannot parse date '{s}' (use YYYY-MM-DD or RFC 3339)"
            ))
        })
}

pub fn parse_claim_time(s: &str) -> Result<TimeValue> {
    let r = parse_claim(s)?;
    Ok(TimeValue::naive(
        chrono::DateTime::from_timestamp(r.start, 0)
            .unwrap()
            .naive_utc(),
    ))
}

impl Claims {
    pub fn validate(&self) -> Result<()> {
        for s in [&self.start, &self.completion, &self.deadline]
            .into_iter()
            .flatten()
        {
            parse_claim(s)?;
        }
        for m in &self.milestones {
            parse_claim(&m.date)?;
        }
        if let (Some(a), Some(b)) = (&self.start, &self.deadline) {
            if parse_claim(a)?.start > parse_claim(b)?.end {
                return Err(MetaError::InvalidArgument(
                    "claimed start is after the deadline".into(),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn date_only_is_a_whole_day() {
        let r = parse_claim("2024-03-01").unwrap();
        assert_eq!(r.end - r.start, 86_399);
        assert!(
            parse_claim("2024-03-01T10:00:00Z").unwrap().start
                == parse_claim("2024-03-01T10:00:00Z").unwrap().end
        );
        assert!(parse_claim("soon").is_err());
    }
    #[test]
    fn validate_catches_inverted_range() {
        let c = Claims {
            start: Some("2024-05-01".into()),
            deadline: Some("2024-01-01".into()),
            ..Default::default()
        };
        assert!(c.validate().is_err());
    }
}
