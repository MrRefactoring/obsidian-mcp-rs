use std::{fs, path::Path};

use walkdir::WalkDir;

use super::frontmatter::content_has_tag;

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub path: String,
    pub filename: String,
    pub matches: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SearchType {
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

    let mut results = Vec::new();

    for entry in WalkDir::new(search_root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| x == "md")
                .unwrap_or(false)
        })
    {
        let path = entry.path();
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
                for (i, line) in content.lines().enumerate() {
                    let line_cmp = if case_sensitive {
                        line.to_string()
                    } else {
                        line.to_lowercase()
                    };
                    if line_cmp.contains(&query_lower) {
                        match_lines.push(format!("line {}: {}", i + 1, line.trim()));
                    }
                }
            }
        }

        if !match_lines.is_empty() {
            results.push(SearchResult {
                path: rel_path,
                filename: filename.trim_end_matches(".md").to_string(),
                matches: match_lines,
            });
        }
    }

    results.sort_by(|a, b| a.path.cmp(&b.path));
    results
}
