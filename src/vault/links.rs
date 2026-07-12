use std::{
    collections::HashMap,
    ops::Range,
    path::{Path, PathBuf},
};

use rayon::prelude::*;
use schemars::JsonSchema;
use serde::Serialize;

use super::walk::md_files;

/// How a link was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LinkKind {
    /// `[[Note]]`
    Wiki,
    /// `[text](Note.md)`
    Markdown,
}

/// One link occurrence in a note.
#[derive(Debug, Clone)]
pub(crate) struct Link {
    /// Byte range of the whole link in the source, so it can be rewritten.
    pub span: Range<usize>,
    /// The note being linked to, as written — no `#heading`, no `|alias`.
    pub target: String,
    /// `#heading` or `#^block` suffix, without the `#`.
    pub heading: Option<String>,
    /// `|alias` suffix, without the `|`.
    pub alias: Option<String>,
    /// Written as `![[…]]` — an embed rather than a reference.
    pub embed: bool,
    pub kind: LinkKind,
    /// 1-based line number.
    pub line: usize,
}

/// Byte ranges covered by fenced or inline code.
///
/// Load-bearing: a `[[wikilink]]` inside a code block is an *example*, not a
/// reference. Rewriting it on rename would corrupt the very documentation that
/// explains the syntax, and counting it would invent links that don't exist.
pub(crate) fn code_spans(content: &str) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut fence: Option<char> = None;
    let mut offset = 0;

    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();

        // A fence opens or closes on a line of three-or-more ` or ~.
        let fence_char = trimmed.chars().next().filter(|c| *c == '`' || *c == '~');
        let is_fence = fence_char
            .map(|c| trimmed.chars().take_while(|x| *x == c).count() >= 3)
            .unwrap_or(false);

        if let Some(open) = fence {
            spans.push(offset..offset + line.len());
            if is_fence && fence_char == Some(open) {
                fence = None;
            }
        } else if is_fence {
            fence = Some(fence_char.expect("is_fence implies a fence char"));
            spans.push(offset..offset + line.len());
        } else {
            spans.extend(inline_code_spans(line, offset + indent, trimmed));
        }
        offset += line.len();
    }
    spans
}

/// Backtick-delimited spans within a single line. Runs must match in length, so
/// ``a ` b`` closes correctly.
fn inline_code_spans(_line: &str, base: usize, text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let run = bytes[i..].iter().take_while(|b| **b == b'`').count();
        let start = i;
        let mut j = i + run;
        while j < bytes.len() {
            if bytes[j] == b'`' {
                let close = bytes[j..].iter().take_while(|b| **b == b'`').count();
                if close == run {
                    spans.push(base + start..base + j + close);
                    break;
                }
                j += close;
            } else {
                j += 1;
            }
        }
        // Unterminated run — the rest of the line is ordinary text.
        if j >= bytes.len() {
            break;
        }
        i = j + run;
    }
    spans
}

pub(crate) fn in_code(spans: &[Range<usize>], pos: usize) -> bool {
    spans.iter().any(|s| s.contains(&pos))
}

/// A link target that names a protocol is somewhere else entirely.
fn is_external(target: &str) -> bool {
    let lower = target.to_ascii_lowercase();
    ["http://", "https://", "mailto:", "obsidian://", "ftp://"]
        .iter()
        .any(|p| lower.starts_with(p))
}

/// Percent-decode the `%20`-style escapes Obsidian writes into markdown links.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(byte as char);
            i += 3;
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Split `Note#heading|alias` into its parts.
fn split_target(raw: &str) -> (String, Option<String>, Option<String>) {
    let (before_alias, alias) = match raw.split_once('|') {
        Some((t, a)) => (t, Some(a.trim().to_string())),
        None => (raw, None),
    };
    let (target, heading) = match before_alias.split_once('#') {
        Some((t, h)) => (t, Some(h.trim().to_string())),
        None => (before_alias, None),
    };
    (target.trim().to_string(), heading, alias)
}

/// Every link in `content`, skipping anything inside code.
pub(crate) fn parse_links(content: &str) -> Vec<Link> {
    let code = code_spans(content);
    let bytes = content.as_bytes();

    // Line number for a byte offset, computed once by prefix scan.
    let mut line_starts = vec![0usize];
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' {
            line_starts.push(i + 1);
        }
    }
    let line_of = |pos: usize| match line_starts.binary_search(&pos) {
        Ok(i) => i + 1,
        Err(i) => i,
    };

    let mut links = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        // ── [[wiki]] ────────────────────────────────────────────────────────
        if bytes[i] == b'['
            && bytes.get(i + 1) == Some(&b'[')
            && let Some(rel) = content[i + 2..].find("]]")
        {
            {
                let inner_end = i + 2 + rel;
                let end = inner_end + 2;
                let embed = i > 0 && bytes[i - 1] == b'!';
                let start = if embed { i - 1 } else { i };
                if !in_code(&code, start) {
                    let (target, heading, alias) = split_target(&content[i + 2..inner_end]);
                    if !target.is_empty() && !is_external(&target) {
                        links.push(Link {
                            span: start..end,
                            target,
                            heading,
                            alias,
                            embed,
                            kind: LinkKind::Wiki,
                            line: line_of(start),
                        });
                    }
                }
                i = end;
                continue;
            }
        }

        // ── [text](target) ──────────────────────────────────────────────────
        if bytes[i] == b'['
            && let Some(close) = content[i..].find("](")
        {
            {
                let text_end = i + close;
                if bytes.get(text_end + 2).is_some()
                    && let Some(rel) = content[text_end + 2..].find(')')
                {
                    let target_end = text_end + 2 + rel;
                    let raw = &content[text_end + 2..target_end];
                    let embed = i > 0 && bytes[i - 1] == b'!';
                    let start = if embed { i - 1 } else { i };
                    if !in_code(&code, start) && !is_external(raw) && !raw.is_empty() {
                        let (target, heading, _) = split_target(&percent_decode(raw));
                        // Only vault notes participate in the graph. The `.md`
                        // is dropped so a target reads the same whether it was
                        // written as `[[Note]]` or `[x](Note.md)`.
                        if let Some(target) = target.strip_suffix(".md") {
                            let target = target.to_string();
                            links.push(Link {
                                span: start..target_end + 1,
                                target,
                                heading,
                                alias: Some(content[i + 1..text_end].to_string()),
                                embed,
                                kind: LinkKind::Markdown,
                                line: line_of(start),
                            });
                            i = target_end + 1;
                            continue;
                        }
                    }
                }
            }
        }
        i += 1;
    }
    links
}

/// Resolves link targets to vault notes, the way Obsidian does: a bare name
/// matches by basename anywhere in the vault; a name containing `/` is a
/// vault-relative path.
pub(crate) struct Resolver {
    /// Lowercased file stem → notes with that name.
    by_stem: HashMap<String, Vec<PathBuf>>,
    /// Lowercased vault-relative path, without `.md` → note.
    by_path: HashMap<String, PathBuf>,
}

impl Resolver {
    pub(crate) fn new(root: &Path, files: &[PathBuf]) -> Self {
        let mut by_stem: HashMap<String, Vec<PathBuf>> = HashMap::new();
        let mut by_path = HashMap::new();
        for path in files {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                by_stem
                    .entry(stem.to_lowercase())
                    .or_default()
                    .push(path.clone());
            }
            if let Ok(rel) = path.strip_prefix(root) {
                let key = rel.with_extension("").to_string_lossy().to_lowercase();
                by_path.insert(key.replace('\\', "/"), path.clone());
            }
        }
        Self { by_stem, by_path }
    }

    /// The note `target` refers to, as written in `from`. `None` means the link
    /// is broken.
    pub(crate) fn resolve(&self, target: &str, from: &Path) -> Option<PathBuf> {
        let key = target
            .trim_end_matches(".md")
            .trim_matches('/')
            .to_lowercase()
            .replace('\\', "/");
        if key.is_empty() {
            return None;
        }

        if key.contains('/')
            && let Some(hit) = self.by_path.get(&key)
        {
            return Some(hit.clone());
        }

        let candidates = self.by_stem.get(key.rsplit('/').next().unwrap_or(&key))?;
        match candidates.len() {
            0 => None,
            1 => Some(candidates[0].clone()),
            // Ambiguous basename — Obsidian prefers the one nearest the source.
            _ => {
                let here = from.parent();
                candidates
                    .iter()
                    .find(|c| c.parent() == here)
                    .or_else(|| candidates.iter().min_by_key(|c| c.components().count()))
                    .cloned()
            }
        }
    }

    /// Whether a bare basename would resolve unambiguously.
    fn stem_is_unique(&self, stem: &str) -> bool {
        self.by_stem
            .get(&stem.to_lowercase())
            .is_some_and(|v| v.len() == 1)
    }
}

/// One edge of the link graph, as reported by the `wikilinks` tool.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LinkRef {
    /// Vault-relative path of the note containing the link.
    pub from: String,
    /// The target exactly as written.
    pub target: String,
    /// Vault-relative path of the note it resolves to, absent when broken.
    pub resolved: Option<String>,
    pub line: usize,
    pub kind: LinkKind,
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Every link in the vault, resolved. One parallel pass.
pub(crate) fn link_graph(root: &Path) -> (Vec<PathBuf>, Resolver, Vec<LinkRef>) {
    let files = md_files(root);
    let resolver = Resolver::new(root, &files);

    let refs: Vec<LinkRef> = files
        .par_iter()
        .flat_map_iter(|path| {
            let content = std::fs::read_to_string(path).unwrap_or_default();
            let from = rel(root, path);
            parse_links(&content)
                .into_iter()
                .map(|link| LinkRef {
                    from: from.clone(),
                    resolved: resolver.resolve(&link.target, path).map(|p| rel(root, &p)),
                    target: link.target,
                    line: link.line,
                    kind: link.kind,
                })
                .collect::<Vec<_>>()
        })
        .collect();

    (files, resolver, refs)
}

/// Rewrite every link in `content` that points at `old` so it points at `new`.
///
/// A link written as a bare name (`[[Note]]`) keeps that shape when the new
/// basename is still unique, which is why an ordinary folder move rewrites
/// nothing at all. Path-style links (`[[folder/Note]]`) follow the note to its
/// new folder. `#heading`, `|alias` and `!embed` are preserved.
pub(crate) fn rewrite_links(
    content: &str,
    from: &Path,
    old: &Path,
    new_rel: &str,
    resolver: &Resolver,
) -> Option<String> {
    let new_stem = Path::new(new_rel)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(new_rel);
    let new_path = new_rel.trim_end_matches(".md");

    let mut edits: Vec<(Range<usize>, String)> = Vec::new();

    for link in parse_links(content) {
        if resolver.resolve(&link.target, from).as_deref() != Some(old) {
            continue;
        }

        // Keep the shape the author used.
        let was_path = link.target.contains('/');
        let bare_still_works = resolver.stem_is_unique(new_stem);
        let target = if was_path || !bare_still_works {
            new_path.to_string()
        } else {
            new_stem.to_string()
        };

        let replacement = match link.kind {
            LinkKind::Wiki => {
                let mut inner = target;
                if let Some(h) = &link.heading {
                    inner.push('#');
                    inner.push_str(h);
                }
                if let Some(a) = &link.alias {
                    inner.push('|');
                    inner.push_str(a);
                }
                format!("{}[[{}]]", if link.embed { "!" } else { "" }, inner)
            }
            LinkKind::Markdown => {
                let mut href = format!("{}.md", target);
                if let Some(h) = &link.heading {
                    href.push('#');
                    href.push_str(h);
                }
                format!(
                    "{}[{}]({})",
                    if link.embed { "!" } else { "" },
                    link.alias.clone().unwrap_or_default(),
                    href.replace(' ', "%20")
                )
            }
        };

        if content[link.span.clone()] != replacement {
            edits.push((link.span, replacement));
        }
    }

    if edits.is_empty() {
        return None;
    }

    // Apply right-to-left so earlier spans keep their offsets.
    let mut out = content.to_string();
    for (span, text) in edits.into_iter().rev() {
        out.replace_range(span, &text);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(content: &str) -> Vec<String> {
        parse_links(content).into_iter().map(|l| l.target).collect()
    }

    #[test]
    fn parses_the_wikilink_shapes() {
        let links = parse_links("[[Plain]] [[Note#Heading]] [[Note|alias]] ![[Embed]]\n");
        assert_eq!(links.len(), 4);
        assert_eq!(links[0].target, "Plain");
        assert_eq!(links[1].heading.as_deref(), Some("Heading"));
        assert_eq!(links[2].alias.as_deref(), Some("alias"));
        assert!(links[3].embed);
    }

    #[test]
    fn parses_block_reference_targets() {
        let links = parse_links("[[Note#^block-id]]\n");
        assert_eq!(links[0].target, "Note");
        assert_eq!(links[0].heading.as_deref(), Some("^block-id"));
    }

    #[test]
    fn parses_markdown_links_to_notes() {
        let links = parse_links("[text](My%20Note.md) and [x](https://example.com)\n");
        assert_eq!(links.len(), 1, "external URLs are not vault links");
        assert_eq!(links[0].target, "My Note");
        assert_eq!(links[0].kind, LinkKind::Markdown);
    }

    #[test]
    fn ignores_links_inside_fenced_code() {
        let content = "real [[Alpha]]\n\n```\nexample [[Beta]]\n```\n";
        assert_eq!(targets(content), vec!["Alpha"]);
    }

    #[test]
    fn ignores_links_inside_tilde_fences() {
        let content = "~~~\n[[Beta]]\n~~~\n[[Alpha]]\n";
        assert_eq!(targets(content), vec!["Alpha"]);
    }

    #[test]
    fn ignores_links_inside_inline_code() {
        let content = "use `[[Beta]]` to link, like [[Alpha]]\n";
        assert_eq!(targets(content), vec!["Alpha"]);
    }

    #[test]
    fn records_line_numbers() {
        let links = parse_links("one\ntwo [[Target]]\n");
        assert_eq!(links[0].line, 2);
    }

    // ── Resolution ───────────────────────────────────────────────────────────

    fn resolver(paths: &[&str]) -> (PathBuf, Vec<PathBuf>, Resolver) {
        let root = PathBuf::from("/vault");
        let files: Vec<PathBuf> = paths.iter().map(|p| root.join(p)).collect();
        let r = Resolver::new(&root, &files);
        (root, files, r)
    }

    #[test]
    fn resolves_a_bare_name_anywhere_in_the_vault() {
        let (root, _, r) = resolver(&["notes/Target.md", "other.md"]);
        let hit = r.resolve("Target", &root.join("other.md")).unwrap();
        assert!(hit.ends_with("notes/Target.md"));
    }

    #[test]
    fn resolves_a_path_style_target() {
        let (root, _, r) = resolver(&["a/Note.md", "b/Note.md"]);
        let hit = r.resolve("b/Note", &root.join("x.md")).unwrap();
        assert!(hit.ends_with("b/Note.md"));
    }

    #[test]
    fn an_ambiguous_name_prefers_the_source_folder() {
        let (root, _, r) = resolver(&["a/Note.md", "b/Note.md"]);
        let hit = r.resolve("Note", &root.join("b/from.md")).unwrap();
        assert!(hit.ends_with("b/Note.md"));
    }

    #[test]
    fn an_unknown_target_is_broken() {
        let (root, _, r) = resolver(&["a.md"]);
        assert!(r.resolve("Ghost", &root.join("a.md")).is_none());
    }

    // ── Rewriting ────────────────────────────────────────────────────────────

    #[test]
    fn a_rename_rewrites_bare_links() {
        let (root, _, r) = resolver(&["Old.md", "src.md"]);
        let out = rewrite_links(
            "see [[Old]] here\n",
            &root.join("src.md"),
            &root.join("Old.md"),
            "New.md",
            &r,
        )
        .unwrap();
        assert_eq!(out, "see [[New]] here\n");
    }

    #[test]
    fn a_rename_preserves_heading_alias_and_embed() {
        let (root, _, r) = resolver(&["Old.md", "src.md"]);
        let out = rewrite_links(
            "![[Old#Sec|the alias]]\n",
            &root.join("src.md"),
            &root.join("Old.md"),
            "New.md",
            &r,
        )
        .unwrap();
        assert_eq!(out, "![[New#Sec|the alias]]\n");
    }

    #[test]
    fn a_plain_folder_move_rewrites_nothing() {
        // The basename is unchanged and still unique, so `[[Note]]` keeps
        // resolving — there is nothing to rewrite.
        let (root, _, r) = resolver(&["Note.md", "src.md"]);
        let out = rewrite_links(
            "see [[Note]]\n",
            &root.join("src.md"),
            &root.join("Note.md"),
            "archive/Note.md",
            &r,
        );
        assert!(out.is_none(), "expected no rewrite, got {:?}", out);
    }

    #[test]
    fn a_path_style_link_follows_the_note() {
        let (root, _, r) = resolver(&["old/Note.md", "src.md"]);
        let out = rewrite_links(
            "see [[old/Note]]\n",
            &root.join("src.md"),
            &root.join("old/Note.md"),
            "new/Note.md",
            &r,
        )
        .unwrap();
        assert_eq!(out, "see [[new/Note]]\n");
    }

    #[test]
    fn a_markdown_link_is_rewritten_with_its_extension() {
        let (root, _, r) = resolver(&["Old.md", "src.md"]);
        let out = rewrite_links(
            "[label](Old.md)\n",
            &root.join("src.md"),
            &root.join("Old.md"),
            "New Name.md",
            &r,
        )
        .unwrap();
        assert_eq!(out, "[label](New%20Name.md)\n");
    }

    #[test]
    fn a_link_inside_code_is_never_rewritten() {
        let (root, _, r) = resolver(&["Old.md", "src.md"]);
        let content = "```\n[[Old]]\n```\n[[Old]]\n";
        let out = rewrite_links(
            content,
            &root.join("src.md"),
            &root.join("Old.md"),
            "New.md",
            &r,
        )
        .unwrap();
        assert_eq!(
            out, "```\n[[Old]]\n```\n[[New]]\n",
            "the code sample must survive the rename"
        );
    }

    #[test]
    fn multiple_links_on_one_line_are_all_rewritten() {
        let (root, _, r) = resolver(&["Old.md", "src.md"]);
        let out = rewrite_links(
            "[[Old]] and [[Old|a]]\n",
            &root.join("src.md"),
            &root.join("Old.md"),
            "New.md",
            &r,
        )
        .unwrap();
        assert_eq!(out, "[[New]] and [[New|a]]\n");
    }
}
