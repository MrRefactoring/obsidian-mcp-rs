use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
}

impl Version {
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = text.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
        })
    }

    pub fn current() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION")).expect("this package's own version is not semver")
    }

    fn key(&self) -> (u64, u64, u64) {
        (self.major, self.minor, self.patch)
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key().cmp(&other.key())
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn a_leading_v_is_optional() {
        assert_eq!(v("v1.2.3"), v("1.2.3"));
    }

    #[test]
    fn versions_order_by_component_not_by_string() {
        assert!(v("0.10.0") > v("0.9.9"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert!(v("0.7.2") > v("0.7.1"));
        assert_eq!(v("0.7.1").cmp(&v("0.7.1")), Ordering::Equal);
    }

    #[test]
    fn a_prerelease_tag_is_not_a_version_we_will_install() {
        assert!(Version::parse("1.0.0-rc1").is_none());
        assert!(Version::parse("v2.0.0-beta.1").is_none());
    }

    #[test]
    fn anything_that_is_not_three_numbers_is_rejected() {
        for bad in ["", "v", "1", "1.2", "1.2.3.4", "a.b.c", "1.2.x", "latest"] {
            assert!(Version::parse(bad).is_none(), "accepted {bad:?}");
        }
    }

    #[test]
    fn our_own_version_parses() {
        assert_eq!(Version::current().to_string(), env!("CARGO_PKG_VERSION"));
    }
}
