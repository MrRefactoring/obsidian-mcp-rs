use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};

const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/MrRefactoring/obsidian-mcp-rs/releases/latest";

pub const ENDPOINT_ENV: &str = "OBSIDIAN_MCP_UPDATE_ENDPOINT";

const MAX_REDIRECTS: usize = 5;
const MAX_DOWNLOAD_BYTES: u64 = 64 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct Release {
    pub tag: String,
    pub published_at: DateTime<Utc>,
    pub assets: Vec<Asset>,
}

impl Release {
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }

    pub fn parse(json: &str) -> Result<Self> {
        let value: serde_json::Value =
            serde_json::from_str(json).context("the release feed is not JSON")?;

        let tag = value
            .get("tag_name")
            .and_then(|v| v.as_str())
            .context("the release feed has no tag_name")?
            .to_string();

        let published = value
            .get("published_at")
            .and_then(|v| v.as_str())
            .context("the release feed has no published_at")?;
        let published_at = DateTime::parse_from_rfc3339(published)
            .with_context(|| format!("published_at is not a timestamp: {published}"))?
            .with_timezone(&Utc);

        let assets = value
            .get("assets")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        Some(Asset {
                            name: item.get("name")?.as_str()?.to_string(),
                            url: item.get("browser_download_url")?.as_str()?.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(Self {
            tag,
            published_at,
            assets,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Allow {
    GitHub,
    Only(String),
}

impl Allow {
    fn permits(&self, url: &str) -> bool {
        match self {
            Self::GitHub => {
                let Some(authority) = authority_of(url) else {
                    return false;
                };
                if !url.starts_with("https://") {
                    return false;
                }
                let host = authority.split(':').next().unwrap_or(authority);
                host == "github.com"
                    || host == "api.github.com"
                    || host.ends_with(".github.com")
                    || host.ends_with(".githubusercontent.com")
            }
            Self::Only(expected) => authority_of(url).is_some_and(|a| a == expected),
        }
    }
}

fn authority_of(url: &str) -> Option<&str> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?;
    if authority.is_empty() {
        None
    } else {
        Some(authority)
    }
}

pub struct Source {
    latest_url: String,
    allow: Allow,
    agent: ureq::Agent,
}

impl Source {
    pub fn github() -> Self {
        Self::new(LATEST_RELEASE_URL.to_string(), Allow::GitHub)
    }

    pub fn from_env() -> Self {
        match std::env::var(ENDPOINT_ENV) {
            Ok(base) if !base.trim().is_empty() => {
                let base = base.trim().trim_end_matches('/').to_string();
                let allow = authority_of(&base)
                    .map(|a| Allow::Only(a.to_string()))
                    .unwrap_or(Allow::GitHub);
                Self::new(format!("{base}/releases/latest"), allow)
            }
            _ => Self::github(),
        }
    }

    fn new(latest_url: String, allow: Allow) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .max_redirects(0)
            .max_redirects_will_error(false)
            .http_status_as_error(false)
            .user_agent(concat!("obsidian-mcp-rs/", env!("CARGO_PKG_VERSION")))
            .build()
            .new_agent();
        Self {
            latest_url,
            allow,
            agent,
        }
    }

    pub fn latest(&self) -> Result<Release> {
        let body = self.get(&self.latest_url)?;
        let text = String::from_utf8(body).context("the release feed is not UTF-8")?;
        Release::parse(&text)
    }

    pub fn download(&self, url: &str) -> Result<Vec<u8>> {
        self.get(url)
    }

    pub fn text(&self, url: &str) -> Result<String> {
        String::from_utf8(self.get(url)?).context("expected a text response")
    }

    fn get(&self, url: &str) -> Result<Vec<u8>> {
        let mut url = url.to_string();

        for _ in 0..=MAX_REDIRECTS {
            if !self.allow.permits(&url) {
                bail!("refusing to fetch {url}: not an address this updater trusts");
            }

            let mut response = self
                .agent
                .get(&url)
                .call()
                .with_context(|| format!("could not reach {url}"))?;

            let status = response.status().as_u16();
            if (300..400).contains(&status) {
                let location = response
                    .headers()
                    .get("location")
                    .and_then(|v| v.to_str().ok())
                    .with_context(|| format!("{url} redirected without saying where"))?;
                url = resolve(&url, location)
                    .with_context(|| format!("{url} redirected to an address we cannot read"))?;
                continue;
            }
            if status != 200 {
                bail!("{url} answered {status}");
            }

            return response
                .body_mut()
                .with_config()
                .limit(MAX_DOWNLOAD_BYTES)
                .read_to_vec()
                .with_context(|| format!("could not read the response from {url}"));
        }

        bail!("{url} redirected more than {MAX_REDIRECTS} times")
    }
}

fn resolve(base: &str, location: &str) -> Option<String> {
    if location.starts_with("https://") || location.starts_with("http://") {
        return Some(location.to_string());
    }
    let scheme_end = base.find("://")? + 3;
    let authority_len = base[scheme_end..]
        .find('/')
        .unwrap_or(base.len() - scheme_end);
    let root = &base[..scheme_end + authority_len];
    if location.starts_with('/') {
        Some(format!("{root}{location}"))
    } else {
        Some(format!("{root}/{location}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_github_allowlist_accepts_where_releases_actually_live() {
        let allow = Allow::GitHub;
        for url in [
            "https://api.github.com/repos/x/y/releases/latest",
            "https://github.com/x/y/releases/download/v1/asset",
            "https://objects.githubusercontent.com/blob/abc",
            "https://release-assets.githubusercontent.com/x",
        ] {
            assert!(allow.permits(url), "rejected {url}");
        }
    }

    #[test]
    fn the_github_allowlist_refuses_a_redirect_off_github() {
        let allow = Allow::GitHub;
        for url in [
            "https://evil.example.com/payload",
            "https://github.com.evil.example/payload",
            "https://githubusercontent.com.evil.test/x",
            "http://github.com/x/y",
            "file:///etc/passwd",
            "https://user@evil.test/x",
            "",
        ] {
            assert!(!allow.permits(url), "accepted {url}");
        }
    }

    #[test]
    fn a_test_endpoint_is_pinned_to_exactly_its_own_authority() {
        let allow = Allow::Only("127.0.0.1:8080".to_string());
        assert!(allow.permits("http://127.0.0.1:8080/releases/latest"));
        assert!(!allow.permits("http://127.0.0.1:9090/releases/latest"));
        assert!(!allow.permits("https://github.com/x"));
    }

    #[test]
    fn an_authority_is_read_without_its_userinfo_or_path() {
        assert_eq!(authority_of("https://a.test/x/y"), Some("a.test"));
        assert_eq!(authority_of("http://a.test:9/x"), Some("a.test:9"));
        assert_eq!(authority_of("https://u:p@a.test/x"), Some("a.test"));
        assert_eq!(authority_of("https://a.test?q=1"), Some("a.test"));
        assert_eq!(authority_of("ftp://a.test/x"), None);
        assert_eq!(authority_of("https:///x"), None);
    }

    #[test]
    fn a_relative_redirect_resolves_against_the_origin_not_the_path() {
        let base = "https://api.github.com/repos/x/y/releases/latest";
        assert_eq!(
            resolve(base, "/elsewhere").as_deref(),
            Some("https://api.github.com/elsewhere")
        );
        assert_eq!(
            resolve(base, "https://objects.githubusercontent.com/a").as_deref(),
            Some("https://objects.githubusercontent.com/a")
        );
    }

    #[test]
    fn a_release_feed_yields_its_tag_date_and_assets() {
        let json = r#"{
            "tag_name": "v0.8.0",
            "published_at": "2026-09-01T12:00:00Z",
            "assets": [
                {"name": "obsidian-mcp-rs-aarch64-apple-darwin",
                 "browser_download_url": "https://github.com/x/y/releases/download/v0.8.0/a"},
                {"name": "checksums.txt",
                 "browser_download_url": "https://github.com/x/y/releases/download/v0.8.0/c"}
            ]
        }"#;
        let release = Release::parse(json).unwrap();
        assert_eq!(release.tag, "v0.8.0");
        assert_eq!(
            release.published_at.to_rfc3339(),
            "2026-09-01T12:00:00+00:00"
        );
        assert_eq!(release.assets.len(), 2);
        assert_eq!(
            release.asset("checksums.txt").map(|a| a.url.as_str()),
            Some("https://github.com/x/y/releases/download/v0.8.0/c")
        );
        assert!(release.asset("nothing-like-this").is_none());
    }

    #[test]
    fn a_release_feed_missing_what_we_need_is_an_error_not_a_default() {
        assert!(Release::parse("not json").is_err());
        assert!(Release::parse(r#"{"published_at":"2026-09-01T12:00:00Z"}"#).is_err());
        assert!(Release::parse(r#"{"tag_name":"v1.0.0"}"#).is_err());
        assert!(Release::parse(r#"{"tag_name":"v1.0.0","published_at":"soon"}"#).is_err());
    }

    #[test]
    fn a_release_with_no_assets_parses_but_offers_nothing() {
        let release =
            Release::parse(r#"{"tag_name":"v1.0.0","published_at":"2026-09-01T12:00:00Z"}"#)
                .unwrap();
        assert!(release.assets.is_empty());
    }
}
