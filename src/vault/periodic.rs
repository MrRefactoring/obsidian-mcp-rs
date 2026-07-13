//! Daily notes, and their weekly/monthly/quarterly/yearly siblings.
//!
//! "Add this to today's note" is one of the things people most want an agent to
//! do with a vault, and it is exactly the thing an agent cannot do reliably by
//! guessing: the note's name and folder are whatever the user configured in
//! Obsidian, and getting either wrong creates a stray note instead of appending
//! to the real one.
//!
//! So we don't guess — we read Obsidian's own settings (`.obsidian/`) and land
//! in the same place Obsidian would. Only when there is no config do we fall back
//! to Obsidian's own defaults.

use std::path::Path;

use chrono::{Datelike, Days, Local, Months, NaiveDate};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::VaultError;

/// Which periodic note.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Period {
    Daily,
    Weekly,
    Monthly,
    Quarterly,
    Yearly,
}

/// What to do with it.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PeriodicAction {
    /// Read the note. Fails if it doesn't exist yet.
    Get,
    /// Read it, creating it first if it isn't there. Idempotent.
    Create,
    /// List the periodic notes that exist, most recent first.
    List,
}

/// Answer to a `periodic` call.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PeriodicOutput {
    /// Vault-relative path of the note. Absent for `list`.
    pub path: Option<String>,
    /// The note's text, capped like `read-note`'s. If it was cut, the last line
    /// says so and `path` is what you pass to `read-note` for the rest.
    /// Absent for `list`.
    pub content: Option<String>,
    /// Whether this call is what brought the note into existence.
    pub created: bool,
    /// Populated by `list` — vault-relative paths, most recent first.
    pub notes: Vec<String>,
}

impl Period {
    /// Obsidian's own default when nothing is configured.
    fn default_format(self) -> &'static str {
        match self {
            Period::Daily => "YYYY-MM-DD",
            Period::Weekly => "gggg-[W]ww",
            Period::Monthly => "YYYY-MM",
            Period::Quarterly => "YYYY-[Q]Q",
            Period::Yearly => "YYYY",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Period::Daily => "daily",
            Period::Weekly => "weekly",
            Period::Monthly => "monthly",
            Period::Quarterly => "quarterly",
            Period::Yearly => "yearly",
        }
    }

    /// Step `n` periods back from `date`. Used to walk backwards over the notes
    /// that could exist, so `list` never has to parse a filename.
    fn step_back(self, date: NaiveDate, n: u32) -> Option<NaiveDate> {
        match self {
            Period::Daily => date.checked_sub_days(Days::new(n as u64)),
            Period::Weekly => date.checked_sub_days(Days::new(n as u64 * 7)),
            Period::Monthly => date.checked_sub_months(Months::new(n)),
            Period::Quarterly => date.checked_sub_months(Months::new(n * 3)),
            Period::Yearly => date.checked_sub_months(Months::new(n * 12)),
        }
    }
}

/// Where a period's notes live and what they're called.
struct Settings {
    format: String,
    folder: String,
    template: Option<String>,
}

/// Obsidian's settings for this period.
///
/// The Periodic Notes community plugin owns every period when it's installed;
/// core Obsidian only has daily notes. We read the plugin first, then core, then
/// fall back to the documented defaults — the same precedence Obsidian applies.
fn settings(root: &Path, period: Period) -> Settings {
    let plugin = read_json(&root.join(".obsidian/plugins/periodic-notes/data.json"))
        .and_then(|cfg| cfg.get(period.key()).cloned())
        // `enabled: false` means the user turned this period off, so its settings
        // are not what Obsidian would use.
        .filter(|cfg| cfg.get("enabled").and_then(|e| e.as_bool()) != Some(false));

    let core = (period == Period::Daily)
        .then(|| read_json(&root.join(".obsidian/daily-notes.json")))
        .flatten();

    let field = |name: &str| -> Option<String> {
        let pick = |cfg: &Option<serde_json::Value>| {
            cfg.as_ref()
                .and_then(|c| c.get(name))
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .filter(|s| !s.is_empty())
        };
        pick(&plugin).or_else(|| pick(&core))
    };

    Settings {
        format: field("format").unwrap_or_else(|| period.default_format().to_string()),
        folder: field("folder").unwrap_or_default(),
        template: field("template"),
    }
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

// ── Moment.js date formats ───────────────────────────────────────────────────
//
// Obsidian names periodic notes with moment.js tokens, so `YYYY-MM-DD` has to
// mean what the user's vault already means by it. We support the tokens that
// actually appear in note formats — dates, not times — plus `[...]` literals
// (`gggg-[W]ww` → `2026-W29`).

/// Tokens, longest first: `YYYY` must win over `YY`.
const TOKENS: [&str; 16] = [
    "YYYY", "GGGG", "gggg", "MMMM", "dddd", "MMM", "ddd", "Do", "YY", "MM", "DD", "ww", "WW", "Q",
    "M", "D",
];

fn expand(token: &str, d: NaiveDate) -> String {
    let quarter = (d.month() - 1) / 3 + 1;
    match token {
        "YYYY" | "GGGG" => format!("{:04}", d.year()),
        // ISO week-year: the last days of December can belong to next year's W01.
        "gggg" => format!("{:04}", d.iso_week().year()),
        "YY" => format!("{:02}", d.year() % 100),
        "MMMM" => d.format("%B").to_string(),
        "MMM" => d.format("%b").to_string(),
        "MM" => format!("{:02}", d.month()),
        "M" => d.month().to_string(),
        "DD" => format!("{:02}", d.day()),
        "D" => d.day().to_string(),
        "Do" => ordinal(d.day()),
        "dddd" => d.format("%A").to_string(),
        "ddd" => d.format("%a").to_string(),
        "ww" | "WW" => format!("{:02}", d.iso_week().week()),
        "Q" => quarter.to_string(),
        _ => token.to_string(),
    }
}

fn ordinal(day: u32) -> String {
    let suffix = match (day % 10, day % 100) {
        (1, 1) | (1, 21) | (1, 31) => "st",
        (2, 2) | (2, 22) => "nd",
        (3, 3) | (3, 23) => "rd",
        _ => "th",
    };
    format!("{}{}", day, suffix)
}

/// Render a moment.js date format. Anything in `[...]` is a literal, and any
/// character that isn't a token passes through.
fn render(format: &str, date: NaiveDate) -> String {
    let mut out = String::new();
    let mut rest = format;

    while !rest.is_empty() {
        if let Some(literal) = rest.strip_prefix('[')
            && let Some(close) = literal.find(']')
        {
            out.push_str(&literal[..close]);
            rest = &literal[close + 1..];
            continue;
        }
        match TOKENS.iter().find(|t| rest.starts_with(**t)) {
            Some(token) => {
                out.push_str(&expand(token, date));
                rest = &rest[token.len()..];
            }
            None => {
                let c = rest.chars().next().expect("rest is not empty");
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    out
}

/// Obsidian's core template variables. Other template plugins have their own
/// syntax, which we deliberately leave untouched rather than half-expand.
fn fill_template(template: &str, date: NaiveDate, title: &str) -> String {
    template
        .replace("{{date}}", &date.format("%Y-%m-%d").to_string())
        .replace("{{title}}", title)
}

/// The note for `period` on `date`: its vault-relative path, and the text to
/// seed it with if it has to be created.
pub(crate) struct Resolved {
    pub folder: Option<String>,
    pub filename: String,
    pub seed: String,
}

pub(crate) fn resolve(
    root: &Path,
    period: Period,
    date: NaiveDate,
) -> Result<Resolved, VaultError> {
    let cfg = settings(root, period);
    let filename = render(&cfg.format, date);
    if filename.is_empty() {
        return Err(VaultError::InvalidPath(format!(
            "the configured {} note format ('{}') produced an empty name",
            period.key(),
            cfg.format
        )));
    }

    // A template is user config, not model input — but it is still a path, so it
    // goes through the sandbox like everything else.
    let seed = cfg
        .template
        .and_then(|t| {
            let path =
                super::path::safe_join(root, None, &super::path::ensure_md_extension(&t)).ok()?;
            std::fs::read_to_string(path).ok()
        })
        .map(|text| fill_template(&text, date, &filename))
        .unwrap_or_default();

    Ok(Resolved {
        folder: (!cfg.folder.is_empty()).then_some(cfg.folder),
        filename,
        seed,
    })
}

/// Today, in the machine's own timezone — a "daily note" means the user's today,
/// not UTC's.
pub(crate) fn today() -> NaiveDate {
    Local::now().date_naive()
}

/// Parse an explicit `YYYY-MM-DD`.
pub(crate) fn parse_date(date: &str) -> Result<NaiveDate, VaultError> {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|_| VaultError::InvalidPath(format!("'{}' is not a date — use YYYY-MM-DD", date)))
}

/// Paths of the periodic notes that exist, most recent first.
///
/// Walks back one period at a time and asks whether that note exists, rather than
/// trying to parse filenames back into dates — a renderer is easy, and a parser
/// for arbitrary moment formats is not.
pub(crate) fn list(
    root: &Path,
    period: Period,
    limit: usize,
    lookback: u32,
) -> Result<Vec<String>, VaultError> {
    let today = today();
    let mut found = Vec::new();

    for n in 0..lookback {
        let Some(date) = period.step_back(today, n) else {
            break;
        };
        let resolved = resolve(root, period, date)?;
        let path = super::path::safe_join(
            root,
            resolved.folder.as_deref(),
            &super::path::ensure_md_extension(&resolved.filename),
        )?;
        if path.exists() {
            found.push(super::rel_path(root, &path));
            if found.len() == limit {
                break;
            }
        }
    }
    Ok(found)
}

/// How far back `list` looks for each period. Enough to cover a year of dailies
/// or a decade of anything coarser.
pub(crate) fn lookback(period: Period) -> u32 {
    match period {
        Period::Daily => 365,
        Period::Weekly => 104,
        Period::Monthly => 60,
        Period::Quarterly => 40,
        Period::Yearly => 20,
    }
}

pub(crate) fn folder_of(resolved: &Resolved) -> Option<&str> {
    resolved.folder.as_deref()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn d(s: &str) -> NaiveDate {
        parse_date(s).unwrap()
    }

    // ── Format rendering ─────────────────────────────────────────────────────

    #[test]
    fn renders_obsidians_default_formats() {
        let date = d("2026-07-13"); // a Monday, ISO week 29, Q3
        assert_eq!(render("YYYY-MM-DD", date), "2026-07-13");
        assert_eq!(render("gggg-[W]ww", date), "2026-W29");
        assert_eq!(render("YYYY-MM", date), "2026-07");
        assert_eq!(render("YYYY-[Q]Q", date), "2026-Q3");
        assert_eq!(render("YYYY", date), "2026");
    }

    #[test]
    fn square_brackets_are_literal_text() {
        // Without this, the W in `[W]ww` would be… nothing at all, and the note
        // would be named `2026-29` — a different file from the user's.
        assert_eq!(render("[Week] ww", d("2026-07-13")), "Week 29");
    }

    #[test]
    fn longer_tokens_win() {
        // `YYYY` must not be read as two `YY`s.
        assert_eq!(render("YYYY", d("2026-01-05")), "2026");
        assert_eq!(render("YY", d("2026-01-05")), "26");
        assert_eq!(render("MMMM MMM MM M", d("2026-01-05")), "January Jan 01 1");
        assert_eq!(render("DD D Do", d("2026-01-05")), "05 5 5th");
    }

    #[test]
    fn iso_week_year_is_not_the_calendar_year() {
        // 2025-12-29 is a Monday in ISO week 1 *of 2026*. A weekly note named
        // with YYYY would collide with the wrong year's week 1.
        let date = d("2025-12-29");
        assert_eq!(render("gggg-[W]ww", date), "2026-W01");
        assert_eq!(render("YYYY", date), "2025");
    }

    #[test]
    fn unknown_characters_pass_through() {
        assert_eq!(
            render("Journal/YYYY_MM", d("2026-07-13")),
            "Journal/2026_07"
        );
    }

    // ── Reading Obsidian's own config ────────────────────────────────────────

    #[test]
    fn falls_back_to_obsidians_defaults_when_unconfigured() {
        let dir = TempDir::new().unwrap();
        let r = resolve(dir.path(), Period::Daily, d("2026-07-13")).unwrap();
        assert_eq!(r.filename, "2026-07-13");
        assert!(r.folder.is_none());
    }

    #[test]
    fn core_daily_notes_settings_are_honoured() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".obsidian")).unwrap();
        fs::write(
            dir.path().join(".obsidian/daily-notes.json"),
            r#"{"folder": "Journal", "format": "DD-MM-YYYY"}"#,
        )
        .unwrap();

        let r = resolve(dir.path(), Period::Daily, d("2026-07-13")).unwrap();
        assert_eq!(r.filename, "13-07-2026");
        assert_eq!(r.folder.as_deref(), Some("Journal"));
    }

    #[test]
    fn the_periodic_notes_plugin_wins_over_core() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".obsidian/plugins/periodic-notes")).unwrap();
        fs::write(
            dir.path().join(".obsidian/daily-notes.json"),
            r#"{"folder": "Core", "format": "YYYY-MM-DD"}"#,
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(".obsidian/plugins/periodic-notes/data.json"),
            r#"{"daily": {"enabled": true, "folder": "Plugin", "format": "YYYY.MM.DD"}}"#,
        )
        .unwrap();

        let r = resolve(dir.path(), Period::Daily, d("2026-07-13")).unwrap();
        assert_eq!(r.filename, "2026.07.13");
        assert_eq!(r.folder.as_deref(), Some("Plugin"));
    }

    #[test]
    fn a_disabled_period_falls_back_instead_of_using_its_settings() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".obsidian/plugins/periodic-notes")).unwrap();
        fs::write(
            dir.path()
                .join(".obsidian/plugins/periodic-notes/data.json"),
            r#"{"daily": {"enabled": false, "folder": "Off", "format": "[nope]"}}"#,
        )
        .unwrap();

        let r = resolve(dir.path(), Period::Daily, d("2026-07-13")).unwrap();
        assert_eq!(r.filename, "2026-07-13", "the disabled format must not win");
        assert!(r.folder.is_none());
    }

    #[test]
    fn a_configured_template_seeds_the_note() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".obsidian")).unwrap();
        fs::create_dir_all(dir.path().join("Templates")).unwrap();
        fs::write(
            dir.path().join("Templates/Daily.md"),
            "# {{title}}\n\nWritten on {{date}}\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".obsidian/daily-notes.json"),
            r#"{"template": "Templates/Daily"}"#,
        )
        .unwrap();

        let r = resolve(dir.path(), Period::Daily, d("2026-07-13")).unwrap();
        assert_eq!(r.seed, "# 2026-07-13\n\nWritten on 2026-07-13\n");
    }

    #[test]
    fn a_template_outside_the_vault_is_ignored_not_read() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".obsidian")).unwrap();
        fs::write(
            dir.path().join(".obsidian/daily-notes.json"),
            r#"{"template": "../../../etc/hosts"}"#,
        )
        .unwrap();

        let r = resolve(dir.path(), Period::Daily, d("2026-07-13")).unwrap();
        assert!(r.seed.is_empty(), "the sandbox must hold even for config");
    }

    // ── Stepping back ────────────────────────────────────────────────────────

    #[test]
    fn each_period_steps_back_by_its_own_unit() {
        let date = d("2026-07-13");
        assert_eq!(Period::Daily.step_back(date, 1).unwrap(), d("2026-07-12"));
        assert_eq!(Period::Weekly.step_back(date, 1).unwrap(), d("2026-07-06"));
        assert_eq!(Period::Monthly.step_back(date, 1).unwrap(), d("2026-06-13"));
        assert_eq!(
            Period::Quarterly.step_back(date, 1).unwrap(),
            d("2026-04-13")
        );
        assert_eq!(Period::Yearly.step_back(date, 1).unwrap(), d("2025-07-13"));
    }

    #[test]
    fn parse_date_rejects_nonsense() {
        assert!(parse_date("yesterday").is_err());
        assert!(parse_date("2026-13-01").is_err());
        assert!(parse_date("2026-07-13").is_ok());
    }
}
