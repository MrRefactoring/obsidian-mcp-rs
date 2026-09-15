use std::path::PathBuf;

use chrono::{DateTime, TimeDelta, Utc};

use super::Version;

pub const INTERVAL: TimeDelta = match TimeDelta::try_hours(24) {
    Some(d) => d,
    None => panic!("24 hours is a valid duration"),
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub checked_at: DateTime<Utc>,
    pub latest: Option<Version>,
}

pub fn path() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("obsidian-mcp-rs").join("update-check.json"))
}

pub fn due(last: Option<&Check>, now: DateTime<Utc>) -> bool {
    last.is_none_or(|c| now.signed_duration_since(c.checked_at) >= INTERVAL)
}

pub fn read() -> Option<Check> {
    parse(&std::fs::read_to_string(path()?).ok()?)
}

pub fn parse(text: &str) -> Option<Check> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let checked_at = DateTime::parse_from_rfc3339(value.get("checked_at")?.as_str()?)
        .ok()?
        .with_timezone(&Utc);
    let latest = value
        .get("latest")
        .and_then(|v| v.as_str())
        .and_then(Version::parse);
    Some(Check { checked_at, latest })
}

pub fn record(latest: Option<Version>) {
    if let Some(path) = path() {
        record_at(&path, latest, Utc::now());
    }
}

fn record_at(path: &std::path::Path, latest: Option<Version>, now: DateTime<Utc>) {
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }
    let body = match latest {
        Some(v) => format!(r#"{{"checked_at":"{}","latest":"{v}"}}"#, now.to_rfc3339()),
        None => format!(r#"{{"checked_at":"{}"}}"#, now.to_rfc3339()),
    };
    let _ = std::fs::write(path, body);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn a_first_run_has_nothing_to_go_on_and_checks() {
        assert!(due(None, at("2026-09-01T00:00:00Z")));
    }

    #[test]
    fn duplicate_servers_launched_together_do_not_each_hit_the_network() {
        let just_now = Check {
            checked_at: at("2026-09-01T00:00:00Z"),
            latest: None,
        };
        assert!(!due(Some(&just_now), at("2026-09-01T00:00:01Z")));
        assert!(!due(Some(&just_now), at("2026-09-01T23:59:59Z")));
        assert!(due(Some(&just_now), at("2026-09-02T00:00:00Z")));
    }

    #[test]
    fn a_check_round_trips_through_the_file_format() {
        let text = r#"{"checked_at":"2026-09-01T00:00:00+00:00","latest":"0.8.0"}"#;
        let check = parse(text).unwrap();
        assert_eq!(check.checked_at, at("2026-09-01T00:00:00Z"));
        assert_eq!(check.latest, Version::parse("0.8.0"));
    }

    #[test]
    fn a_check_without_a_version_still_records_that_we_looked() {
        let check = parse(r#"{"checked_at":"2026-09-01T00:00:00+00:00"}"#).unwrap();
        assert_eq!(check.latest, None);
    }

    #[test]
    fn an_unreadable_state_file_is_treated_as_never_checked() {
        for bad in ["", "not json", "{}", r#"{"checked_at":"whenever"}"#, "[]"] {
            assert!(parse(bad).is_none(), "accepted {bad:?}");
        }
    }

    #[test]
    fn what_we_write_is_what_we_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("nested").join("update-check.json");
        let now = at("2026-09-01T12:34:56Z");

        record_at(&file, Version::parse("0.8.0"), now);
        let back = parse(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(back.checked_at, now);
        assert_eq!(back.latest, Version::parse("0.8.0"));

        record_at(&file, None, now);
        let back = parse(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(back.checked_at, now);
        assert_eq!(back.latest, None);
    }

    #[test]
    fn a_failed_check_still_stamps_the_time_so_it_backs_off() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("update-check.json");
        let now = at("2026-09-01T00:00:00Z");

        record_at(&file, None, now);
        let back = parse(&std::fs::read_to_string(&file).unwrap()).unwrap();

        assert!(!due(Some(&back), at("2026-09-01T06:00:00Z")));
    }

    #[test]
    fn the_state_file_lives_in_the_cache_directory_not_in_a_vault() {
        let Some(path) = path() else {
            return;
        };
        assert!(dirs::cache_dir().is_some_and(|c| path.starts_with(c)));
    }
}
