mod frontmatter;
mod path;
mod search;
mod tags;
mod walk;

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use rayon::prelude::*;

use crate::error::VaultError;

use frontmatter::content_has_tag;
use path::{ensure_md_extension, safe_join};
use tags::{
    add_tags_to_content, add_tags_to_frontmatter, normalize_tag, remove_tags_from_note,
    rename_tag_in_note,
};
use walk::md_files;

pub use search::{SearchResult, SearchType};

#[derive(Debug, Clone)]
pub struct VaultManager {
    vaults: HashMap<String, PathBuf>,
}

impl VaultManager {
    pub fn new(vault_paths: Vec<PathBuf>) -> Self {
        let mut vaults = HashMap::new();
        for path in vault_paths {
            let base = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("vault")
                .to_string();
            // Disambiguate when the basename collides — `~/work/notes` and
            // `~/personal/notes` would otherwise shadow each other silently.
            let name = if vaults.contains_key(&base) {
                let mut n = 2;
                loop {
                    let candidate = format!("{base}-{n}");
                    if !vaults.contains_key(&candidate) {
                        tracing::warn!(
                            vault = %candidate,
                            original = %base,
                            path = %path.display(),
                            "vault basename collision — registered under disambiguated name"
                        );
                        break candidate;
                    }
                    n += 1;
                }
            } else {
                base
            };
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
        safe_join(root, folder, &filename)
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
        let root = self.resolve_vault(vault)?.to_path_buf();
        let path = self.note_path(vault, filename, folder)?;
        if !path.exists() {
            return Err(VaultError::NoteNotFound(
                path.display().to_string(),
                vault.to_string(),
            ));
        }
        fs::remove_file(&path).map_err(|e| VaultError::io(path.display().to_string(), e))?;
        prune_empty_parent(&path, &root);
        Ok(())
    }

    pub fn move_note(
        &self,
        vault: &str,
        filename: &str,
        folder: Option<&str>,
        new_folder: Option<&str>,
        new_filename: Option<&str>,
    ) -> Result<PathBuf, VaultError> {
        let root = self.resolve_vault(vault)?.to_path_buf();
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
        prune_empty_parent(&src, &root);
        Ok(dest)
    }

    pub fn create_directory(
        &self,
        vault: &str,
        path: &str,
        recursive: bool,
    ) -> Result<PathBuf, VaultError> {
        let root = self.resolve_vault(vault)?;
        let dir = safe_join(root, None, path)?;
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
            Some(p) if !p.is_empty() => safe_join(root, None, p)?,
            _ => root.to_path_buf(),
        };
        Ok(search::search(
            root,
            &search_root,
            query,
            case_sensitive,
            search_type,
        ))
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
            let path = safe_join(root, None, file)?;
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
            let path = safe_join(root, None, file)?;
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

        let mut modified: Vec<String> = md_files(root)
            .par_iter()
            .map(|path| -> Result<Option<String>, VaultError> {
                let content = fs::read_to_string(path)
                    .map_err(|e| VaultError::io(path.display().to_string(), e))?;
                if !content_has_tag(&content, old_tag) {
                    return Ok(None);
                }
                let new_content = rename_tag_in_note(&content, old_tag, new_tag);
                fs::write(path, new_content)
                    .map_err(|e| VaultError::io(path.display().to_string(), e))?;
                let rel = path
                    .strip_prefix(root)
                    .unwrap_or(path)
                    .display()
                    .to_string();
                Ok(Some(rel))
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();
        modified.sort();

        Ok(modified)
    }
}

/// Remove `note`'s parent directory if the just-completed operation left it
/// empty — but never the vault `root`. Best-effort: a failed cleanup is logged,
/// not propagated, so it can't fail the move/delete that triggered it.
fn prune_empty_parent(note: &Path, root: &Path) {
    if let Some(parent) = note.parent()
        && parent != root
        && fs::read_dir(parent).is_ok_and(|mut d| d.next().is_none())
        && let Err(e) = fs::remove_dir(parent)
    {
        tracing::warn!(dir = %parent.display(), error = %e, "failed to remove emptied source folder");
    }
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
    fn vault_basename_collisions_are_disambiguated() {
        use std::fs;
        let parent_a = TempDir::new().unwrap();
        let parent_b = TempDir::new().unwrap();
        fs::create_dir(parent_a.path().join("notes")).unwrap();
        fs::create_dir(parent_b.path().join("notes")).unwrap();
        let manager = VaultManager::new(vec![
            parent_a.path().join("notes"),
            parent_b.path().join("notes"),
        ]);
        let names: Vec<String> = manager.list_vaults().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names.len(), 2, "both vaults must be registered: {names:?}");
        assert!(names.iter().any(|n| n == "notes"));
        assert!(names.iter().any(|n| n == "notes-2"));
    }

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

    #[test]
    fn delete_note_removes_emptied_source_folder() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/note.md"), "body").unwrap();
        vault.delete_note(&name, "note", Some("sub")).unwrap();
        assert!(
            !dir.path().join("sub").exists(),
            "emptied source folder must be removed"
        );
    }

    #[test]
    fn delete_note_keeps_nonempty_source_folder() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/a.md"), "a").unwrap();
        fs::write(dir.path().join("sub/b.md"), "b").unwrap();
        vault.delete_note(&name, "a", Some("sub")).unwrap();
        assert!(
            dir.path().join("sub").exists(),
            "source folder still has b.md and must stay"
        );
        assert!(dir.path().join("sub/b.md").exists());
    }

    #[test]
    fn delete_note_does_not_remove_vault_root() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "only.md", "body");
        vault.delete_note(&name, "only", None).unwrap();
        // The note lived directly in the vault root; the root must never be
        // pruned even though it is now empty of notes.
        assert!(dir.path().exists());
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

    #[test]
    fn move_note_removes_emptied_source_folder() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/note.md"), "body").unwrap();
        vault
            .move_note(&name, "note", Some("src"), Some("dst"), None)
            .unwrap();
        assert!(dir.path().join("dst/note.md").exists());
        assert!(
            !dir.path().join("src").exists(),
            "emptied source folder must be removed"
        );
    }

    #[test]
    fn move_note_keeps_nonempty_source_folder() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/a.md"), "a").unwrap();
        fs::write(dir.path().join("src/b.md"), "b").unwrap();
        vault
            .move_note(&name, "a", Some("src"), Some("dst"), None)
            .unwrap();
        assert!(
            dir.path().join("src").exists(),
            "source folder still has b.md and must stay"
        );
        assert!(dir.path().join("src/b.md").exists());
    }

    #[test]
    fn move_note_does_not_remove_vault_root() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "only.md", "body");
        vault
            .move_note(&name, "only", None, Some("sub"), None)
            .unwrap();
        // The note lived directly in the vault root; the root must never be
        // pruned even though it is now empty of notes.
        assert!(dir.path().exists());
        assert!(dir.path().join("sub/only.md").exists());
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
    fn rename_tag_does_not_corrupt_overlapping_inline_tags() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "text #foo and #foobar and #foo-extra end");
        vault.rename_tag(&name, "foo", "bar").unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        // exact #foo replaced; #foobar and #foo-extra preserved
        assert!(
            content.contains("text #bar and #foobar and #foo-extra end"),
            "got: {content:?}"
        );
    }

    #[test]
    fn remove_tags_does_not_corrupt_overlapping_inline_tags() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "#foo #foobar #foo-extra done");
        vault
            .remove_tags(&name, &["note.md".into()], &["foo".into()])
            .unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(
            !content.contains("#foo "),
            "exact #foo must be gone: {content:?}"
        );
        assert!(content.contains("#foobar"), "got: {content:?}");
        assert!(content.contains("#foo-extra"), "got: {content:?}");
    }

    #[test]
    fn rename_tag_no_matches() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "no tags here");
        let modified = vault.rename_tag(&name, "absent", "new").unwrap();
        assert!(modified.is_empty());
    }

    // ── Path traversal / sandboxing ───────────────────────────────────────────

    #[test]
    fn rejects_parent_traversal_in_filename() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let result = vault.note_path(&name, "../escaped", None);
        assert!(result.is_err(), "expected error, got {:?}", result);
        assert!(
            result.unwrap_err().to_string().contains("escapes vault"),
            "wrong error message"
        );
    }

    #[test]
    fn rejects_parent_traversal_in_folder() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let result = vault.note_path(&name, "note", Some("../.."));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("escapes vault"));
    }

    #[test]
    fn rejects_absolute_filename() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let result = vault.note_path(&name, "/etc/passwd", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("absolute"));
    }

    #[test]
    fn rejects_absolute_folder() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let result = vault.note_path(&name, "note", Some("/tmp"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("absolute"));
    }

    #[test]
    fn allows_normal_nested_path() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let p = vault
            .note_path(&name, "note", Some("subdir/inner"))
            .unwrap();
        assert!(p.starts_with(dir.path()));
        assert!(p.ends_with("note.md"));
    }

    #[test]
    fn create_note_blocks_traversal() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let result = vault.create_note(&name, "../pwned", "x", None);
        assert!(result.is_err());
        // file must not have been created outside the vault
        assert!(!dir.path().parent().unwrap().join("pwned.md").exists());
    }

    #[test]
    fn delete_note_blocks_traversal() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        // create a sibling file outside the vault
        let outside = dir.path().parent().unwrap().join("sibling.md");
        fs::write(&outside, "private").unwrap();
        let result = vault.delete_note(&name, "../sibling", None);
        assert!(result.is_err());
        assert!(outside.exists(), "outside file must not be deleted");
        fs::remove_file(&outside).ok();
    }

    #[test]
    fn create_directory_blocks_traversal() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let result = vault.create_directory(&name, "../escaped", true);
        assert!(result.is_err());
    }

    #[test]
    fn search_vault_blocks_traversal_in_path() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let result = vault.search_vault(&name, "x", Some("../.."), false, &SearchType::Content);
        assert!(result.is_err());
    }

    #[test]
    fn add_tags_blocks_traversal() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        // create a sibling file outside the vault that must not be touched
        let outside = dir.path().parent().unwrap().join("sibling-add.md");
        fs::write(&outside, "untouched").unwrap();
        let result = vault.add_tags(
            &name,
            &["../sibling-add.md".into()],
            &["pwned".into()],
            "frontmatter",
            false,
            "end",
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("escapes vault"));
        // outside file must be byte-identical
        assert_eq!(fs::read_to_string(&outside).unwrap(), "untouched");
        fs::remove_file(&outside).ok();
    }

    #[test]
    fn add_tags_blocks_absolute_path() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let result = vault.add_tags(
            &name,
            &["/etc/hosts".into()],
            &["x".into()],
            "frontmatter",
            false,
            "end",
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("absolute"));
    }

    #[test]
    fn remove_tags_blocks_traversal() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let outside = dir.path().parent().unwrap().join("sibling-rm.md");
        fs::write(&outside, "#keep").unwrap();
        let result = vault.remove_tags(&name, &["../sibling-rm.md".into()], &["keep".into()]);
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&outside).unwrap(), "#keep");
        fs::remove_file(&outside).ok();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        use std::os::unix::fs::symlink;
        let outer = TempDir::new().unwrap();
        let secret = outer.path().join("secret.md");
        fs::write(&secret, "top secret").unwrap();

        let (vault_dir, vault) = make_vault();
        let name = vault_name(&vault_dir);
        // symlink inside the vault pointing to a directory outside
        symlink(outer.path(), vault_dir.path().join("escape")).unwrap();

        let result = vault.read_note(&name, "secret", Some("escape"));
        assert!(
            result.is_err(),
            "symlink escape must be rejected, got {:?}",
            result
        );
        assert!(result.unwrap_err().to_string().contains("escapes vault"));
    }

    #[cfg(unix)]
    #[test]
    fn allows_symlink_staying_inside_vault() {
        use std::os::unix::fs::symlink;
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        fs::create_dir(dir.path().join("real")).unwrap();
        fs::write(dir.path().join("real/note.md"), "hello").unwrap();
        symlink(dir.path().join("real"), dir.path().join("link")).unwrap();

        let content = vault.read_note(&name, "note", Some("link")).unwrap();
        assert_eq!(content, "hello");
    }
}
