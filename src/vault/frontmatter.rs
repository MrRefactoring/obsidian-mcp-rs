#[derive(Debug, Clone)]
pub(crate) struct Frontmatter {
    pub tags: Vec<String>,
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
        tags: parse_yaml_tags(raw),
    })
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

pub(crate) fn parse_yaml_tags(yaml: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut in_tags = false;

    for line in yaml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("tags:") {
            let inline = trimmed.trim_start_matches("tags:").trim();
            if inline.starts_with('[') {
                let inner = inline.trim_matches(|c| c == '[' || c == ']');
                for t in inner.split(',') {
                    let t = t.trim().trim_matches('"').trim_matches('\'');
                    if !t.is_empty() {
                        tags.push(t.to_string());
                    }
                }
                in_tags = false;
            } else if inline.is_empty() {
                in_tags = true;
            } else {
                tags.push(inline.to_string());
                in_tags = false;
            }
        } else if in_tags && trimmed.starts_with("- ") {
            tags.push(trimmed.trim_start_matches("- ").trim().to_string());
        } else if in_tags && !trimmed.is_empty() && !trimmed.starts_with('-') {
            in_tags = false;
        }
    }
    tags
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
    fn extract_frontmatter_ignores_inline_dashes_in_body() {
        // A standalone closing marker comes only at the third `---` here; the
        // intervening `----` and inline forms must not terminate the block.
        let content = "---\ntags:\n  - real\n----\nstill yaml\ntags: [extra]\n---\nbody";
        let fm = extract_frontmatter(content).unwrap();
        // `extra` would only be parsed as a tag if we kept consuming past the
        // false-positive `----` line.
        assert!(
            fm.tags.iter().any(|t| t == "extra"),
            "tags parsed past false-positive marker, got {:?}",
            fm.tags
        );
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
