use std::ops::Range;

use serde::Deserialize;
use serde_json::{Map, Value};

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
    // Same boundary rule the rewrites use, so search can't report a note that
    // `rename-tag` would then decline to touch. `nested` matches Obsidian: a
    // search for `parent` finds `#parent/child`.
    super::tags::contains_inline_tag(content, tag, true)
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

// ── Editing ──────────────────────────────────────────────────────────────────
//
// Every frontmatter rewrite goes through `edit_frontmatter`, which splits the
// note once, hands the frontmatter's lines to a closure, and reassembles. Doing
// the split/reassemble in exactly one place is what keeps the closing `---`
// marker correct — three hand-rolled reassemblies previously disagreed and two
// of them glued the marker onto the last frontmatter line.
//
// Edits are line surgery on the *one* key being changed, never a YAML
// round-trip. A round-trip would reformat the whole block: comments dropped, key
// order normalised, quoting churned. Users notice that immediately, and it makes
// every write a diff against the user's own formatting.

/// Split `content` into its frontmatter lines and body, apply `edit` to those
/// lines, and reassemble. Returns `None` when the note has no well-formed
/// frontmatter block, in which case the caller leaves the note alone.
pub(crate) fn edit_frontmatter<F>(content: &str, edit: F) -> Option<String>
where
    F: FnOnce(&mut Vec<String>),
{
    let after = content.strip_prefix("---")?;
    let end = find_closing_fm(after)?;
    // `find_closing_fm` returns the offset of the `\n` that precedes the closing
    // `---`, so the marker itself spans `end..end + 4`.
    let rest = &after[end + 4..];

    let mut lines: Vec<String> = after[..end].lines().map(String::from).collect();
    edit(&mut lines);

    Some(format!("---{}\n---{}", lines.join("\n"), rest))
}

/// Whether `content` opens a well-formed frontmatter block.
pub(crate) fn has_frontmatter(content: &str) -> bool {
    content
        .strip_prefix("---")
        .and_then(find_closing_fm)
        .is_some()
}

/// How a field's value is written.
pub(crate) enum Style {
    /// `key:` followed by `- item` lines (possibly none yet).
    Block,
    /// `key: [a, b]`
    Inline,
    /// `key: value`
    Scalar,
}

/// A top-level key located within a frontmatter body's lines.
pub(crate) struct Field {
    /// Index of the `key:` line.
    pub key: usize,
    /// Lines the key owns below it — block-list items, a nested mapping, or a
    /// block scalar's text. Empty when the value sits entirely on the key line.
    pub items: Range<usize>,
    pub style: Style,
}

/// The lines a key owns: everything indented beneath it, plus block-list items
/// written at column 0 (`- a` with no indent is legal YAML, and some editors
/// emit it).
fn owned_lines(lines: &[String], key: usize) -> Range<usize> {
    let mut end = key + 1;
    while let Some(line) = lines.get(end) {
        if !(line.starts_with([' ', '\t']) || line.starts_with("- ")) {
            break;
        }
        end += 1;
    }
    (key + 1)..end
}

/// Locate a top-level key. Top-level keys are unindented, so a leading space
/// means it belongs to some nested mapping and is not the field we manage.
pub(crate) fn find_field(lines: &[String], name: &str) -> Option<Field> {
    let prefix = format!("{}:", name);
    let key = lines.iter().position(|l| l.starts_with(&prefix))?;

    let value = lines[key][prefix.len()..].trim();
    let style = if value.is_empty() || value.starts_with('#') {
        Style::Block
    } else if value.starts_with('[') {
        Style::Inline
    } else {
        Style::Scalar
    };

    Some(Field {
        key,
        items: owned_lines(lines, key),
        style,
    })
}

/// Byte offsets of the `[` and `]` delimiting an inline sequence on `line`.
fn inline_bounds(line: &str) -> Option<(usize, usize)> {
    let open = line.find('[')?;
    let close = line[open..].find(']')? + open;
    Some((open, close))
}

/// Read an inline sequence's items. Tags never contain commas, so a plain split
/// is sufficient.
pub(crate) fn inline_items(line: &str) -> Vec<String> {
    let Some((open, close)) = inline_bounds(line) else {
        return Vec::new();
    };
    line[open + 1..close]
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Rewrite an inline sequence's items, preserving anything around it on the line
/// (indentation, a trailing comment).
pub(crate) fn render_inline(line: &str, items: &[String]) -> String {
    let Some((open, close)) = inline_bounds(line) else {
        return line.to_string();
    };
    format!(
        "{}[{}]{}",
        &line[..open],
        items.join(", "),
        &line[close + 1..]
    )
}

/// The value of a scalar `name: value` line, with any trailing comment dropped.
pub(crate) fn scalar_value(line: &str, name: &str) -> String {
    line[name.len() + 1..]
        .split(" #")
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Indentation to use for new block items — copied from the existing items so we
/// don't mix indentation styles within one list.
pub(crate) fn item_indent(lines: &[String], items: &Range<usize>) -> String {
    lines
        .get(items.start)
        .map(|l| l.chars().take_while(|c| c.is_whitespace()).collect())
        .unwrap_or_else(|| "  ".to_string())
}

// ── Generic field access ─────────────────────────────────────────────────────

/// The whole frontmatter as a JSON object. A note without a frontmatter block
/// has no fields, which is not an error; malformed YAML is.
pub(crate) fn parse_fields(content: &str) -> Result<Map<String, Value>, String> {
    let Some(body) = content.strip_prefix("---").and_then(|after| {
        let end = find_closing_fm(after)?;
        Some(&after[..end])
    }) else {
        return Ok(Map::new());
    };
    if body.trim().is_empty() {
        return Ok(Map::new());
    }

    let yaml: serde_yml::Value = serde_yml::from_str(body).map_err(|e| e.to_string())?;
    match serde_json::to_value(&yaml).map_err(|e| e.to_string())? {
        Value::Object(map) => Ok(map),
        _ => Err("frontmatter is not a YAML mapping".to_string()),
    }
}

/// Render `name: value` as frontmatter lines. serde does the emitting, so
/// quoting, escaping and block scalars are all its problem, not ours.
fn render_field(name: &str, value: &Value) -> Result<Vec<String>, String> {
    let mut map = serde_yml::Mapping::new();
    map.insert(
        serde_yml::Value::String(name.to_string()),
        serde_yml::to_value(value).map_err(|e| e.to_string())?,
    );
    let text = serde_yml::to_string(&serde_yml::Value::Mapping(map)).map_err(|e| e.to_string())?;

    let lines: Vec<String> = text
        .trim_end_matches('\n')
        .lines()
        // serde emits sequence items at column 0; Obsidian indents them.
        .map(|l| {
            if l.starts_with("- ") {
                format!("  {}", l)
            } else {
                l.to_string()
            }
        })
        .collect();

    // A key that YAML has to quote (`'a: b':`) would no longer be findable by
    // `find_field`, so a second `set` would append a duplicate. Refuse it rather
    // than corrupt the block.
    if !lines
        .first()
        .is_some_and(|l| l.starts_with(&format!("{}:", name)))
    {
        return Err(format!("'{}' is not a plain frontmatter key", name));
    }
    Ok(lines)
}

/// Set `name` to `value`, leaving every other line of the note untouched. Opens
/// a frontmatter block if the note has none.
pub(crate) fn set_field(content: &str, name: &str, value: &Value) -> Result<String, String> {
    let field = render_field(name, value)?;

    if !has_frontmatter(content) {
        return Ok(format!("---\n{}\n---\n{}", field.join("\n"), content));
    }

    Ok(
        edit_frontmatter(content, |lines| match find_field(lines, name) {
            Some(found) => {
                lines.splice(found.key..found.items.end, field);
            }
            None => lines.extend(field),
        })
        .expect("frontmatter block checked by has_frontmatter"),
    )
}

/// Drop `name` and the lines it owns. A note without that key is returned as-is.
pub(crate) fn remove_field(content: &str, name: &str) -> String {
    edit_frontmatter(content, |lines| {
        if let Some(found) = find_field(lines, name) {
            lines.drain(found.key..found.items.end);
        }
    })
    .unwrap_or_else(|| content.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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

    // ── Generic field access ─────────────────────────────────────────────────

    #[test]
    fn parse_fields_reads_every_key_not_just_tags() {
        let fields = parse_fields("---\ntitle: T\ncount: 3\ndone: true\n---\nbody").unwrap();
        assert_eq!(fields["title"], json!("T"));
        assert_eq!(fields["count"], json!(3));
        assert_eq!(fields["done"], json!(true));
    }

    #[test]
    fn parse_fields_without_frontmatter_is_empty_not_an_error() {
        assert!(parse_fields("just a body").unwrap().is_empty());
        assert!(parse_fields("---\n---\nbody").unwrap().is_empty());
    }

    #[test]
    fn parse_fields_rejects_malformed_yaml() {
        assert!(parse_fields("---\nkey: [unclosed\n---\nbody").is_err());
    }

    #[test]
    fn set_field_replaces_a_scalar_in_place() {
        let out = set_field(
            "---\ntitle: Old\ncount: 3\n---\nbody\n",
            "title",
            &json!("New"),
        )
        .unwrap();
        assert_eq!(out, "---\ntitle: New\ncount: 3\n---\nbody\n");
    }

    #[test]
    fn set_field_appends_a_missing_key() {
        let out = set_field("---\ntitle: T\n---\nbody\n", "status", &json!("draft")).unwrap();
        assert_eq!(out, "---\ntitle: T\nstatus: draft\n---\nbody\n");
    }

    #[test]
    fn set_field_writes_a_list_as_an_indented_block() {
        let out = set_field("---\ntitle: T\n---\nbody\n", "tags", &json!(["a", "b"])).unwrap();
        assert_eq!(out, "---\ntitle: T\ntags:\n  - a\n  - b\n---\nbody\n");
        assert_eq!(
            extract_tags("\ntitle: T\ntags:\n  - a\n  - b"),
            vec!["a", "b"]
        );
    }

    #[test]
    fn set_field_opens_a_frontmatter_block_when_there_is_none() {
        let out = set_field("just a body\n", "title", &json!("T")).unwrap();
        assert_eq!(out, "---\ntitle: T\n---\njust a body\n");
    }

    #[test]
    fn set_field_replaces_a_whole_block_value() {
        let content = "---\ntags:\n  - old1\n  - old2\ncount: 3\n---\nbody\n";
        let out = set_field(content, "tags", &json!(["new"])).unwrap();
        assert_eq!(out, "---\ntags:\n  - new\ncount: 3\n---\nbody\n");
    }

    #[test]
    fn set_field_quotes_values_that_yaml_would_misread() {
        // The string "true" must not come back as the boolean `true`, and a bare
        // `#` would open a comment. serde does the quoting; we just check that
        // what goes in is what comes out.
        let out = set_field("---\nx: 1\n---\nb\n", "answer", &json!("true")).unwrap();
        assert_eq!(out, "---\nx: 1\nanswer: 'true'\n---\nb\n");

        let out = set_field(&out, "topic", &json!("#rust")).unwrap();
        let fields = parse_fields(&out).unwrap();
        assert_eq!(fields["answer"], json!("true"), "string, not bool: {out:?}");
        assert_eq!(fields["topic"], json!("#rust"), "not a comment: {out:?}");
    }

    #[test]
    fn set_field_survives_a_multiline_string() {
        let out = set_field("---\nx: 1\n---\nb\n", "note", &json!("one\ntwo")).unwrap();
        assert_eq!(
            parse_fields(&out).unwrap()["note"],
            json!("one\ntwo"),
            "the note must still parse: {out:?}"
        );
    }

    #[test]
    fn set_field_leaves_comments_and_key_order_alone() {
        let content = "---\n# a comment\nzebra: 1\nnested:\n  deep: v\napple: 2\n---\nbody\n";
        let out = set_field(content, "apple", &json!(3)).unwrap();
        assert_eq!(
            out, "---\n# a comment\nzebra: 1\nnested:\n  deep: v\napple: 3\n---\nbody\n",
            "a YAML round-trip would have dropped the comment and re-sorted the keys"
        );
    }

    #[test]
    fn set_field_rejects_a_key_yaml_would_have_to_quote() {
        assert!(set_field("---\nx: 1\n---\nb\n", "a: b", &json!(1)).is_err());
    }

    #[test]
    fn remove_field_drops_the_key_and_the_lines_it_owns() {
        let content = "---\ntitle: T\ntags:\n  - a\n  - b\ncount: 3\n---\nbody\n";
        assert_eq!(
            remove_field(content, "tags"),
            "---\ntitle: T\ncount: 3\n---\nbody\n"
        );
    }

    #[test]
    fn remove_field_drops_a_nested_mapping_whole() {
        let content = "---\nnested:\n  deep: v\n  other: w\nkeep: 1\n---\nbody\n";
        assert_eq!(remove_field(content, "nested"), "---\nkeep: 1\n---\nbody\n");
    }

    #[test]
    fn remove_field_leaves_an_absent_key_alone() {
        let content = "---\ntitle: T\n---\nbody\n";
        assert_eq!(remove_field(content, "ghost"), content);
    }

    #[test]
    fn remove_the_only_field_leaves_an_empty_but_valid_block() {
        let out = remove_field("---\ntitle: T\n---\nbody\n", "title");
        assert_eq!(out, "---\n---\nbody\n");
        assert!(parse_fields(&out).unwrap().is_empty());
    }
}
