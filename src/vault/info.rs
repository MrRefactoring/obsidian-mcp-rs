//! What's in this vault?
//!
//! A model arriving at a vault it has never seen can search it, but only for
//! words it already knows. It cannot ask "what tags exist here", "what have I
//! been working on", or "how big is this". Those are the questions you ask
//! *before* you know what to search for.
//!
//! Everything here comes from the same parallel walk the rest of the crate uses;
//! nothing is cached, so nothing can be stale.

use std::{collections::HashMap, fs, path::Path};

use chrono::{DateTime, Utc};
use rayon::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::frontmatter::extract_frontmatter;
use super::links::link_graph;
use super::tags::inline_tags;
use super::walk::md_files;

/// Default number of notes `recent` returns.
pub const DEFAULT_RECENT: usize = 20;

/// Which question `vault-info` is answering.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum InfoQuery {
    /// Every tag in the vault with the number of notes carrying it, commonest
    /// first. Frontmatter and inline `#tags` both count.
    Tags,
    /// Notes by last modified, newest first — what was worked on recently.
    Recent,
    /// Vault size and shape: notes, folders, tags, links, broken links.
    Stats,
}

/// A tag and how many notes carry it.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TagCount {
    pub tag: String,
    /// Notes carrying this tag — not occurrences.
    pub notes: usize,
}

/// A note and when it was last touched.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RecentNote {
    pub path: String,
    /// Last modified, RFC 3339 / ISO 8601, UTC.
    pub modified: String,
    pub bytes: u64,
}

/// The vault's shape.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Stats {
    pub notes: usize,
    pub folders: usize,
    pub bytes: u64,
    /// Distinct tags.
    pub tags: usize,
    pub links: usize,
    /// Links whose target does not exist — worth fixing.
    pub broken_links: usize,
}

/// Answer to a `vault-info` query. Only the field the query asked for is filled.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct InfoOutput {
    pub tags: Vec<TagCount>,
    pub recent: Vec<RecentNote>,
    pub stats: Option<Stats>,
}

impl InfoOutput {
    fn empty() -> Self {
        Self {
            tags: Vec::new(),
            recent: Vec::new(),
            stats: None,
        }
    }
}

/// Every tag a note carries, deduplicated — a note that says `#rust` five times
/// still counts once.
fn tags_of(content: &str) -> Vec<String> {
    let mut tags: Vec<String> = extract_frontmatter(content)
        .map(|fm| fm.tags)
        .unwrap_or_default();
    tags.extend(inline_tags(content));

    tags.iter_mut().for_each(|t| *t = t.to_lowercase());
    tags.sort();
    tags.dedup();
    tags
}

fn tag_counts(files: &[std::path::PathBuf]) -> Vec<TagCount> {
    let counts = files
        .par_iter()
        .filter_map(|path| fs::read_to_string(path).ok())
        .fold(HashMap::<String, usize>::new, |mut acc, content| {
            for tag in tags_of(&content) {
                *acc.entry(tag).or_default() += 1;
            }
            acc
        })
        .reduce(HashMap::new, |mut a, b| {
            for (tag, n) in b {
                *a.entry(tag).or_default() += n;
            }
            a
        });

    let mut counts: Vec<TagCount> = counts
        .into_iter()
        .map(|(tag, notes)| TagCount { tag, notes })
        .collect();
    // Commonest first; alphabetical within a count, so the order is stable.
    counts.sort_by(|a, b| b.notes.cmp(&a.notes).then_with(|| a.tag.cmp(&b.tag)));
    counts
}

fn recent(root: &Path, files: &[std::path::PathBuf], limit: usize) -> Vec<RecentNote> {
    let mut notes: Vec<(std::time::SystemTime, RecentNote)> = files
        .par_iter()
        .filter_map(|path| {
            let meta = fs::metadata(path).ok()?;
            let modified = meta.modified().ok()?;
            Some((
                modified,
                RecentNote {
                    path: super::rel_path(root, path),
                    modified: DateTime::<Utc>::from(modified).to_rfc3339(),
                    bytes: meta.len(),
                },
            ))
        })
        .collect();

    notes.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.path.cmp(&b.1.path)));
    notes.into_iter().take(limit).map(|(_, n)| n).collect()
}

pub(crate) fn info(root: &Path, query: &InfoQuery, limit: usize) -> InfoOutput {
    let files = md_files(root);

    match query {
        InfoQuery::Tags => {
            // `limit` used to be documented as "ignored by the other queries",
            // which was honest but left no way at all to cap this list — a vault
            // with 500 tags returned all 500, every time.
            let mut tags = tag_counts(&files);
            tags.truncate(limit);
            InfoOutput {
                tags,
                ..InfoOutput::empty()
            }
        }
        InfoQuery::Recent => InfoOutput {
            recent: recent(root, &files, limit),
            ..InfoOutput::empty()
        },
        InfoQuery::Stats => {
            let bytes = files
                .par_iter()
                .filter_map(|p| fs::metadata(p).ok())
                .map(|m| m.len())
                .sum();
            let folders = files
                .iter()
                .filter_map(|p| p.parent())
                .filter(|p| *p != root)
                .collect::<std::collections::HashSet<_>>()
                .len();
            let (_, _, refs) = link_graph(root);

            InfoOutput {
                stats: Some(Stats {
                    notes: files.len(),
                    folders,
                    bytes,
                    tags: tag_counts(&files).len(),
                    links: refs.len(),
                    broken_links: refs.iter().filter(|r| r.resolved.is_none()).count(),
                }),
                ..InfoOutput::empty()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn vault() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("a.md"),
            "---\ntags:\n  - work\n  - rust\n---\n# A\n\nSee [[b]] and #rust again #rust\n",
        )
        .unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(
            dir.path().join("sub/b.md"),
            "# B\n\n#work and a broken [[ghost]]\n\n```sh\n# not a heading, #notatag\n```\n",
        )
        .unwrap();
        dir
    }

    #[test]
    fn tags_counts_notes_not_occurrences() {
        let dir = vault();
        let out = info(dir.path(), &InfoQuery::Tags, DEFAULT_RECENT);
        let counts: Vec<(&str, usize)> =
            out.tags.iter().map(|t| (t.tag.as_str(), t.notes)).collect();

        // Commonest first. `rust` is in a.md's frontmatter *and* twice inline —
        // still one note, because we count notes, not occurrences.
        assert_eq!(counts, vec![("work", 2), ("rust", 1)]);
    }

    #[test]
    fn a_tag_inside_a_code_block_is_not_a_tag() {
        let dir = vault();
        let out = info(dir.path(), &InfoQuery::Tags, DEFAULT_RECENT);
        assert!(
            !out.tags.iter().any(|t| t.tag == "notatag"),
            "a # in a shell snippet is a comment: {:?}",
            out.tags
        );
    }

    #[test]
    fn stats_describe_the_vault() {
        let dir = vault();
        let s = info(dir.path(), &InfoQuery::Stats, DEFAULT_RECENT)
            .stats
            .unwrap();

        assert_eq!(s.notes, 2);
        assert_eq!(s.folders, 1, "only sub/ — the root is not a folder");
        assert_eq!(s.tags, 2, "rust and work");
        assert_eq!(s.links, 2, "[[b]] and [[ghost]]");
        assert_eq!(s.broken_links, 1, "[[ghost]]");
        assert!(s.bytes > 0);
    }

    #[test]
    fn recent_lists_newest_first() {
        let dir = vault();
        // Touch b.md so it is unambiguously the newer of the two.
        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(dir.path().join("sub/b.md"), "touched").unwrap();

        let out = info(dir.path(), &InfoQuery::Recent, DEFAULT_RECENT);
        let paths: Vec<&str> = out.recent.iter().map(|n| n.path.as_str()).collect();

        assert_eq!(paths, vec!["sub/b.md", "a.md"]);
        assert!(
            out.recent[0].modified.contains('T'),
            "an ISO timestamp, not an epoch: {}",
            out.recent[0].modified
        );
    }

    #[test]
    fn recent_respects_the_limit() {
        let dir = vault();
        let out = info(dir.path(), &InfoQuery::Recent, 1);
        assert_eq!(out.recent.len(), 1);
    }
}
