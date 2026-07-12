use super::frontmatter::{
    Style, edit_frontmatter, extract_tags, find_closing_fm, find_field, inline_items, item_indent,
    render_inline, scalar_value,
};

pub(crate) fn normalize_tag(tag: &str) -> String {
    tag.to_lowercase()
        .replace(' ', "-")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '/')
        .collect()
}

// ── Frontmatter editing ──────────────────────────────────────────────────────
//
// The mechanics — splitting the block, locating a key, rewriting one field —
// live in `frontmatter`, which is what a `tags:` edit and a generic
// `frontmatter set` have in common. What's left here is only what makes a tag a
// tag. Edits stay confined to the `tags:` field, so unrelated keys (and other
// block lists such as `aliases:`) survive byte-for-byte, comments and all.

/// Locate the `tags:` field within a frontmatter body's lines.
fn find_tags_field(lines: &[String]) -> Option<super::frontmatter::Field> {
    find_field(lines, "tags")
}

pub(crate) fn add_tags_to_frontmatter(content: &str, tags: &[String]) -> String {
    // No frontmatter at all → prepend a fresh tags block.
    let Some(after) = content.strip_prefix("---") else {
        return format!("---\ntags:\n{}\n---\n{}", block_list(tags.iter()), content);
    };
    // Opening `---` without a standalone closing marker → leave untouched.
    let Some(end) = find_closing_fm(after) else {
        return content.to_string();
    };

    // Only add tags not already present (case-insensitive).
    let existing = extract_tags(&after[..end]);
    let new_tags: Vec<String> = tags
        .iter()
        .filter(|t| {
            !existing
                .iter()
                .any(|e| e.to_lowercase() == t.to_lowercase())
        })
        .cloned()
        .collect();
    if new_tags.is_empty() {
        return content.to_string();
    }

    edit_frontmatter(content, |lines| {
        let Some(field) = find_tags_field(lines) else {
            // No `tags:` key yet → append a fresh block list to the frontmatter.
            lines.push("tags:".to_string());
            for tag in &new_tags {
                lines.push(format!("  - {}", tag));
            }
            return;
        };

        match field.style {
            Style::Block => {
                let indent = item_indent(lines, &field.items);
                for (i, tag) in new_tags.iter().enumerate() {
                    lines.insert(field.items.end + i, format!("{}- {}", indent, tag));
                }
            }
            Style::Inline => {
                let mut items = inline_items(&lines[field.key]);
                items.extend(new_tags.iter().cloned());
                lines[field.key] = render_inline(&lines[field.key], &items);
            }
            Style::Scalar => {
                // A scalar can't hold a second tag — promote it to a block list.
                let existing = scalar_value(&lines[field.key], "tags");
                lines[field.key] = "tags:".to_string();
                let items = std::iter::once(&existing)
                    .chain(new_tags.iter())
                    .map(|tag| format!("  - {}", tag));
                lines.splice(field.items.clone(), items);
            }
        }
    })
    .unwrap_or_else(|| content.to_string())
}

/// Render tags as indented YAML block-list items (`  - tag`), one per line.
fn block_list<'a>(tags: impl Iterator<Item = &'a String>) -> String {
    tags.map(|t| format!("  - {}", t))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn add_tags_to_content(content: &str, tags: &[String], position: &str) -> String {
    let tag_str: String = tags.iter().map(|t| format!("#{} ", t)).collect();
    let tag_str = tag_str.trim_end();

    if position == "start" {
        if let Some(stripped) = content.strip_prefix("---")
            && let Some(end) = find_closing_fm(stripped)
        {
            let fm_end = 3 + end + 4;
            let after_fm = &content[fm_end..].trim_start();
            return format!("{}\n{}\n{}", &content[..fm_end], tag_str, after_fm);
        }
        format!("{}\n{}", tag_str, content)
    } else {
        format!("{}\n{}", content.trim_end(), tag_str)
    }
}

pub(crate) fn remove_tags_from_note(content: &str, tags: &[String]) -> String {
    let tags_lower: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();
    let matches = |value: &str| tags_lower.contains(&value.trim().to_lowercase());

    let mut result = edit_frontmatter(content, |lines| {
        let Some(field) = find_tags_field(lines) else {
            return;
        };
        match field.style {
            Style::Block => {
                // Confined to this field's own items, so a matching entry in an
                // unrelated list (`aliases:`) is not collaterally deleted.
                for i in field.items.rev() {
                    let value = lines[i].trim().trim_start_matches("- ");
                    if matches(value) {
                        lines.remove(i);
                    }
                }
            }
            Style::Inline => {
                let items: Vec<String> = inline_items(&lines[field.key])
                    .into_iter()
                    .filter(|t| !matches(t))
                    .collect();
                lines[field.key] = render_inline(&lines[field.key], &items);
            }
            Style::Scalar => {
                if matches(&scalar_value(&lines[field.key], "tags")) {
                    lines[field.key] = "tags: []".to_string();
                }
            }
        }
    })
    .unwrap_or_else(|| content.to_string());

    for tag in &tags_lower {
        result = replace_inline_tag(&result, tag, "");
    }
    result
}

pub(crate) fn rename_tag_in_note(content: &str, old_tag: &str, new_tag: &str) -> String {
    let old_lower = old_tag.to_lowercase();
    let matches = |value: &str| value.trim().to_lowercase() == old_lower;

    let result = edit_frontmatter(content, |lines| {
        let Some(field) = find_tags_field(lines) else {
            return;
        };
        match field.style {
            Style::Block => {
                for i in field.items {
                    let value = lines[i].trim().trim_start_matches("- ");
                    if matches(value) {
                        let indent: String =
                            lines[i].chars().take_while(|c| c.is_whitespace()).collect();
                        lines[i] = format!("{}- {}", indent, new_tag);
                    }
                }
            }
            Style::Inline => {
                let items: Vec<String> = inline_items(&lines[field.key])
                    .into_iter()
                    .map(|t| if matches(&t) { new_tag.to_string() } else { t })
                    .collect();
                lines[field.key] = render_inline(&lines[field.key], &items);
            }
            Style::Scalar => {
                if matches(&scalar_value(&lines[field.key], "tags")) {
                    lines[field.key] = format!("tags: {}", new_tag);
                }
            }
        }
    })
    .unwrap_or_else(|| content.to_string());

    replace_inline_tag(&result, old_tag, &format!("#{}", new_tag))
}

// ── Inline (`#tag`) matching ─────────────────────────────────────────────────
//
// Search and rewrite must agree on where a tag ends, or `search-vault` reports a
// note that `rename-tag` then declines to change. Both go through the boundary
// checks below.

/// `#foo` continues with these characters — used as a negative right-boundary
/// so we don't accidentally match `#foo` inside `#foobar` or `#foo-extra`.
fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_' || c == '/'
}

/// A `#` only opens a tag when it isn't glued to a preceding word — this is what
/// keeps `C#foo` and the fragment in `](http://x#foo)` from reading as tags.
fn opens_tag(content: &str, hash: usize) -> bool {
    match content[..hash].chars().next_back() {
        None => true,
        Some(c) => !c.is_alphanumeric(),
    }
}

/// Whether the tag ends at `end`. With `nested`, a `/` counts as the end of the
/// parent tag, so `#parent` matches inside `#parent/child` — this is how Obsidian
/// treats nested tags when searching. Rewrites pass `nested = false`: renaming
/// `parent` must not silently rewrite `#parent/child`.
fn closes_tag(content: &str, end: usize, nested: bool) -> bool {
    match content[end..].chars().next() {
        None => true,
        Some('/') if nested => true,
        Some(c) => !is_tag_char(c),
    }
}

/// Every inline `#tag` in the note, in document order and with duplicates — the
/// caller counts.
///
/// Uses the same boundary rules as search and rewrite, and skips code (a `#tag`
/// in a shell snippet is a comment, not a tag). A heading is not a tag either:
/// `## Log` has no tag character after the hashes. Nor is `#2024` — Obsidian
/// requires a tag to contain at least one letter, or every issue number in the
/// vault would become one.
pub(crate) fn inline_tags(content: &str) -> Vec<String> {
    let code = super::links::code_spans(content);
    let mut found = Vec::new();
    let mut cursor = 0;

    while let Some(rel) = content[cursor..].find('#') {
        let hash = cursor + rel;
        cursor = hash + 1;

        if super::links::in_code(&code, hash) || !opens_tag(content, hash) {
            continue;
        }
        let rest = &content[hash + 1..];
        let len: usize = rest
            .chars()
            .take_while(|c| is_tag_char(*c))
            .map(char::len_utf8)
            .sum();
        if len == 0 {
            continue;
        }

        let tag = &rest[..len];
        if tag.chars().any(char::is_alphabetic) {
            found.push(tag.to_string());
        }
        cursor = hash + 1 + len;
    }
    found
}

/// Does `content` carry `#tag` as a complete inline tag?
pub(crate) fn contains_inline_tag(content: &str, tag: &str, nested: bool) -> bool {
    let haystack = content.to_lowercase();
    let needle = format!("#{}", tag.to_lowercase());
    let mut cursor = 0;
    while let Some(rel) = haystack[cursor..].find(&needle) {
        let pos = cursor + rel;
        let end = pos + needle.len();
        if opens_tag(&haystack, pos) && closes_tag(&haystack, end, nested) {
            return true;
        }
        cursor = end;
    }
    false
}

/// Replace each `#old_tag` occurrence in `content` with `replacement`, only
/// where the match is a complete tag.
fn replace_inline_tag(content: &str, old_tag: &str, replacement: &str) -> String {
    let needle = format!("#{}", old_tag);
    let needle_len = needle.len();
    let mut out = String::with_capacity(content.len());
    let mut cursor = 0;
    while let Some(rel) = content[cursor..].find(&needle) {
        let pos = cursor + rel;
        out.push_str(&content[cursor..pos]);
        let end = pos + needle_len;
        if opens_tag(content, pos) && closes_tag(content, end, false) {
            out.push_str(replacement);
        } else {
            out.push_str(&content[pos..end]);
        }
        cursor = end;
    }
    out.push_str(&content[cursor..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::frontmatter::extract_frontmatter;

    /// Tags as the note actually parses them — the check that matters, since a
    /// mangled frontmatter block silently reads back as "no tags at all".
    fn parsed_tags(content: &str) -> Vec<String> {
        extract_frontmatter(content)
            .map(|fm| fm.tags)
            .unwrap_or_default()
    }

    #[test]
    fn normalize_tag_lowercases_and_hyphenates() {
        assert_eq!(normalize_tag("My Tag"), "my-tag");
        assert_eq!(normalize_tag("Hello World"), "hello-world");
        assert_eq!(normalize_tag("simple"), "simple");
    }

    // ── The closing `---` marker must survive every rewrite ──────────────────

    #[test]
    fn remove_tags_keeps_frontmatter_block_intact() {
        let content = "---\ntitle: Keep Me\ntags:\n  - keep\n  - remove\n---\nbody\n";
        let out = remove_tags_from_note(content, &["remove".into()]);
        assert_eq!(
            out, "---\ntitle: Keep Me\ntags:\n  - keep\n---\nbody\n",
            "the closing marker must stay on its own line"
        );
        assert_eq!(parsed_tags(&out), vec!["keep"]);
    }

    #[test]
    fn rename_tag_keeps_frontmatter_block_intact() {
        let content = "---\ntitle: T\ntags:\n  - old\n---\nbody\n";
        let out = rename_tag_in_note(content, "old", "new");
        assert_eq!(out, "---\ntitle: T\ntags:\n  - new\n---\nbody\n");
        assert_eq!(parsed_tags(&out), vec!["new"]);
    }

    // ── Inline (`tags: [a, b]`) style ────────────────────────────────────────

    #[test]
    fn add_tag_to_inline_list_stays_inline_and_parses() {
        let content = "---\ntitle: x\ntags: [a, b]\n---\nbody\n";
        let out = add_tags_to_frontmatter(content, &["c".into()]);
        assert_eq!(out, "---\ntitle: x\ntags: [a, b, c]\n---\nbody\n");
        assert_eq!(parsed_tags(&out), vec!["a", "b", "c"]);
    }

    #[test]
    fn remove_tag_from_inline_list() {
        let content = "---\ntags: [gone, keep]\n---\nbody\n";
        let out = remove_tags_from_note(content, &["gone".into()]);
        assert_eq!(out, "---\ntags: [keep]\n---\nbody\n");
        assert_eq!(parsed_tags(&out), vec!["keep"]);
    }

    #[test]
    fn rename_tag_in_inline_list() {
        let content = "---\ntags: [old, keep]\n---\nbody\n";
        let out = rename_tag_in_note(content, "old", "new");
        assert_eq!(out, "---\ntags: [new, keep]\n---\nbody\n");
        assert_eq!(parsed_tags(&out), vec!["new", "keep"]);
    }

    #[test]
    fn removing_last_inline_tag_leaves_an_empty_list() {
        let content = "---\ntags: [only]\n---\nbody\n";
        let out = remove_tags_from_note(content, &["only".into()]);
        assert_eq!(out, "---\ntags: []\n---\nbody\n");
        assert!(parsed_tags(&out).is_empty());
    }

    // ── Scalar (`tags: solo`) style ──────────────────────────────────────────

    #[test]
    fn add_tag_to_scalar_promotes_it_to_a_block_list() {
        let content = "---\ntags: solo\n---\nbody\n";
        let out = add_tags_to_frontmatter(content, &["new".into()]);
        assert_eq!(out, "---\ntags:\n  - solo\n  - new\n---\nbody\n");
        assert_eq!(parsed_tags(&out), vec!["solo", "new"]);
    }

    #[test]
    fn rename_scalar_tag() {
        let content = "---\ntags: old\n---\nbody\n";
        let out = rename_tag_in_note(content, "old", "new");
        assert_eq!(out, "---\ntags: new\n---\nbody\n");
        assert_eq!(parsed_tags(&out), vec!["new"]);
    }

    #[test]
    fn remove_scalar_tag() {
        let content = "---\ntags: gone\n---\nbody\n";
        let out = remove_tags_from_note(content, &["gone".into()]);
        assert_eq!(out, "---\ntags: []\n---\nbody\n");
        assert!(parsed_tags(&out).is_empty());
    }

    // ── Edits stay inside the `tags:` field ──────────────────────────────────

    #[test]
    fn remove_tags_does_not_touch_a_matching_alias() {
        let content = "---\naliases:\n  - target\ntags:\n  - target\n---\nbody\n";
        let out = remove_tags_from_note(content, &["target".into()]);
        assert_eq!(
            out, "---\naliases:\n  - target\ntags:\n---\nbody\n",
            "only the tags entry may be removed, never the alias"
        );
    }

    #[test]
    fn rename_tag_does_not_touch_a_matching_alias() {
        let content = "---\naliases:\n  - old\ntags:\n  - old\n---\nbody\n";
        let out = rename_tag_in_note(content, "old", "new");
        assert_eq!(out, "---\naliases:\n  - old\ntags:\n  - new\n---\nbody\n");
    }

    #[test]
    fn unrelated_frontmatter_survives_byte_for_byte() {
        let content =
            "---\ndate: 2026-07-12\nnested:\n  key: value\ntags:\n  - a\ncount: 3\n---\nbody\n";
        let out = add_tags_to_frontmatter(content, &["b".into()]);
        assert_eq!(
            out,
            "---\ndate: 2026-07-12\nnested:\n  key: value\ntags:\n  - a\n  - b\ncount: 3\n---\nbody\n"
        );
    }

    // ── Inline-tag boundaries: search and rewrite agree ──────────────────────

    #[test]
    fn inline_tag_match_requires_a_right_boundary() {
        assert!(contains_inline_tag("see #foo here", "foo", false));
        assert!(
            !contains_inline_tag("see #foobar here", "foo", false),
            "#foo must not match inside #foobar"
        );
    }

    #[test]
    fn inline_tag_match_requires_a_left_boundary() {
        assert!(
            !contains_inline_tag("written in C#foo", "foo", false),
            "a # glued to a word does not open a tag"
        );
    }

    #[test]
    fn nested_tags_match_the_parent_when_searching_only() {
        assert!(
            contains_inline_tag("#parent/child", "parent", true),
            "search matches nested tags, as Obsidian does"
        );
        assert!(
            !contains_inline_tag("#parent/child", "parent", false),
            "rewrites must not reach into nested tags"
        );
    }

    #[test]
    fn rename_leaves_nested_tags_alone() {
        let out = rename_tag_in_note("body #parent/child and #parent\n", "parent", "renamed");
        assert_eq!(out, "body #parent/child and #renamed\n");
    }
}
