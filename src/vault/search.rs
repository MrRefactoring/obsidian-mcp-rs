use std::{fs, path::Path};

use rayon::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::frontmatter::content_has_tag;
use super::walk::md_files;

/// One file's search hits. Serialized into the `search-vault` tool's
/// `structuredContent` and also used to build the human-readable text.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SearchResult {
    pub path: String,
    pub filename: String,
    pub matches: Vec<String>,
}

/// Structured output payload for `search-vault` — its `structuredContent` and
/// declared `outputSchema`. The list is wrapped in an object so the schema root
/// is an object, as MCP requires.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SearchOutput {
    pub results: Vec<SearchResult>,
}

/// What a query is matched against. Deriving `Deserialize`/`JsonSchema` here (in
/// the domain, which owns the vocabulary) means an unknown value is rejected as
/// `INVALID_PARAMS` instead of silently degrading to `Content`, and the tool's
/// `inputSchema` advertises the legal values rather than burying them in prose.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SearchType {
    #[default]
    Content,
    Filename,
    Both,
}

/// Walk `search_root`, returning matches. `root` is the vault root, used only
/// to compute paths relative to the vault for display.
pub(crate) fn search(
    root: &Path,
    search_root: &Path,
    query: &str,
    case_sensitive: bool,
    search_type: &SearchType,
) -> Vec<SearchResult> {
    let query_lower = if case_sensitive {
        query.to_string()
    } else {
        query.to_lowercase()
    };

    let tag_search = query.starts_with("tag:");
    let tag_value = if tag_search {
        Some(query.trim_start_matches("tag:"))
    } else {
        None
    };

    let mut results: Vec<SearchResult> = md_files(search_root)
        .par_iter()
        .filter_map(|path| {
            let rel_path = path
                .strip_prefix(root)
                .unwrap_or(path)
                .display()
                .to_string();
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            let mut match_lines = Vec::new();

            if tag_search {
                if let Some(tag) = tag_value
                    && let Ok(content) = fs::read_to_string(path)
                    && content_has_tag(&content, tag)
                {
                    match_lines.push(format!("tag: {}", tag));
                }
            } else {
                let filename_match = matches!(search_type, SearchType::Filename | SearchType::Both)
                    && {
                        let fname_cmp = if case_sensitive {
                            filename.clone()
                        } else {
                            filename.to_lowercase()
                        };
                        fname_cmp.contains(&query_lower)
                    };

                if filename_match {
                    match_lines.push(format!("filename: {}", filename));
                }

                if matches!(search_type, SearchType::Content | SearchType::Both)
                    && let Ok(content) = fs::read_to_string(path)
                {
                    // Lowercase the whole file once (not per line) when doing a
                    // case-insensitive search, then compare line-by-line.
                    let haystack = if case_sensitive {
                        None
                    } else {
                        Some(content.to_lowercase())
                    };
                    let cmp_lines: Box<dyn Iterator<Item = &str>> = match &haystack {
                        Some(lc) => Box::new(lc.lines()),
                        None => Box::new(content.lines()),
                    };
                    for (i, (line, line_cmp)) in content.lines().zip(cmp_lines).enumerate() {
                        if line_cmp.contains(&query_lower) {
                            match_lines.push(format!("line {}: {}", i + 1, line.trim()));
                        }
                    }
                }
            }

            if match_lines.is_empty() {
                return None;
            }
            Some(SearchResult {
                path: rel_path,
                filename: filename.trim_end_matches(".md").to_string(),
                matches: match_lines,
            })
        })
        .collect();

    results.sort_by(|a, b| a.path.cmp(&b.path));
    results
}
