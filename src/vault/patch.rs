//! Editing *part* of a note: a heading's section, or a block reference.
//!
//! Without this, the only way to change one section is to read the whole note,
//! splice it in the model's head, and write the whole note back — which risks
//! losing everything the model didn't happen to reproduce. Patching writes only
//! the bytes the target covers.
//!
//! The scanners here are also what `read-note view=outline` reports, so what the
//! model is offered as a target is exactly what the patcher can find.

use std::ops::Range;

use schemars::JsonSchema;
use serde::Deserialize;

use super::frontmatter::find_closing_fm;
use super::links::{code_spans, in_code};

/// What part of a note an edit is aimed at.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TargetKind {
    /// A markdown heading — the edit applies to its section: the heading line
    /// and everything under it, up to the next heading of the same or a higher
    /// level.
    Heading,
    /// An Obsidian block reference (`^block-id`) — the edit applies to the block
    /// carrying that marker.
    Block,
}

/// Where in a note an edit lands.
pub(crate) struct Region {
    /// The whole target: a heading line plus its section, or the block's lines.
    pub span: Range<usize>,
    /// Where the target's *body* starts — after the heading line for a heading,
    /// and at the start of the block otherwise. `replace` rewrites `body..end`,
    /// which is what lets a section be replaced without losing its heading.
    pub body: usize,
}

/// A heading found in a note's body.
struct Heading {
    level: usize,
    text: String,
    line: usize,
    /// Byte offset of the start of the heading line.
    start: usize,
}

/// A block-reference marker (`^id`) and the block it terminates.
struct Block {
    id: String,
    line: usize,
    span: Range<usize>,
}

/// Where the note's body begins — past the frontmatter, so a `#` inside the YAML
/// is not mistaken for a heading.
fn body_start(content: &str) -> usize {
    match content.strip_prefix("---").and_then(find_closing_fm) {
        // `find_closing_fm` returns the offset (within the post-`---` slice) of
        // the newline before the closing marker, which spans 4 more bytes.
        Some(end) => 3 + end + 4,
        None => 0,
    }
}

/// Lines of `content` as `(byte offset, text including its newline)`.
fn lines_with_offsets(content: &str) -> Vec<(usize, &str)> {
    let mut offset = 0;
    content
        .split_inclusive('\n')
        .map(|line| {
            let start = offset;
            offset += line.len();
            (start, line)
        })
        .collect()
}

/// Every ATX heading in the note's body, in document order.
///
/// Not headings: anything inside a code fence (that's a code sample), anything
/// in the frontmatter, and `#tag` — a hash glued to a word is a tag, and a note
/// that opens with `#todo` must not read as an H1 called "todo".
fn headings(content: &str) -> Vec<Heading> {
    let code = code_spans(content);
    let start_of_body = body_start(content);

    lines_with_offsets(content)
        .into_iter()
        .enumerate()
        .filter(|(_, (start, _))| *start >= start_of_body && !in_code(&code, *start))
        .filter_map(|(i, (start, line))| {
            let text = line.trim_start();
            let level = text.chars().take_while(|c| *c == '#').count();
            if level == 0 || level > 6 {
                return None;
            }
            let rest = &text[level..];
            // `## Heading` — the space is what separates a heading from a #tag.
            if !rest.starts_with([' ', '\t']) {
                return None;
            }
            Some(Heading {
                level,
                // Closing hashes (`## Heading ##`) are decoration, not content.
                text: rest.trim().trim_end_matches('#').trim().to_string(),
                line: i + 1,
                start,
            })
        })
        .collect()
}

/// The trailing `^block-id` on a line, if it carries one.
fn block_id(line: &str) -> Option<&str> {
    let text = line.trim_end();
    let (before, id) = text.rsplit_once('^')?;
    // A caret glued to a word (`2^n`) is not a block marker.
    if !before.is_empty() && !before.ends_with([' ', '\t']) {
        return None;
    }
    let valid = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_');
    valid.then_some(id)
}

/// Every block reference in the note's body. A block is the run of non-blank
/// lines ending at the marker — Obsidian puts `^id` at the end of the block it
/// names.
fn blocks(content: &str) -> Vec<Block> {
    let code = code_spans(content);
    let start_of_body = body_start(content);
    let lines = lines_with_offsets(content);

    lines
        .iter()
        .enumerate()
        .filter(|(_, (start, _))| *start >= start_of_body && !in_code(&code, *start))
        .filter_map(|(i, (start, line))| {
            let id = block_id(line)?;
            let mut first = i;
            while first > 0 && !lines[first - 1].1.trim().is_empty() {
                first -= 1;
            }
            Some(Block {
                id: id.to_string(),
                line: i + 1,
                span: lines[first].0..start + line.len(),
            })
        })
        .collect()
}

/// Resolve a target to the bytes it covers. Heading targets are matched with or
/// without their `#` prefix (`"Log"` and `"## Log"` both work) and
/// case-insensitively; when a note repeats a heading, the first one wins.
pub(crate) fn find_region(content: &str, kind: &TargetKind, target: &str) -> Option<Region> {
    match kind {
        TargetKind::Heading => {
            let found = headings(content);
            let wanted = target.trim();
            let level = wanted.chars().take_while(|c| *c == '#').count();
            let text = wanted[level..].trim();

            let i = found.iter().position(|h| {
                h.text.eq_ignore_ascii_case(text) && (level == 0 || h.level == level)
            })?;

            // The section runs until the next heading that is not nested inside
            // it — a deeper heading is part of this section.
            let end = found[i + 1..]
                .iter()
                .find(|next| next.level <= found[i].level)
                .map_or(content.len(), |next| next.start);

            let heading_line_end = content[found[i].start..end]
                .find('\n')
                .map_or(end, |n| found[i].start + n + 1);

            Some(Region {
                span: found[i].start..end,
                body: heading_line_end,
            })
        }
        TargetKind::Block => {
            let wanted = target.trim().trim_start_matches('^');
            let found = blocks(content)
                .into_iter()
                .find(|b| b.id.eq_ignore_ascii_case(wanted))?;
            Some(Region {
                body: found.span.start,
                span: found.span,
            })
        }
    }
}

/// Re-attach `text` to the note, keeping whatever blank-line tail the region it
/// replaces already had — and never letting new text run into the line below.
fn with_tail(text: &str, region: &str, at_eof: bool) -> String {
    let body = text.trim_end();
    let tail = &region[region.trim_end().len()..];
    if !tail.is_empty() {
        format!("{}{}", body, tail)
    } else if at_eof {
        body.to_string()
    } else {
        format!("{}\n", body)
    }
}

/// Add `content` after the last non-blank line of the region.
pub(crate) fn append(note: &str, region: &Region, content: &str) -> String {
    let text = &note[region.span.clone()];
    let merged = format!("{}\n{}", text.trim_end(), content.trim_end());
    format!(
        "{}{}{}",
        &note[..region.span.start],
        with_tail(&merged, text, region.span.end == note.len()),
        &note[region.span.end..]
    )
}

/// Insert `content` at the top of the region's body — under the heading, or
/// above the block.
pub(crate) fn prepend(note: &str, region: &Region, content: &str) -> String {
    format!(
        "{}{}\n{}",
        &note[..region.body],
        content.trim_end(),
        &note[region.body..]
    )
}

/// Overwrite the region's body. A heading keeps its heading line.
pub(crate) fn replace(note: &str, region: &Region, content: &str) -> String {
    let body = &note[region.body..region.span.end];
    format!(
        "{}{}{}",
        &note[..region.body],
        with_tail(content, body, region.span.end == note.len()),
        &note[region.span.end..]
    )
}

/// Replace the first occurrence of `search` *within the region*. `None` when the
/// region doesn't contain it — the caller reports that rather than editing
/// somewhere the model didn't ask for.
pub(crate) fn find_and_replace(
    note: &str,
    region: &Region,
    search: &str,
    content: &str,
) -> Option<String> {
    let text = &note[region.span.clone()];
    if !text.contains(search) {
        return None;
    }
    Some(format!(
        "{}{}{}",
        &note[..region.span.start],
        text.replacen(search, content, 1),
        &note[region.span.end..]
    ))
}

/// The note's patchable targets, as markdown. This is what `read-note
/// view=outline` returns: without it the model has to guess what to aim a patch
/// at, and a guessed target is a failed edit.
pub(crate) fn outline(content: &str) -> String {
    let mut out = String::new();

    let fields = super::frontmatter::parse_fields(content).unwrap_or_default();
    if !fields.is_empty() {
        let keys: Vec<&str> = fields.keys().map(String::as_str).collect();
        out.push_str(&format!("frontmatter keys: {}\n", keys.join(", ")));
    }

    let headings = headings(content);
    let blocks = blocks(content);
    if headings.is_empty() && blocks.is_empty() {
        out.push_str("no headings or block references\n");
        return out;
    }

    // The target is *quoted*, and nothing but the target is inside the quotes.
    //
    // This used to print `## Log (line 9)`, and a model that faithfully copied
    // that line as its `target` got `TargetNotFound` — the ` (line 9)` came along
    // with it. The one tool whose entire purpose is to tell the patcher what to
    // aim at was handing it something the patcher cannot find.
    out.push_str("targets for edit-note — pass targetType and the quoted target:\n");
    for h in &headings {
        out.push_str(&format!(
            "  heading  \"{} {}\"  (line {})\n",
            "#".repeat(h.level),
            h.text,
            h.line
        ));
    }
    for b in &blocks {
        out.push_str(&format!("  block    \"^{}\"  (line {})\n", b.id, b.line));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTE: &str = "\
---
title: T
---
# Top

intro

## Log

first entry

## Notes

a note ^n1

### Deep

deep text

## After
";

    fn region(content: &str, kind: TargetKind, target: &str) -> Region {
        find_region(content, &kind, target).expect("target found")
    }

    // ── Finding targets ──────────────────────────────────────────────────────

    #[test]
    fn heading_matches_with_or_without_its_hashes() {
        let a = region(NOTE, TargetKind::Heading, "Log");
        let b = region(NOTE, TargetKind::Heading, "## Log");
        assert_eq!(a.span, b.span);
        assert!(NOTE[a.span.clone()].starts_with("## Log"));
    }

    #[test]
    fn heading_match_is_case_insensitive() {
        assert!(find_region(NOTE, &TargetKind::Heading, "log").is_some());
    }

    #[test]
    fn heading_level_must_agree_when_given() {
        assert!(
            find_region(NOTE, &TargetKind::Heading, "### Log").is_none(),
            "'Log' is an H2, so an H3 target must not match it"
        );
    }

    #[test]
    fn a_section_stops_at_the_next_heading_of_the_same_level() {
        let r = region(NOTE, TargetKind::Heading, "## Log");
        assert_eq!(&NOTE[r.span], "## Log\n\nfirst entry\n\n");
    }

    #[test]
    fn a_section_swallows_its_nested_headings() {
        let r = region(NOTE, TargetKind::Heading, "## Notes");
        let text = &NOTE[r.span];
        assert!(
            text.contains("### Deep"),
            "a deeper heading is part of the section: {text:?}"
        );
        assert!(!text.contains("## After"));
    }

    #[test]
    fn a_headings_body_starts_after_its_own_line() {
        let r = region(NOTE, TargetKind::Heading, "# Top");
        assert!(NOTE[r.body..].starts_with("\nintro"));
    }

    #[test]
    fn missing_target_is_not_found() {
        assert!(find_region(NOTE, &TargetKind::Heading, "Ghost").is_none());
        assert!(find_region(NOTE, &TargetKind::Block, "ghost").is_none());
    }

    #[test]
    fn block_matches_with_or_without_its_caret() {
        let a = region(NOTE, TargetKind::Block, "n1");
        let b = region(NOTE, TargetKind::Block, "^n1");
        assert_eq!(a.span, b.span);
        assert_eq!(&NOTE[a.span], "a note ^n1\n");
    }

    #[test]
    fn a_block_covers_its_whole_paragraph() {
        let note = "para line one\npara line two ^id\n\nafter\n";
        let r = region(note, TargetKind::Block, "id");
        assert_eq!(&note[r.span], "para line one\npara line two ^id\n");
    }

    // ── What is *not* a target ───────────────────────────────────────────────

    #[test]
    fn a_tag_at_the_start_of_a_line_is_not_a_heading() {
        assert!(
            find_region(
                "#todo write this\n",
                &TargetKind::Heading,
                "todo write this"
            )
            .is_none(),
            "#todo is a tag — a heading needs a space after the hashes"
        );
    }

    #[test]
    fn a_heading_inside_a_code_fence_is_not_a_heading() {
        let note = "# Real\n\n```md\n## Fake\n```\n\ntext\n";
        assert!(find_region(note, &TargetKind::Heading, "Fake").is_none());
        assert!(find_region(note, &TargetKind::Heading, "Real").is_some());
    }

    #[test]
    fn a_hash_inside_the_frontmatter_is_not_a_heading() {
        let note = "---\n# just a yaml comment\ntitle: T\n---\n\nbody\n";
        assert!(find_region(note, &TargetKind::Heading, "just a yaml comment").is_none());
    }

    #[test]
    fn a_caret_glued_to_a_word_is_not_a_block_marker() {
        assert!(
            find_region("the value is 2^n\n", &TargetKind::Block, "n").is_none(),
            "2^n is maths, not a block reference"
        );
    }

    // ── Applying an edit ─────────────────────────────────────────────────────

    #[test]
    fn append_lands_at_the_end_of_the_section_not_the_note() {
        let r = region(NOTE, TargetKind::Heading, "## Log");
        let out = append(NOTE, &r, "second entry");
        assert!(
            out.contains("first entry\nsecond entry\n\n## Notes"),
            "{out}"
        );
    }

    #[test]
    fn append_keeps_the_blank_line_before_the_next_heading() {
        let r = region(NOTE, TargetKind::Heading, "## Log");
        let out = append(NOTE, &r, "x");
        assert!(
            out.contains("x\n\n## Notes"),
            "the blank line must survive: {out}"
        );
    }

    #[test]
    fn prepend_lands_under_the_heading_not_above_it() {
        let r = region(NOTE, TargetKind::Heading, "## Log");
        let out = prepend(NOTE, &r, "newest");
        assert!(out.contains("## Log\nnewest\n\nfirst entry"), "{out}");
    }

    #[test]
    fn replace_keeps_the_heading_line() {
        let r = region(NOTE, TargetKind::Heading, "## Log");
        let out = replace(NOTE, &r, "rewritten");
        assert!(out.contains("## Log\nrewritten\n\n## Notes"), "{out}");
        assert!(out.contains("# Top"), "the rest of the note is untouched");
    }

    #[test]
    fn replacing_a_section_does_not_touch_its_siblings() {
        let r = region(NOTE, TargetKind::Heading, "## Log");
        let out = replace(NOTE, &r, "x");
        assert!(out.contains("### Deep"));
        assert!(out.contains("## After"));
        assert!(out.starts_with("---\ntitle: T\n---\n# Top"));
    }

    #[test]
    fn replace_a_block_swaps_only_that_block() {
        let r = region(NOTE, TargetKind::Block, "n1");
        let out = replace(NOTE, &r, "swapped ^n1");
        assert!(out.contains("## Notes\n\nswapped ^n1\n\n### Deep"), "{out}");
    }

    #[test]
    fn append_to_the_last_section_does_not_grow_a_trailing_newline() {
        let r = region(NOTE, TargetKind::Heading, "## After");
        let out = append(NOTE, &r, "tail");
        assert!(out.ends_with("## After\ntail\n"), "{out:?}");
    }

    #[test]
    fn replace_into_an_empty_section_does_not_glue_it_to_the_next_heading() {
        let note = "## A\n## B\n";
        let r = region(note, TargetKind::Heading, "## A");
        assert_eq!(replace(note, &r, "body"), "## A\nbody\n## B\n");
    }

    #[test]
    fn find_and_replace_is_confined_to_the_region() {
        let note = "## A\n\nneedle\n\n## B\n\nneedle\n";
        let r = region(note, TargetKind::Heading, "## B");
        let out = find_and_replace(note, &r, "needle", "found").unwrap();
        assert_eq!(
            out, "## A\n\nneedle\n\n## B\n\nfound\n",
            "only B's needle may change"
        );
    }

    #[test]
    fn find_and_replace_reports_a_miss_rather_than_editing_elsewhere() {
        let note = "## A\n\nneedle\n\n## B\n\nother\n";
        let r = region(note, TargetKind::Heading, "## B");
        assert!(find_and_replace(note, &r, "needle", "x").is_none());
    }

    // ── Outline ──────────────────────────────────────────────────────────────

    #[test]
    fn outline_lists_every_patchable_target() {
        let out = outline(NOTE);
        assert!(out.contains("frontmatter keys: title"));
        assert!(out.contains("heading  \"# Top\"  (line 4)"), "{out}");
        assert!(out.contains("heading  \"## Log\"  (line 8)"), "{out}");
        assert!(out.contains("heading  \"### Deep\"  (line 16)"), "{out}");
        assert!(out.contains("block    \"^n1\"  (line 14)"), "{out}");
    }

    #[test]
    fn every_heading_the_outline_offers_can_actually_be_found() {
        for h in headings(NOTE) {
            assert!(
                find_region(NOTE, &TargetKind::Heading, &h.text).is_some(),
                "outline offered '{}' but the patcher can't find it",
                h.text
            );
        }
    }

    #[test]
    fn a_target_copied_verbatim_out_of_the_rendered_outline_resolves() {
        // The test above checks the *parsed* headings, which is not what a model
        // sees — it sees the rendered text. The outline used to print
        // `## Log (line 9)`, so a model copying that line faithfully got
        // TargetNotFound: the ` (line 9)` came with it. This asserts on the bytes
        // we actually emit, which is where the contract really lives.
        let rendered = outline(NOTE);
        let mut checked = 0;

        for line in rendered.lines() {
            let trimmed = line.trim();
            let kind = if trimmed.starts_with("heading ") {
                TargetKind::Heading
            } else if trimmed.starts_with("block ") {
                TargetKind::Block
            } else {
                continue;
            };
            // Everything the model should copy sits between the quotes.
            let target = trimmed
                .split('"')
                .nth(1)
                .unwrap_or_else(|| panic!("no quoted target in outline line: {line:?}"));

            assert!(
                find_region(NOTE, &kind, target).is_some(),
                "the outline printed {target:?}, but the patcher cannot find it"
            );
            checked += 1;
        }

        assert!(checked >= 4, "the outline offered no targets at all");
    }

    #[test]
    fn outline_says_so_when_there_is_nothing_to_aim_at() {
        assert!(outline("just prose\n").contains("no headings or block references"));
    }
}
