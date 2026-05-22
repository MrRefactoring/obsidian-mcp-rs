use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::error::VaultError;

/// Reject paths that would escape the vault root.
///
/// Defends against:
/// - absolute paths supplied as `folder` or `filename`
/// - `..` traversal that climbs above the vault
/// - symlinks inside the vault that point outside it
///
/// The path may refer to a not-yet-existing file. We canonicalize the deepest
/// existing ancestor and require it to live under the canonicalized vault root.
pub(crate) fn safe_join(
    root: &Path,
    folder: Option<&str>,
    filename: &str,
) -> Result<PathBuf, VaultError> {
    if Path::new(filename).is_absolute() {
        return Err(VaultError::InvalidPath(format!(
            "absolute filename not allowed: '{}'",
            filename
        )));
    }
    if let Some(f) = folder
        && Path::new(f).is_absolute()
    {
        return Err(VaultError::InvalidPath(format!(
            "absolute folder not allowed: '{}'",
            f
        )));
    }

    let joined = match folder {
        Some(f) if !f.is_empty() => root.join(f).join(filename),
        _ => root.join(filename),
    };

    let canon_root =
        fs::canonicalize(root).map_err(|e| VaultError::io(root.display().to_string(), e))?;

    let mut probe: &Path = &joined;
    let canon_anchor = loop {
        if probe.exists() {
            break fs::canonicalize(probe)
                .map_err(|e| VaultError::io(probe.display().to_string(), e))?;
        }
        match probe.parent() {
            Some(parent) => probe = parent,
            None => {
                return Err(VaultError::InvalidPath(format!(
                    "path has no existing ancestor: '{}'",
                    joined.display()
                )));
            }
        }
    };

    if !canon_anchor.starts_with(&canon_root) {
        return Err(VaultError::InvalidPath(format!(
            "path '{}' escapes vault root '{}'",
            joined.display(),
            root.display()
        )));
    }

    Ok(joined)
}

pub(crate) fn ensure_md_extension(filename: &str) -> String {
    if filename.ends_with(".md") {
        filename.to_string()
    } else {
        format!("{}.md", filename)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_md_adds_extension() {
        assert_eq!(ensure_md_extension("note"), "note.md");
        assert_eq!(ensure_md_extension("note.md"), "note.md");
    }
}
