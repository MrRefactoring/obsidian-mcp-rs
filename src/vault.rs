use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use regex::Regex;
use walkdir::WalkDir;

use crate::error::VaultError;

#[derive(Debug, Clone)]
pub struct VaultManager {
    vaults: HashMap<String, PathBuf>,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub vault: String,
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

#[derive(Debug, Clone)]
pub struct Frontmatter {
    pub raw: String,
    pub tags: Vec<String>,
}

impl VaultManager {
    pub fn new(vault_paths: Vec<PathBuf>) -> Self {
        let mut vaults = HashMap::new();
        for path in vault_paths {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("vault")
                .to_string();
            vaults.insert(name, path);
        }
        Self { vaults }
    }

    pub fn list_vaults(&self) -> Vec<(String, PathBuf)> {
        let mut list: Vec<(String, PathBuf)> = self
            .vaults
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        list.sort_by(|a, b| a.0.cmp(&b.0));
        list
    }

    pub fn resolve_vault(&self, name: &str) -> Result<&Path, VaultError> {
        self.vaults.get(name).map(|p| p.as_path()).ok_or_else(|| {
            let available = self.vaults.keys().cloned().collect::<Vec<_>>().join(", ");
            VaultError::VaultNotFound(name.to_string(), available)
        })
    }

    pub fn note_path(
        &self,
        vault: &str,
        filename: &str,
        folder: Option<&str>,
    ) -> Result<PathBuf, VaultError> {
        let root = self.resolve_vault(vault)?;
        let filename = ensure_md_extension(filename);
        Ok(match folder {
            Some(f) if !f.is_empty() => root.join(f).join(&filename),
            _ => root.join(&filename),
        })
    }

    pub fn read_note(
        &self,
        vault: &str,
        filename: &str,
        folder: Option<&str>,
    ) -> Result<String, VaultError> {
        let path = self.note_path(vault, filename, folder)?;
        if !path.exists() {
            return Err(VaultError::NoteNotFound(
                path.display().to_string(),
                vault.to_string(),
            ));
        }
        fs::read_to_string(&path).map_err(|e| VaultError::io(path.display().to_string(), e))
    }

    pub fn create_note(
        &self,
        vault: &str,
        filename: &str,
        content: &str,
        folder: Option<&str>,
    ) -> Result<PathBuf, VaultError> {
        let path = self.note_path(vault, filename, folder)?;
        if path.exists() {
            return Err(VaultError::NoteAlreadyExists(
                path.display().to_string(),
                vault.to_string(),
            ));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| VaultError::io(parent.display().to_string(), e))?;
        }
        fs::write(&path, content).map_err(|e| VaultError::io(path.display().to_string(), e))?;
        Ok(path)
    }

    pub fn edit_note(
        &self,
        vault: &str,
        filename: &str,
        operation: &str,
        content: &str,
        folder: Option<&str>,
        search: Option<&str>,
    ) -> Result<(String, String), VaultError> {
        let path = self.note_path(vault, filename, folder)?;
        if !path.exists() {
            return Err(VaultError::NoteNotFound(
                path.display().to_string(),
                vault.to_string(),
            ));
        }
        let old =
            fs::read_to_string(&path).map_err(|e| VaultError::io(path.display().to_string(), e))?;
        let new = match operation {
            "append" => format!("{}\n{}", old.trim_end(), content),
            "prepend" => format!("{}\n{}", content, old.trim_start()),
            "replace" => content.to_string(),
            "find_and_replace" => {
                let needle = search.ok_or_else(|| {
                    VaultError::InvalidPath(
                        "find_and_replace requires a 'search' parameter".to_string(),
                    )
                })?;
                if !old.contains(needle) {
                    return Err(VaultError::InvalidPath(format!(
                        "Search text not found in note '{}'",
                        filename
                    )));
                }
                old.replacen(needle, content, 1)
            }
            op => {
                return Err(VaultError::InvalidPath(format!(
                    "Unknown operation '{}'. Use: append, prepend, replace, find_and_replace",
                    op
                )));
            }
        };
        fs::write(&path, &new).map_err(|e| VaultError::io(path.display().to_string(), e))?;
        Ok((old, new))
    }

    pub fn delete_note(
        &self,
        vault: &str,
        filename: &str,
        folder: Option<&str>,
    ) -> Result<(), VaultError> {
        let path = self.note_path(vault, filename, folder)?;
        if !path.exists() {
            return Err(VaultError::NoteNotFound(
                path.display().to_string(),
                vault.to_string(),
            ));
        }
        fs::remove_file(&path).map_err(|e| VaultError::io(path.display().to_string(), e))
    }

    pub fn move_note(
        &self,
        vault: &str,
        filename: &str,
        folder: Option<&str>,
        new_folder: Option<&str>,
        new_filename: Option<&str>,
    ) -> Result<PathBuf, VaultError> {
        let src = self.note_path(vault, filename, folder)?;
        if !src.exists() {
            return Err(VaultError::NoteNotFound(
                src.display().to_string(),
                vault.to_string(),
            ));
        }
        let dest_filename = new_filename.unwrap_or(filename);
        let dest = self.note_path(vault, dest_filename, new_folder)?;
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| VaultError::io(parent.display().to_string(), e))?;
        }
        fs::rename(&src, &dest).map_err(|e| VaultError::io(src.display().to_string(), e))?;
        Ok(dest)
    }

    pub fn create_directory(
        &self,
        vault: &str,
        path: &str,
        recursive: bool,
    ) -> Result<PathBuf, VaultError> {
        let root = self.resolve_vault(vault)?;
        let dir = root.join(path);
        if dir.exists() {
            return Err(VaultError::DirectoryAlreadyExists(
                dir.display().to_string(),
            ));
        }
        if recursive {
            fs::create_dir_all(&dir).map_err(|e| VaultError::io(dir.display().to_string(), e))?;
        } else {
            fs::create_dir(&dir).map_err(|e| VaultError::io(dir.display().to_string(), e))?;
        }
        Ok(dir)
    }

    pub fn search_vault(
        &self,
        vault: &str,
        query: &str,
        search_path: Option<&str>,
        case_sensitive: bool,
        search_type: &SearchType,
    ) -> Result<Vec<SearchResult>, VaultError> {
        let root = self.resolve_vault(vault)?;
        let search_root = match search_path {
            Some(p) if !p.is_empty() => root.join(p),
            _ => root.to_path_buf(),
        };

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

        for entry in WalkDir::new(&search_root)
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
                    vault: vault.to_string(),
                    path: rel_path,
                    filename: filename.trim_end_matches(".md").to_string(),
                    matches: match_lines,
                });
            }
        }

        results.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(results)
    }

    pub fn add_tags(
        &self,
        vault: &str,
        files: &[String],
        tags: &[String],
        location: &str,
        normalize: bool,
        position: &str,
    ) -> Result<Vec<String>, VaultError> {
        let root = self.resolve_vault(vault)?;
        let mut modified = Vec::new();

        for file in files {
            let path = root.join(file);
            if !path.exists() {
                continue;
            }

            let content = fs::read_to_string(&path)
                .map_err(|e| VaultError::io(path.display().to_string(), e))?;

            let processed_tags: Vec<String> = tags
                .iter()
                .map(|t| {
                    if normalize {
                        normalize_tag(t)
                    } else {
                        t.clone()
                    }
                })
                .collect();

            let new_content = match location {
                "content" => add_tags_to_content(&content, &processed_tags, position),
                "frontmatter" => add_tags_to_frontmatter(&content, &processed_tags),
                _ => {
                    let with_front = add_tags_to_frontmatter(&content, &processed_tags);
                    add_tags_to_content(&with_front, &processed_tags, position)
                }
            };

            fs::write(&path, new_content)
                .map_err(|e| VaultError::io(path.display().to_string(), e))?;
            modified.push(file.clone());
        }

        Ok(modified)
    }

    pub fn remove_tags(
        &self,
        vault: &str,
        files: &[String],
        tags: &[String],
    ) -> Result<Vec<String>, VaultError> {
        let root = self.resolve_vault(vault)?;
        let mut modified = Vec::new();

        for file in files {
            let path = root.join(file);
            if !path.exists() {
                continue;
            }

            let content = fs::read_to_string(&path)
                .map_err(|e| VaultError::io(path.display().to_string(), e))?;

            let new_content = remove_tags_from_note(&content, tags);
            fs::write(&path, new_content)
                .map_err(|e| VaultError::io(path.display().to_string(), e))?;
            modified.push(file.clone());
        }

        Ok(modified)
    }

    pub fn rename_tag(
        &self,
        vault: &str,
        old_tag: &str,
        new_tag: &str,
    ) -> Result<Vec<String>, VaultError> {
        let root = self.resolve_vault(vault)?;
        let mut modified = Vec::new();

        for entry in WalkDir::new(root)
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
            let content = fs::read_to_string(path)
                .map_err(|e| VaultError::io(path.display().to_string(), e))?;

            if content_has_tag(&content, old_tag) {
                let new_content = rename_tag_in_note(&content, old_tag, new_tag);
                fs::write(path, new_content)
                    .map_err(|e| VaultError::io(path.display().to_string(), e))?;
                let rel = path
                    .strip_prefix(root)
                    .unwrap_or(path)
                    .display()
                    .to_string();
                modified.push(rel);
            }
        }

        Ok(modified)
    }
}

fn ensure_md_extension(filename: &str) -> String {
    if filename.ends_with(".md") {
        filename.to_string()
    } else {
        format!("{}.md", filename)
    }
}

fn normalize_tag(tag: &str) -> String {
    let re = Regex::new(r"[A-Z]").unwrap();
    let lower = tag.to_lowercase();
    let _ = re;
    lower
        .replace(' ', "-")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '/')
        .collect()
}

fn content_has_tag(content: &str, tag: &str) -> bool {
    let tag_lower = tag.to_lowercase();
    if let Some(fm) = extract_frontmatter(content)
        && fm.tags.iter().any(|t| t.to_lowercase() == tag_lower)
    {
        return true;
    }
    let inline_pattern = format!("#{}", tag_lower);
    content.to_lowercase().contains(&inline_pattern)
}

pub fn extract_frontmatter(content: &str) -> Option<Frontmatter> {
    if !content.starts_with("---") {
        return None;
    }
    let after = &content[3..];
    let end = after.find("\n---")?;
    let raw = &after[..end];

    let tags = parse_yaml_tags(raw);
    Some(Frontmatter {
        raw: raw.to_string(),
        tags,
    })
}

fn parse_yaml_tags(yaml: &str) -> Vec<String> {
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

fn add_tags_to_frontmatter(content: &str, tags: &[String]) -> String {
    if let Some(after) = content.strip_prefix("---") {
        if let Some(end) = after.find("\n---") {
            let fm_content = &after[..end];
            let rest = &after[end + 4..];
            let existing_tags = parse_yaml_tags(fm_content);
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

            if fm_content.contains("tags:") {
                let mut lines: Vec<String> = fm_content.lines().map(String::from).collect();
                let tag_pos = lines.iter().position(|l| l.trim().starts_with("tags:"));
                if let Some(pos) = tag_pos {
                    let tag_line = &lines[pos];
                    let is_block = tag_line.trim() == "tags:";
                    if is_block {
                        let insert_after = {
                            let mut idx = pos + 1;
                            while idx < lines.len() && lines[idx].trim().starts_with("- ") {
                                idx += 1;
                            }
                            idx
                        };
                        for tag in new_tags.iter().rev() {
                            lines.insert(insert_after, format!("  - {}", tag));
                        }
                    } else {
                        for tag in &new_tags {
                            lines.push(format!("  - {}", tag));
                        }
                    }
                    let new_fm = lines.join("\n");
                    return format!("---{}{}---\n{}", new_fm, "\n", rest.trim_start());
                }
            } else {
                let tag_block: String = new_tags
                    .iter()
                    .map(|t| format!("  - {}", t))
                    .collect::<Vec<_>>()
                    .join("\n");
                let new_fm = format!("{}\ntags:\n{}", fm_content.trim_end(), tag_block);
                return format!("---{}\n{}---\n{}", "\n", new_fm, rest.trim_start());
            }
        }
        content.to_string()
    } else {
        let tag_block: String = tags
            .iter()
            .map(|t| format!("  - {}", t))
            .collect::<Vec<_>>()
            .join("\n");
        format!("---\ntags:\n{}\n---\n{}", tag_block, content)
    }
}

fn add_tags_to_content(content: &str, tags: &[String], position: &str) -> String {
    let tag_str: String = tags.iter().map(|t| format!("#{} ", t)).collect();
    let tag_str = tag_str.trim_end();

    if position == "start" {
        if let Some(stripped) = content.strip_prefix("---")
            && let Some(end) = stripped.find("\n---")
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

fn remove_tags_from_note(content: &str, tags: &[String]) -> String {
    let tags_lower: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();
    let mut result = content.to_string();

    if result.starts_with("---")
        && let Some(end_pos) = result[3..].find("\n---")
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
        let inline = format!("#{}", tag);
        result = result
            .replace(&inline, "")
            .replace(&format!("#{} ", tag), "");
    }
    result
}

fn rename_tag_in_note(content: &str, old_tag: &str, new_tag: &str) -> String {
    let old_lower = old_tag.to_lowercase();
    let mut result = content.to_string();

    if result.starts_with("---")
        && let Some(end_pos) = result[3..].find("\n---")
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

    let inline_old = format!("#{}", old_tag);
    let inline_new = format!("#{}", new_tag);
    result.replace(&inline_old, &inline_new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_vault() -> (TempDir, VaultManager) {
        let dir = TempDir::new().unwrap();
        let manager = VaultManager::new(vec![dir.path().to_path_buf()]);
        (dir, manager)
    }

    fn vault_name(dir: &TempDir) -> String {
        dir.path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
    }

    fn write_note(dir: &TempDir, filename: &str, content: &str) {
        fs::write(dir.path().join(filename), content).unwrap();
    }

    #[test]
    fn append_adds_content_to_end() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "hello");

        let (old, new) = vault
            .edit_note(&name, "note", "append", " world", None, None)
            .unwrap();

        assert_eq!(old, "hello");
        assert_eq!(new, "hello\n world");
    }

    #[test]
    fn prepend_adds_content_to_start() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "world");

        let (old, new) = vault
            .edit_note(&name, "note", "prepend", "hello\n", None, None)
            .unwrap();

        assert_eq!(old, "world");
        assert_eq!(new, "hello\n\nworld");
    }

    #[test]
    fn replace_overwrites_entire_content() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "old content");

        let (old, new) = vault
            .edit_note(&name, "note", "replace", "new content", None, None)
            .unwrap();

        assert_eq!(old, "old content");
        assert_eq!(new, "new content");
        assert_eq!(
            fs::read_to_string(dir.path().join("note.md")).unwrap(),
            "new content"
        );
    }

    #[test]
    fn find_and_replace_substitutes_first_occurrence() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "foo bar foo");

        let (old, new) = vault
            .edit_note(&name, "note", "find_and_replace", "baz", None, Some("foo"))
            .unwrap();

        assert_eq!(old, "foo bar foo");
        assert_eq!(new, "baz bar foo");
        assert_eq!(
            fs::read_to_string(dir.path().join("note.md")).unwrap(),
            "baz bar foo"
        );
    }

    #[test]
    fn find_and_replace_returns_error_when_search_text_not_found() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "hello world");

        let result = vault.edit_note(
            &name,
            "note",
            "find_and_replace",
            "replacement",
            None,
            Some("missing"),
        );

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Search text not found")
        );
    }

    #[test]
    fn find_and_replace_returns_error_when_search_param_missing() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "hello");

        let result = vault.edit_note(&name, "note", "find_and_replace", "replacement", None, None);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("search"));
    }

    #[test]
    fn unknown_operation_returns_error() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "hello");

        let result = vault.edit_note(&name, "note", "invalid_op", "content", None, None);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Unknown operation")
        );
    }

    #[test]
    fn edit_returns_error_for_missing_note() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);

        let result = vault.edit_note(&name, "nonexistent", "append", "data", None, None);

        assert!(result.is_err());
    }

    #[test]
    fn append_persists_to_disk() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "line1");

        vault
            .edit_note(&name, "note", "append", "line2", None, None)
            .unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join("note.md")).unwrap(),
            "line1\nline2"
        );
    }

    // ── VaultManager basics ───────────────────────────────────────────────────

    #[test]
    fn list_vaults_returns_sorted() {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();
        let manager = VaultManager::new(vec![dir2.path().to_path_buf(), dir1.path().to_path_buf()]);
        let vaults = manager.list_vaults();
        assert!(!vaults.is_empty());
        // sorted
        let names: Vec<_> = vaults.iter().map(|(n, _)| n.clone()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn resolve_vault_not_found_error() {
        let (_, vault) = make_vault();
        let err = vault.resolve_vault("nonexistent").unwrap_err();
        assert!(err.to_string().contains("nonexistent"));
    }

    #[test]
    fn note_path_adds_md_extension() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let p = vault.note_path(&name, "note", None).unwrap();
        assert!(p.to_str().unwrap().ends_with("note.md"));
    }

    #[test]
    fn note_path_keeps_md_extension() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let p = vault.note_path(&name, "note.md", None).unwrap();
        assert!(p.to_str().unwrap().ends_with("note.md"));
    }

    #[test]
    fn note_path_with_folder() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let p = vault.note_path(&name, "note", Some("sub")).unwrap();
        assert!(p.to_str().unwrap().contains("sub"));
        assert!(p.to_str().unwrap().ends_with("note.md"));
    }

    #[test]
    fn note_path_empty_folder_ignores_folder() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let with_empty = vault.note_path(&name, "note", Some("")).unwrap();
        let without = vault.note_path(&name, "note", None).unwrap();
        assert_eq!(with_empty, without);
    }

    // ── create_note ───────────────────────────────────────────────────────────

    #[test]
    fn create_note_writes_content() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        vault
            .create_note(&name, "new", "hello world", None)
            .unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("new.md")).unwrap(),
            "hello world"
        );
    }

    #[test]
    fn create_note_returns_path() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let p = vault.create_note(&name, "new", "", None).unwrap();
        assert!(p.exists());
    }

    #[test]
    fn create_note_creates_parent_directories() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        vault
            .create_note(&name, "deep", "content", Some("a/b/c"))
            .unwrap();
        assert!(dir.path().join("a/b/c/deep.md").exists());
    }

    #[test]
    fn create_note_errors_if_already_exists() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "exists.md", "");
        let err = vault
            .create_note(&name, "exists", "content", None)
            .unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    // ── read_note ─────────────────────────────────────────────────────────────

    #[test]
    fn read_note_returns_content() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "hello");
        assert_eq!(vault.read_note(&name, "note", None).unwrap(), "hello");
    }

    #[test]
    fn read_note_accepts_md_extension() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "content");
        assert_eq!(vault.read_note(&name, "note.md", None).unwrap(), "content");
    }

    #[test]
    fn read_note_error_if_not_found() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        assert!(vault.read_note(&name, "ghost", None).is_err());
    }

    #[test]
    fn read_note_in_subfolder() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/note.md"), "deep").unwrap();
        assert_eq!(vault.read_note(&name, "note", Some("sub")).unwrap(), "deep");
    }

    // ── delete_note ───────────────────────────────────────────────────────────

    #[test]
    fn delete_note_removes_file() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "del.md", "bye");
        vault.delete_note(&name, "del", None).unwrap();
        assert!(!dir.path().join("del.md").exists());
    }

    #[test]
    fn delete_note_error_if_not_found() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        assert!(vault.delete_note(&name, "ghost", None).is_err());
    }

    // ── move_note ─────────────────────────────────────────────────────────────

    #[test]
    fn move_note_renames_file() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "original.md", "body");
        let dest = vault
            .move_note(&name, "original", None, None, Some("renamed"))
            .unwrap();
        assert!(dest.exists());
        assert!(!dir.path().join("original.md").exists());
    }

    #[test]
    fn move_note_to_subfolder() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "body");
        vault
            .move_note(&name, "note", None, Some("sub"), None)
            .unwrap();
        assert!(dir.path().join("sub/note.md").exists());
    }

    #[test]
    fn move_note_error_if_not_found() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        assert!(vault.move_note(&name, "ghost", None, None, None).is_err());
    }

    // ── create_directory ──────────────────────────────────────────────────────

    #[test]
    fn create_directory_recursive() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        vault.create_directory(&name, "a/b/c", true).unwrap();
        assert!(dir.path().join("a/b/c").is_dir());
    }

    #[test]
    fn create_directory_non_recursive() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        vault.create_directory(&name, "flat", false).unwrap();
        assert!(dir.path().join("flat").is_dir());
    }

    #[test]
    fn create_directory_error_if_already_exists() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        fs::create_dir_all(dir.path().join("existing")).unwrap();
        assert!(vault.create_directory(&name, "existing", true).is_err());
    }

    // ── search_vault ──────────────────────────────────────────────────────────

    #[test]
    fn search_content_finds_matching_lines() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "a.md", "the quick brown fox");
        write_note(&dir, "b.md", "no match here");
        let results = vault
            .search_vault(&name, "quick", None, false, &SearchType::Content)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].filename, "a");
    }

    #[test]
    fn search_filename_finds_by_name() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "journal_2024.md", "");
        write_note(&dir, "other.md", "");
        let results = vault
            .search_vault(&name, "journal", None, false, &SearchType::Filename)
            .unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_both_finds_in_filename_and_content() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "target.md", "nothing special");
        write_note(&dir, "other.md", "has target word inside");
        let results = vault
            .search_vault(&name, "target", None, false, &SearchType::Both)
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn search_tag_finds_frontmatter_tag() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "tagged.md", "---\ntags:\n  - work\n---\ncontent");
        write_note(&dir, "other.md", "no tags");
        let results = vault
            .search_vault(&name, "tag:work", None, false, &SearchType::Content)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].filename, "tagged");
    }

    #[test]
    fn search_tag_finds_inline_tag() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "inline.md", "some text #urgent here");
        let results = vault
            .search_vault(&name, "tag:urgent", None, false, &SearchType::Content)
            .unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_case_sensitive() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "Hello World");
        let insensitive = vault
            .search_vault(&name, "hello", None, false, &SearchType::Content)
            .unwrap();
        let sensitive = vault
            .search_vault(&name, "hello", None, true, &SearchType::Content)
            .unwrap();
        assert_eq!(insensitive.len(), 1);
        assert_eq!(sensitive.len(), 0);
    }

    #[test]
    fn search_with_path_limit() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/inner.md"), "needle").unwrap();
        write_note(&dir, "root.md", "needle");
        let results = vault
            .search_vault(&name, "needle", Some("sub"), false, &SearchType::Content)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].filename, "inner");
    }

    #[test]
    fn search_no_results() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "content");
        let results = vault
            .search_vault(&name, "zzz_not_here", None, false, &SearchType::Content)
            .unwrap();
        assert!(results.is_empty());
    }

    // ── add_tags ──────────────────────────────────────────────────────────────

    #[test]
    fn add_tags_to_frontmatter_block_list() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "---\ntags:\n  - existing\n---\nbody");
        vault
            .add_tags(
                &name,
                &["note.md".into()],
                &["new-tag".into()],
                "frontmatter",
                false,
                "end",
            )
            .unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(content.contains("new-tag"));
        assert!(content.contains("existing"));
    }

    #[test]
    fn add_tags_creates_frontmatter_if_absent() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "plain.md", "just content");
        vault
            .add_tags(
                &name,
                &["plain.md".into()],
                &["fresh".into()],
                "frontmatter",
                false,
                "end",
            )
            .unwrap();
        let content = fs::read_to_string(dir.path().join("plain.md")).unwrap();
        assert!(content.starts_with("---"));
        assert!(content.contains("fresh"));
    }

    #[test]
    fn add_tags_to_content_end() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "body text");
        vault
            .add_tags(
                &name,
                &["note.md".into()],
                &["inline".into()],
                "content",
                false,
                "end",
            )
            .unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(content.ends_with("#inline"));
    }

    #[test]
    fn add_tags_to_content_start() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "body");
        vault
            .add_tags(
                &name,
                &["note.md".into()],
                &["first".into()],
                "content",
                false,
                "start",
            )
            .unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(content.contains("#first"));
    }

    #[test]
    fn add_tags_both_location() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "body");
        vault
            .add_tags(
                &name,
                &["note.md".into()],
                &["mytag".into()],
                "both",
                false,
                "end",
            )
            .unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(content.contains("mytag")); // in frontmatter
        assert!(content.contains("#mytag")); // inline
    }

    #[test]
    fn add_tags_skips_missing_files() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let modified = vault
            .add_tags(
                &name,
                &["ghost.md".into()],
                &["tag".into()],
                "frontmatter",
                false,
                "end",
            )
            .unwrap();
        assert!(modified.is_empty());
    }

    #[test]
    fn add_tags_normalizes() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "content");
        vault
            .add_tags(
                &name,
                &["note.md".into()],
                &["My Tag".into()],
                "frontmatter",
                true,
                "end",
            )
            .unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(content.contains("my-tag"));
    }

    #[test]
    fn add_tags_deduplicates() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "---\ntags:\n  - existing\n---\n");
        vault
            .add_tags(
                &name,
                &["note.md".into()],
                &["existing".into()],
                "frontmatter",
                false,
                "end",
            )
            .unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        // should still only appear once
        assert_eq!(content.matches("existing").count(), 1);
    }

    #[test]
    fn add_tags_to_frontmatter_inline_tag_line() {
        // covers the inline (non-block) tags: branch in add_tags_to_frontmatter
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "---\ntags: [existing]\n---\nbody");
        vault
            .add_tags(
                &name,
                &["note.md".into()],
                &["added".into()],
                "frontmatter",
                false,
                "end",
            )
            .unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(content.contains("added"));
    }

    #[test]
    fn add_tags_to_content_start_with_frontmatter() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "---\ntitle: test\n---\nbody text");
        vault
            .add_tags(
                &name,
                &["note.md".into()],
                &["top".into()],
                "content",
                false,
                "start",
            )
            .unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(content.contains("#top"));
    }

    // ── remove_tags ───────────────────────────────────────────────────────────

    #[test]
    fn remove_tags_from_frontmatter() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(
            &dir,
            "note.md",
            "---\ntags:\n  - keep\n  - remove\n---\nbody",
        );
        vault
            .remove_tags(&name, &["note.md".into()], &["remove".into()])
            .unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(!content.contains("  - remove"));
        assert!(content.contains("keep"));
    }

    #[test]
    fn remove_tags_from_content() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "text #remove more");
        vault
            .remove_tags(&name, &["note.md".into()], &["remove".into()])
            .unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(!content.contains("#remove"));
    }

    #[test]
    fn remove_tags_skips_missing_files() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let modified = vault
            .remove_tags(&name, &["ghost.md".into()], &["t".into()])
            .unwrap();
        assert!(modified.is_empty());
    }

    // ── rename_tag ────────────────────────────────────────────────────────────

    #[test]
    fn rename_tag_in_frontmatter() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "---\ntags:\n  - old\n---\n");
        let modified = vault.rename_tag(&name, "old", "new").unwrap();
        assert_eq!(modified.len(), 1);
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(content.contains("- new"));
        assert!(!content.contains("- old"));
    }

    #[test]
    fn rename_tag_inline() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "text #old-tag more");
        vault.rename_tag(&name, "old-tag", "new-tag").unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(content.contains("#new-tag"));
        assert!(!content.contains("#old-tag"));
    }

    #[test]
    fn rename_tag_no_matches() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "no tags here");
        let modified = vault.rename_tag(&name, "absent", "new").unwrap();
        assert!(modified.is_empty());
    }

    // ── private helpers ───────────────────────────────────────────────────────

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
    fn extract_frontmatter_parses_single_value() {
        let fm = extract_frontmatter("---\ntags: solo\n---\n").unwrap();
        assert_eq!(fm.tags, vec!["solo"]);
    }

    #[test]
    fn normalize_tag_lowercases_and_hyphenates() {
        assert_eq!(normalize_tag("My Tag"), "my-tag");
        assert_eq!(normalize_tag("Hello World"), "hello-world");
        assert_eq!(normalize_tag("simple"), "simple");
    }

    #[test]
    fn ensure_md_adds_extension() {
        assert_eq!(ensure_md_extension("note"), "note.md");
        assert_eq!(ensure_md_extension("note.md"), "note.md");
    }
}
