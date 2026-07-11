use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::error::VaultError;

/// Monotonic counter making temp filenames unique within this process; combined
/// with the PID it stays unique across concurrent processes too.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Write `contents` to `path` atomically.
///
/// Writes to a temp file in the *same directory*, flushes it, then `rename`s it
/// over the target. A crash or concurrent write mid-operation leaves the
/// original note fully intact (or absent) — never a half-written file. The temp
/// lives beside the target so the final `rename` stays on one filesystem
/// (cross-device renames aren't atomic and would fail).
pub(crate) fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), VaultError> {
    let tmp = temp_path(path);

    // Scoped so the file handle is dropped (flushed + closed) before the rename.
    {
        let mut file =
            fs::File::create(&tmp).map_err(|e| VaultError::io(tmp.display().to_string(), e))?;
        if let Err(e) = file.write_all(contents).and_then(|()| file.flush()) {
            let _ = fs::remove_file(&tmp); // best-effort cleanup; keep original error
            return Err(VaultError::io(tmp.display().to_string(), e));
        }
    }

    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        VaultError::io(path.display().to_string(), e)
    })
}

/// A unique sibling temp path for `target`, e.g. `.note.md.4213.7.tmp`.
fn temp_path(target: &Path) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let stem = target
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("note");
    let name = format!(".{stem}.{pid}.{n}.tmp");
    match target.parent() {
        Some(parent) => parent.join(name),
        None => PathBuf::from(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_full_contents_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("note.md");

        atomic_write(&target, b"hello world").unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "hello world");
        // No leftover temp files in the directory.
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp file was left behind");
    }

    #[test]
    fn overwrites_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("note.md");
        fs::write(&target, "old contents").unwrap();

        atomic_write(&target, b"new contents").unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "new contents");
    }

    #[test]
    fn temp_path_is_sibling_of_target() {
        let target = Path::new("/vault/sub/note.md");
        let tmp = temp_path(target);
        assert_eq!(tmp.parent(), Some(Path::new("/vault/sub")));
        assert!(tmp.file_name().unwrap().to_str().unwrap().ends_with(".tmp"));
    }
}
