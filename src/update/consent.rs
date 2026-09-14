use std::path::PathBuf;

use anyhow::{Context, Result};

const MARKER: &str = "no-auto-update";

pub fn marker_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("obsidian-mcp-rs").join(MARKER))
}

pub fn is_enabled() -> bool {
    marker_path().is_none_or(|p| enabled_at(&p))
}

fn enabled_at(marker: &std::path::Path) -> bool {
    !marker.exists()
}

pub fn set(enabled: bool) -> Result<()> {
    let Some(path) = marker_path() else {
        return Ok(());
    };
    set_at(&path, enabled)
}

fn set_at(path: &std::path::Path, enabled: bool) -> Result<()> {
    if enabled {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(e).with_context(|| format!("could not remove {}", path.display()))
            }
            _ => Ok(()),
        };
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    std::fs::write(path, b"").with_context(|| format!("could not write {}", path.display()))
}

pub fn forget() {
    if let Some(path) = marker_path() {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_marker_sits_beside_the_installed_binary_not_inside_a_vault() {
        let (Some(marker), Some(exe)) = (marker_path(), crate::install::binary::stable_path())
        else {
            return;
        };
        assert_eq!(marker.parent(), exe.parent().and_then(|p| p.parent()));
        assert!(dirs::data_local_dir().is_some_and(|d| marker.starts_with(d)));
    }

    #[test]
    fn absent_means_on_because_that_is_the_default_we_ship() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("no-auto-update");
        assert!(enabled_at(&marker));
    }

    #[test]
    fn opting_out_is_recorded_and_can_be_taken_back() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("nested").join("no-auto-update");

        set_at(&marker, false).unwrap();
        assert!(marker.exists());
        assert!(!enabled_at(&marker));

        set_at(&marker, true).unwrap();
        assert!(!marker.exists());
        assert!(enabled_at(&marker));
    }

    #[test]
    fn opting_back_in_twice_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("no-auto-update");
        set_at(&marker, true).unwrap();
        set_at(&marker, true).unwrap();
        assert!(enabled_at(&marker));
    }
}
