mod consent;
mod source;
mod state;
mod target;
mod version;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, TimeDelta, Utc};
use console::style;
use sha2::{Digest, Sha256};

use crate::install::binary;
use crate::vault::lock;

pub use consent::{
    forget as forget_consent, is_enabled as auto_update_enabled, set as set_consent,
};
pub use source::{Release, Source};
pub use target::asset_name;
pub use version::Version;

pub const SOAK: TimeDelta = match TimeDelta::try_hours(48) {
    Some(d) => d,
    None => panic!("48 hours is a valid duration"),
};

#[derive(Debug, clap::Args)]
pub struct UpdateArgs {
    /// Report what an update would do, without changing anything.
    #[arg(long, default_value_t = false)]
    pub check: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    UpToDate,
    Unreadable {
        tag: String,
    },
    Soaking {
        to: Version,
        ready_at: DateTime<Utc>,
    },
    Take {
        to: Version,
    },
}

pub fn decide(current: Version, release: &Release, now: DateTime<Utc>) -> Decision {
    let Some(to) = Version::parse(&release.tag) else {
        return Decision::Unreadable {
            tag: release.tag.clone(),
        };
    };
    if to == current {
        return Decision::UpToDate;
    }
    let ready_at = release.published_at + SOAK;
    if now < ready_at {
        return Decision::Soaking { to, ready_at };
    }
    Decision::Take { to }
}

pub fn last_seen_release() -> Option<Version> {
    state::read()?.latest
}

pub fn announce_installation() {
    match installed_copy() {
        Ok(path) => tracing::debug!(
            path = %path.display(),
            auto_update = consent::is_enabled(),
            "this server is the installed copy"
        ),
        Err(_) => tracing::info!(
            "this server was not placed by `install`, so it will not update itself — \
             run `obsidian-mcp-rs install` to manage it from here"
        ),
    }
}

pub fn watch_for_updates() {
    std::thread::spawn(|| match check_and_apply() {
        Ok(Some(to)) => tracing::info!(
            %to,
            "replaced the installed server; the next client launch will use it"
        ),
        Ok(None) => {}
        Err(e) => tracing::debug!(error = %e, "update check did not finish"),
    });
}

fn check_and_apply() -> Result<Option<Version>> {
    if !consent::is_enabled() {
        return Ok(None);
    }
    let Ok(dest) = installed_copy() else {
        return Ok(None);
    };
    if target::asset_name().is_none() {
        return Ok(None);
    }

    let now = Utc::now();
    if !state::due(state::read().as_ref(), now) {
        return Ok(None);
    }

    let source = Source::from_env();
    let fetched = source.latest();
    state::record(fetched.as_ref().ok().and_then(|r| Version::parse(&r.tag)));
    let release = fetched?;

    match decide(Version::current(), &release, now) {
        Decision::Take { to } => {
            apply(&source, &release, to, &dest)?;
            Ok(Some(to))
        }
        _ => Ok(None),
    }
}

pub fn run(args: UpdateArgs) -> Result<()> {
    let current = Version::current();
    let dest = match installed_copy() {
        Ok(path) => path,
        Err(reason) => {
            println!("  {} {reason}", style("!").yellow().bold());
            return Ok(());
        }
    };

    if target::asset_name().is_none() {
        println!(
            "  {} no release is published for this platform, so it cannot update itself",
            style("!").yellow().bold()
        );
        return Ok(());
    }

    let source = Source::from_env();
    let release = source.latest()?;

    match decide(current, &release, Utc::now()) {
        Decision::UpToDate => {
            println!(
                "  {} {} is the current release",
                style("✓").green().bold(),
                style(format!("v{current}")).dim()
            );
        }
        Decision::Unreadable { tag } => {
            println!(
                "  {} the latest release is tagged {tag}, which is not a version this can rank",
                style("!").yellow().bold()
            );
        }
        Decision::Soaking { to, ready_at } => {
            println!(
                "  {} v{to} is out; holding until {} so a bad release can be withdrawn first",
                style("~").cyan().bold(),
                style(ready_at.format("%Y-%m-%d %H:%M UTC")).dim()
            );
        }
        Decision::Take { to } if args.check => {
            println!(
                "  {} v{to} is available (running v{current})",
                style("→").cyan().bold()
            );
        }
        Decision::Take { to } => {
            apply(&source, &release, to, &dest)?;
            println!(
                "  {} updated v{current} → {}",
                style("✓").green().bold(),
                style(format!("v{to}")).cyan()
            );
            println!(
                "  {}",
                style("restart your AI client for it to take effect").dim()
            );
        }
    }

    Ok(())
}

fn installed_copy() -> std::result::Result<PathBuf, String> {
    let Some(dest) = binary::stable_path() else {
        return Err(
            "this system has no per-user data directory, so nothing was ever installed \
                    here to update"
                .to_string(),
        );
    };
    let running = std::env::current_exe()
        .map_err(|e| format!("could not work out which binary is running: {e}"))?;

    if !binary::is_same_file(&running, &dest) {
        return Err(format!(
            "this is not the installed copy ({}), so it is not ours to replace — \
             run `obsidian-mcp-rs install` to manage it from here",
            running.display()
        ));
    }
    Ok(dest)
}

fn apply(source: &Source, release: &Release, to: Version, dest: &Path) -> Result<()> {
    let _guard = lock_path().and_then(|p| lock::lock_exclusive(&p));

    if binary::installed_version()
        .as_deref()
        .and_then(Version::parse)
        == Some(to)
    {
        return Ok(());
    }

    let asset_name = target::asset_name().context("no release asset covers this platform")?;
    let asset = release
        .asset(&asset_name)
        .with_context(|| format!("release {} has no asset named {asset_name}", release.tag))?;
    let sums = release
        .asset("checksums.txt")
        .with_context(|| format!("release {} publishes no checksums.txt", release.tag))?;

    let want = checksum_for(&source.text(&sums.url)?, &asset_name)
        .with_context(|| format!("checksums.txt does not list {asset_name}"))?;

    let bytes = source.download(&asset.url)?;
    let got = sha256_hex(&bytes);
    if got != want {
        bail!("{asset_name} does not match its published checksum (got {got}, expected {want})");
    }

    let staged = download_path(dest);
    let _ = std::fs::remove_file(&staged);
    std::fs::write(&staged, &bytes)
        .with_context(|| format!("could not write {}", staged.display()))?;
    make_executable(&staged)?;

    if let Err(e) = smoke_test(&staged, to) {
        let _ = std::fs::remove_file(&staged);
        return Err(e);
    }

    let outcome = self_replace::self_replace(&staged)
        .with_context(|| format!("could not replace {}", dest.display()));
    let _ = std::fs::remove_file(&staged);
    outcome
}

fn smoke_test(path: &Path, expected: Version) -> Result<()> {
    let out = std::process::Command::new(path)
        .arg("--version")
        .output()
        .with_context(|| format!("the downloaded server would not start ({})", path.display()))?;
    if !out.status.success() {
        bail!("the downloaded server exited {} on --version", out.status);
    }
    let reported = String::from_utf8_lossy(&out.stdout);
    let parsed = reported
        .split_whitespace()
        .next_back()
        .and_then(Version::parse);
    if parsed != Some(expected) {
        bail!(
            "the downloaded server reports {:?}, not v{expected}",
            reported.trim()
        );
    }
    Ok(())
}

fn checksum_for(text: &str, asset: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let digest = parts.next()?;
        let name = parts.next()?.trim_start_matches('*');
        let listed = Path::new(name).file_name()?;
        (listed == asset).then(|| digest.to_ascii_lowercase())
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut acc, b| {
            use std::fmt::Write as _;
            let _ = write!(acc, "{b:02x}");
            acc
        })
}

fn download_path(dest: &Path) -> PathBuf {
    let mut name = dest.as_os_str().to_owned();
    name.push(".download");
    PathBuf::from(name)
}

fn lock_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("obsidian-mcp-rs").join("update.lock"))
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .with_context(|| format!("could not make {} executable", path.display()))
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, published: &str) -> Release {
        Release::parse(&format!(
            r#"{{"tag_name":"{tag}","published_at":"{published}","assets":[]}}"#
        ))
        .unwrap()
    }

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn the_same_version_is_not_an_update() {
        let r = release("v0.7.1", "2020-01-01T00:00:00Z");
        assert_eq!(
            decide(v("0.7.1"), &r, at("2026-09-01T00:00:00Z")),
            Decision::UpToDate
        );
    }

    #[test]
    fn a_release_younger_than_the_soak_window_is_held() {
        let r = release("v0.8.0", "2026-09-01T00:00:00Z");
        let Decision::Soaking { to, ready_at } = decide(v("0.7.1"), &r, at("2026-09-02T00:00:00Z"))
        else {
            panic!("a one-day-old release should still be soaking");
        };
        assert_eq!(to, v("0.8.0"));
        assert_eq!(ready_at, at("2026-09-03T00:00:00Z"));
    }

    #[test]
    fn a_release_past_the_soak_window_is_taken() {
        let r = release("v0.8.0", "2026-09-01T00:00:00Z");
        assert_eq!(
            decide(v("0.7.1"), &r, at("2026-09-03T00:00:01Z")),
            Decision::Take { to: v("0.8.0") }
        );
    }

    #[test]
    fn a_withdrawn_release_is_followed_back_down() {
        let r = release("v0.7.1", "2026-07-26T00:00:00Z");
        assert_eq!(
            decide(v("0.8.0"), &r, at("2026-09-01T00:00:00Z")),
            Decision::Take { to: v("0.7.1") }
        );
    }

    #[test]
    fn a_tag_we_cannot_rank_changes_nothing() {
        let r = release("nightly", "2020-01-01T00:00:00Z");
        assert_eq!(
            decide(v("0.7.1"), &r, at("2026-09-01T00:00:00Z")),
            Decision::Unreadable {
                tag: "nightly".to_string()
            }
        );
    }

    #[test]
    fn the_soak_window_is_the_two_days_we_agreed_on() {
        assert_eq!(SOAK.num_hours(), 48);
    }

    #[test]
    fn a_checksum_is_found_by_file_name_however_the_line_spells_the_path() {
        let text = "\
aaa  obsidian-mcp-rs-x86_64-unknown-linux-gnu
BBB  release-assets/obsidian-mcp-rs-aarch64-apple-darwin
ccc *obsidian-mcp-rs-x86_64-pc-windows-msvc.exe
ddd  checksums.txt
";
        assert_eq!(
            checksum_for(text, "obsidian-mcp-rs-x86_64-unknown-linux-gnu").as_deref(),
            Some("aaa")
        );
        assert_eq!(
            checksum_for(text, "obsidian-mcp-rs-aarch64-apple-darwin").as_deref(),
            Some("bbb")
        );
        assert_eq!(
            checksum_for(text, "obsidian-mcp-rs-x86_64-pc-windows-msvc.exe").as_deref(),
            Some("ccc")
        );
    }

    #[test]
    fn an_asset_the_checksums_do_not_list_has_no_checksum() {
        assert_eq!(
            checksum_for("aaa  something-else", "obsidian-mcp-rs-x"),
            None
        );
        assert_eq!(checksum_for("", "obsidian-mcp-rs-x"), None);
        assert_eq!(
            checksum_for("garbage-with-one-field", "obsidian-mcp-rs-x"),
            None
        );
    }

    #[test]
    fn a_near_miss_on_the_asset_name_is_not_a_match() {
        let text = "aaa  obsidian-mcp-rs-aarch64-apple-darwin\n";
        assert_eq!(
            checksum_for(text, "obsidian-mcp-rs-aarch64-apple-darwin.exe"),
            None
        );
        assert_eq!(checksum_for(text, "obsidian-mcp-rs-aarch64-apple"), None);
    }

    #[test]
    fn the_digest_matches_the_reference_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(sha256_hex(b"").len(), 64);
    }

    #[test]
    fn the_download_sits_beside_the_binary_it_will_become() {
        let dest = Path::new("/data/obsidian-mcp-rs/bin/obsidian-mcp-rs");
        let staged = download_path(dest);
        assert_eq!(staged.parent(), dest.parent());
        assert_ne!(staged, dest);
    }

    #[test]
    fn the_update_lock_is_not_the_one_that_serialises_vault_writes() {
        let (Some(update), Some(writes)) = (lock_path(), lock::default_lock_path()) else {
            return;
        };
        assert_ne!(
            update, writes,
            "an update would otherwise block every vault write for the length of a download"
        );
    }
}
