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
