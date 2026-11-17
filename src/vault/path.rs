use std::{
    fs, io,
    path::{Component, Path, PathBuf},
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
///
/// TOCTOU limitation: the returned path is lexical, not canonicalized. The
/// caller's filesystem operation resolves symbolic links again. A symbolic link
/// swapped between this check and that operation could still escape. Winning that race requires write access to the
/// vault directory, which already defeats the sandbox for a local single-user
/// tool — so this is out of the threat model rather than closed with
/// `openat`-style (`RESOLVE_BENEATH`) APIs.
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
    let escapes =
        |_| VaultError::InvalidPath(format!("path '{}' escapes vault root", joined.display()));
    let relative = joined.strip_prefix(root).map_err(escapes)?;
    normalize_relative(relative).map_err(|_| {
        VaultError::InvalidPath(format!("path '{}' escapes vault root", joined.display()))
    })?;

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

pub(crate) fn normalize_relative(path: &Path) -> Result<PathBuf, VaultError> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(name) => normalized.push(name),
            Component::CurDir => {}
            Component::ParentDir if normalized.pop() => {}
            _ => return Err(VaultError::InvalidPath(path.display().to_string())),
        }
    }
    Ok(normalized)
}

pub(crate) fn projected_path(path: &Path) -> Result<PathBuf, VaultError> {
    let mut ancestor = path;
    loop {
        match fs::canonicalize(ancestor) {
            Ok(canonical) => {
                let tail = path
                    .strip_prefix(ancestor)
                    .map_err(|_| VaultError::InvalidPath(path.display().to_string()))?;
                let tail = normalize_relative(tail)?;
                return Ok(if tail.as_os_str().is_empty() {
                    canonical
                } else {
                    canonical.join(tail)
                });
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                ancestor = ancestor
                    .parent()
                    .ok_or_else(|| VaultError::io(path.display().to_string(), e))?;
            }
            Err(e) => return Err(VaultError::io(path.display().to_string(), e)),
        }
    }
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
