use serde::Deserialize;

#[derive(Debug, Clone)]
pub(crate) struct Frontmatter {
    pub tags: Vec<String>,
}

/// Frontmatter as far as we care about it — only `tags` is read. Accepts both
/// a single scalar (`tags: x`) and a sequence (`tags: [a, b]` / block list).
#[derive(Deserialize)]
struct RawFrontmatter {
    #[serde(default)]
    tags: Option<TagField>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum TagField {
    One(String),
    Many(Vec<String>),
}

impl TagField {
    fn into_vec(self) -> Vec<String> {
        match self {
            TagField::One(t) => vec![t],
            TagField::Many(t) => t,
        }
    }
}

pub(crate) fn content_has_tag(content: &str, tag: &str) -> bool {
    let tag_lower = tag.to_lowercase();
    if let Some(fm) = extract_frontmatter(content)
        && fm.tags.iter().any(|t| t.to_lowercase() == tag_lower)
    {
        return true;
    }
    let inline_pattern = format!("#{}", tag_lower);
    content.to_lowercase().contains(&inline_pattern)
}

pub(crate) fn extract_frontmatter(content: &str) -> Option<Frontmatter> {
    if !content.starts_with("---") {
        return None;
    }
    let after = &content[3..];
    let end = find_closing_fm(after)?;
    let raw = &after[..end];

    Some(Frontmatter {
        tags: extract_tags(raw),
    })
}

/// Parse the `tags` field out of a frontmatter YAML body. Malformed YAML yields
/// no tags (strict parsing — we no longer best-effort scrape line-by-line).
pub(crate) fn extract_tags(yaml: &str) -> Vec<String> {
    serde_yml::from_str::<RawFrontmatter>(yaml)
        .ok()
        .and_then(|fm| fm.tags)
        .map(TagField::into_vec)
        .unwrap_or_default()
}

/// Locate the closing `---` frontmatter marker — only matches when `---`
/// stands alone on a line (followed by `\n`, `\r`, or end-of-input). Returns
/// the offset of the leading `\n`, mirroring `str::find("\n---")` semantics.
pub(crate) fn find_closing_fm(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut start = 0;
    loop {
        let rel = s[start..].find("\n---")?;
        let pos = start + rel;
        let after = pos + 4;
        let standalone = after == s.len() || bytes[after] == b'\n' || bytes[after] == b'\r';
        if standalone {
            return Some(pos);
        }
        start = pos + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_frontmatter_returns_none_without_dashes() {
        assert!(extract_frontmatter("no frontmatter").is_none());
    }

    #[test]
    fn extract_frontmatter_parses_block_list() {
        let fm = extract_frontmatter("---\ntags:\n  - a\n  - b\n---\nbody").unwrap();
        assert_eq!(fm.tags, vec!["a", "b"]);
    }

    #[test]
    fn extract_frontmatter_parses_inline_list() {
        let fm = extract_frontmatter("---\ntags: [x, y]\n---\n").unwrap();
        assert_eq!(fm.tags, vec!["x", "y"]);
    }

    #[test]
    fn find_closing_fm_skips_false_marker_line() {
        // `----` is not a standalone `---` terminator, so the closing marker is
        // the later standalone `---`. The offset must point past the `----`.
        let after = "\ntags:\n  - real\n----\nstill text\n---\nbody";
        let end = find_closing_fm(after).expect("closing marker found");
        let raw = &after[..end];
        assert!(
            raw.contains("----"),
            "raw must include the false marker line"
        );
        assert!(raw.contains("real"));
    }

    #[test]
    fn extract_frontmatter_parses_multiline_block() {
        let fm = extract_frontmatter("---\ntitle: t\ntags:\n  - a\n  - b\n---\nbody").unwrap();
        assert_eq!(fm.tags, vec!["a", "b"]);
    }

    #[test]
    fn extract_frontmatter_open_without_close_returns_none() {
        let content = "---\ntitle: x\n----still no close\nbody";
        assert!(extract_frontmatter(content).is_none());
    }

    #[test]
    fn extract_frontmatter_parses_single_value() {
        let fm = extract_frontmatter("---\ntags: solo\n---\n").unwrap();
        assert_eq!(fm.tags, vec!["solo"]);
    }
}
