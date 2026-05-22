use super::frontmatter::{extract_tags, find_closing_fm};

pub(crate) fn normalize_tag(tag: &str) -> String {
    tag.to_lowercase()
        .replace(' ', "-")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '/')
        .collect()
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

    let fm_content = &after[..end];
    let rest = &after[end + 4..];

    // Only add tags not already present (case-insensitive).
    let existing_tags = extract_tags(fm_content);
    let new_tags: Vec<&String> = tags
        .iter()
        .filter(|t| {
            !existing_tags
                .iter()
                .any(|e| e.to_lowercase() == t.to_lowercase())
        })
        .collect();
    if new_tags.is_empty() {
        return content.to_string();
    }

    // Frontmatter has no `tags:` key → append a fresh block to it.
    if !fm_content.contains("tags:") {
        let new_fm = format!(
            "{}\ntags:\n{}",
            fm_content.trim_end(),
            block_list(new_tags.iter().copied())
        );
        return format!("---{}\n{}---\n{}", "\n", new_fm, rest.trim_start());
    }

    // A `tags:` key exists — locate the line and insert into it.
    let mut lines: Vec<String> = fm_content.lines().map(String::from).collect();
    let Some(pos) = lines.iter().position(|l| l.trim().starts_with("tags:")) else {
        // "tags:" only appeared inside a value, not as a key → leave untouched.
        return content.to_string();
    };

    if lines[pos].trim() == "tags:" {
        // Block style: insert after the existing `- ` items.
        let mut insert_at = pos + 1;
        while insert_at < lines.len() && lines[insert_at].trim().starts_with("- ") {
            insert_at += 1;
        }
        for tag in new_tags.iter().rev() {
            lines.insert(insert_at, format!("  - {}", tag));
        }
    } else {
        // Inline style (`tags: [...]`): append block items below it.
        for tag in &new_tags {
            lines.push(format!("  - {}", tag));
        }
    }

    format!("---{}{}---\n{}", lines.join("\n"), "\n", rest.trim_start())
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
    let mut result = content.to_string();

    if result.starts_with("---")
        && let Some(end_pos) = find_closing_fm(&result[3..])
    {
        let fm_end = 3 + end_pos;
        let fm_content = result[3..fm_end].to_string();
        let rest = result[fm_end + 4..].to_string();

        let new_fm_lines: Vec<String> = fm_content
            .lines()
            .filter(|line| {
                let t = line.trim().trim_start_matches("- ").trim().to_lowercase();
                !tags_lower.contains(&t)
            })
            .map(String::from)
            .collect();

        result = format!("---{}{}---{}", "\n", new_fm_lines.join("\n"), rest);
    }

    for tag in &tags_lower {
        result = replace_inline_tag(&result, tag, "");
    }
    result
}

pub(crate) fn rename_tag_in_note(content: &str, old_tag: &str, new_tag: &str) -> String {
    let old_lower = old_tag.to_lowercase();
    let mut result = content.to_string();

    if result.starts_with("---")
        && let Some(end_pos) = find_closing_fm(&result[3..])
    {
        let fm_end = 3 + end_pos;
        let fm_content = result[3..fm_end].to_string();
        let rest = result[fm_end + 4..].to_string();

        let new_fm: String = fm_content
            .lines()
            .map(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with("- ") {
                    let tag_val = trimmed.trim_start_matches("- ").trim();
                    if tag_val.to_lowercase() == old_lower {
                        let indent: String =
                            line.chars().take_while(|c| c.is_whitespace()).collect();
                        return format!("{}- {}", indent, new_tag);
                    }
                }
                line.to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");

        result = format!("---{}{}---{}", "\n", new_fm, rest);
    }

    replace_inline_tag(&result, old_tag, &format!("#{}", new_tag))
}

/// `#foo` continues with these characters — used as a negative right-boundary
/// so we don't accidentally match `#foo` inside `#foobar` or `#foo-extra`.
fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_' || c == '/'
}

/// Replace each `#old_tag` occurrence in `content` with `replacement`, only
/// where the match is a complete tag — i.e. not followed by another
/// tag-continuation character.
fn replace_inline_tag(content: &str, old_tag: &str, replacement: &str) -> String {
    let needle = format!("#{}", old_tag);
    let needle_len = needle.len();
    let mut out = String::with_capacity(content.len());
    let mut cursor = 0;
    while let Some(rel) = content[cursor..].find(&needle) {
        let pos = cursor + rel;
        out.push_str(&content[cursor..pos]);
        let end = pos + needle_len;
        let standalone = match content[end..].chars().next() {
            None => true,
            Some(c) => !is_tag_char(c),
        };
        if standalone {
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

    #[test]
    fn normalize_tag_lowercases_and_hyphenates() {
        assert_eq!(normalize_tag("My Tag"), "my-tag");
        assert_eq!(normalize_tag("Hello World"), "hello-world");
        assert_eq!(normalize_tag("simple"), "simple");
    }
}
