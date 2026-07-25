//! Serialising writes between processes, not just between threads.
//!
//! `VaultManager`'s mutex covers threads inside one process. That was the whole
//! story while there was one server per vault — and there is not. Clients spawn
//! duplicate servers, users run more than one client against the same vault, and
//! two live servers each doing read → edit → write on one note lose an edit: both
//! read the old text, both write, and the second `rename` wins. No error, no
//! trace, the user's change is simply gone. `atomic_write` does not help; it
//! makes a single write atomic, never the read-modify-write *pair*.
//!
//! So the mutex is backed by an advisory file lock, taken for the same span.
//!
//! # One lock, not one per vault
//!
//! The in-process mutex already serialises every mutation across every vault,
//! for the reasons in `write_guard`'s docs: mutations are short, tool calls are
//! not a hot loop, and one lock cannot deadlock. The file lock keeps that shape,
//! which also means `write_guard` stays callable as the first statement of a
//! mutating method — before the vault argument has been resolved to a path.
//!
//! # Outside the vault, deliberately
//!
//! The lock file lives in the OS cache directory. Inside the vault it would sync
//! through iCloud or Obsidian Sync and show up in the user's own file tree.
//!
//! # Scope
//!
//! Advisory locking is per machine. Two devices writing to one cloud-synced
//! vault are not covered and cannot be at this layer — that is a sync conflict,
//! and it belongs to whatever is doing the syncing.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

/// Where the lock lives when nobody has said otherwise.
///
/// `None` when the OS has no cache directory to offer, which is the one case
/// where cross-process locking is simply unavailable.
pub(crate) fn default_lock_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("obsidian-mcp-rs").join("write.lock"))
}

/// Hold an exclusive advisory lock on `path` until the returned file is dropped.
///
/// Blocks while another process holds it. That is the point: the caller is about
/// to read, edit and write a note, and the whole span has to be exclusive. A
/// process that dies holding the lock releases it — the kernel drops the lock
/// with the file descriptor — so a crash cannot wedge every other server.
///
/// Returns `None` when the lock could not be taken at all. Callers carry on with
/// in-process locking only: refusing every write because a cache directory is
/// unavailable would be a worse failure than the race this prevents.
pub(crate) fn lock_exclusive(path: &Path) -> Option<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
        .ok()?;

    // Spelled out rather than `file.lock()`: `std::fs::File` grew an inherent
    // `lock` of its own in 1.89, which would silently win the method lookup on a
    // new enough toolchain and leave the crate importing something it no longer
    // uses. Our MSRV is 1.88, so the crate is still what makes this compile.
    fs4::FileExt::lock(&file).ok()?;
    Some(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lock_file_lives_outside_any_vault() {
        let Some(path) = default_lock_path() else {
            return; // no cache dir on this machine; nothing to assert
        };
        assert!(path.ends_with("obsidian-mcp-rs/write.lock"));
        assert!(
            dirs::cache_dir().is_some_and(|c| path.starts_with(c)),
            "the lock must not land anywhere a vault could be synced from"
        );
    }

    #[test]
    fn locking_creates_the_file_and_its_parent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("write.lock");

        let held = lock_exclusive(&path).expect("take the lock");

        assert!(path.exists());
        drop(held);
    }

    #[test]
    fn an_unusable_path_degrades_instead_of_panicking() {
        let dir = tempfile::tempdir().unwrap();
        // A file where a directory would have to be: `create_dir_all` fails, and
        // the caller has to end up with in-process locking rather than a panic.
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, b"not a directory").unwrap();

        assert!(lock_exclusive(&blocker.join("write.lock")).is_none());
    }
}
