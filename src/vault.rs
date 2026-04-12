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
            let available = self
                .vaults
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
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
        let old = fs::read_to_string(&path).map_err(|e| VaultError::io(path.display().to_string(), e))?;
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
            return Err(VaultError::DirectoryAlreadyExists(dir.display().to_string()));
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
                if let Some(tag) = tag_value {
                    if let Ok(content) = fs::read_to_string(path) {
                        if content_has_tag(&content, tag) {
                            match_lines.push(format!("tag: {}", tag));
                        }
                    }
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

                if matches!(search_type, SearchType::Content | SearchType::Both) {
                    if let Ok(content) = fs::read_to_string(path) {
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
            let path = if file.contains('/') || file.contains('\\') {
                root.join(file)
            } else {
                root.join(file)
            };
            if !path.exists() {
                continue;
            }

            let content =
                fs::read_to_string(&path).map_err(|e| VaultError::io(path.display().to_string(), e))?;

            let processed_tags: Vec<String> = tags
                .iter()
                .map(|t| if normalize { normalize_tag(t) } else { t.clone() })
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

            let content =
                fs::read_to_string(&path).map_err(|e| VaultError::io(path.display().to_string(), e))?;

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
            let content =
                fs::read_to_string(path).map_err(|e| VaultError::io(path.display().to_string(), e))?;

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
    if let Some(fm) = extract_frontmatter(content) {
        if fm.tags.iter().any(|t| t.to_lowercase() == tag_lower) {
            return true;
        }
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
    if content.starts_with("---") {
        let after = &content[3..];
        if let Some(end) = after.find("\n---") {
            let fm_content = &after[..end];
            let rest = &after[end + 4..];
            let existing_tags = parse_yaml_tags(fm_content);
            let new_tags: Vec<&String> = tags
                .iter()
                .filter(|t| !existing_tags.iter().any(|e| e.to_lowercase() == t.to_lowercase()))
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
        if content.starts_with("---") {
            if let Some(end) = content[3..].find("\n---") {
                let fm_end = 3 + end + 4;
                let after_fm = &content[fm_end..].trim_start();
                return format!("{}\n{}\n{}", &content[..fm_end], tag_str, after_fm);
            }
        }
        format!("{}\n{}", tag_str, content)
    } else {
        format!("{}\n{}", content.trim_end(), tag_str)
    }
}

fn remove_tags_from_note(content: &str, tags: &[String]) -> String {
    let tags_lower: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();
    let mut result = content.to_string();

    if result.starts_with("---") {
        if let Some(end_pos) = result[3..].find("\n---") {
            let fm_end = 3 + end_pos;
            let fm_content = &result[3..fm_end].to_string();
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
    }

    for tag in &tags_lower {
        let inline = format!("#{}", tag);
        result = result.replace(&inline, "").replace(&format!("#{} ", tag), "");
    }
    result
}

fn rename_tag_in_note(content: &str, old_tag: &str, new_tag: &str) -> String {
    let old_lower = old_tag.to_lowercase();
    let mut result = content.to_string();

    if result.starts_with("---") {
        if let Some(end_pos) = result[3..].find("\n---") {
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

        let (old, new) = vault.edit_note(&name, "note", "append", " world", None, None).unwrap();

        assert_eq!(old, "hello");
        assert_eq!(new, "hello\n world");
    }

    #[test]
    fn prepend_adds_content_to_start() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "world");

        let (old, new) = vault.edit_note(&name, "note", "prepend", "hello\n", None, None).unwrap();

        assert_eq!(old, "world");
        assert_eq!(new, "hello\n\nworld");
    }

    #[test]
    fn replace_overwrites_entire_content() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "old content");

        let (old, new) = vault.edit_note(&name, "note", "replace", "new content", None, None).unwrap();

        assert_eq!(old, "old content");
        assert_eq!(new, "new content");
        assert_eq!(fs::read_to_string(dir.path().join("note.md")).unwrap(), "new content");
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
        assert_eq!(fs::read_to_string(dir.path().join("note.md")).unwrap(), "baz bar foo");
    }

    #[test]
    fn find_and_replace_returns_error_when_search_text_not_found() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "hello world");

        let result = vault.edit_note(&name, "note", "find_and_replace", "replacement", None, Some("missing"));

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Search text not found"));
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
        assert!(result.unwrap_err().to_string().contains("Unknown operation"));
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

        vault.edit_note(&name, "note", "append", "line2", None, None).unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join("note.md")).unwrap(),
            "line1\nline2"
        );
    }
}
