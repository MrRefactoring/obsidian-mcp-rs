use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

/// Collect every `.md` file under `root`.
///
/// Backed by `ignore::WalkBuilder`, so `.gitignore` rules and hidden files are
/// respected — a vault that gitignores `.trash/` or templates won't surface
/// those notes. `follow_links(false)` keeps the walk inside the vault, matching
/// the sandbox guarantee of `safe_join` (symlinks are never expanded).
pub(crate) fn md_files(root: &Path) -> Vec<PathBuf> {
    WalkBuilder::new(root)
        .follow_links(false)
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .map(|e| e.into_path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn hidden_directories_are_not_walked() {
        // Load-bearing for `delete-note`: a trashed note lives in `.trash/`, and
        // must not come back in search results or the link graph.
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join(".trash")).unwrap();
        fs::write(dir.path().join(".trash/gone.md"), "trashed").unwrap();
        fs::write(dir.path().join("live.md"), "kept").unwrap();

        let found: Vec<String> = md_files(dir.path())
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();

        assert_eq!(found, vec!["live.md"], "a trashed note must stay invisible");
    }
}
