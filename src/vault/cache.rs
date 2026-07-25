//! Not reading the whole vault again when the whole vault hasn't changed.
//!
//! Every vault-wide operation — search, `vault-info`, the link graph — reads
//! every note. Measured on a 5,000-note vault: the directory walk costs ~5 ms,
//! `stat`-ing every file ~3 ms, and *reading* every file ~50 ms. So the read is
//! the bill, and it is paid again on every single call.
//!
//! This caches file contents keyed by what the filesystem says about them. A
//! call still walks and still stats — that is how a change made by anything
//! else gets noticed — but it only re-reads the files whose `(mtime, len)` moved.
//!
//! # Why this matters more on synced vaults, not less
//!
//! On a real iCloud vault the same measurements come out at ~8 µs per file to
//! walk against ~1 µs locally: synced directories are the expensive kind. And
//! when a file has been evicted ("Optimize Mac Storage"), `stat` still answers
//! from the placeholder while *reading* pulls the file back over the network.
//! Re-reading the vault on every call is therefore worst exactly where this
//! cache helps most.
//!
//! # The invariant that keeps this from eating data
//!
//! **Only read-only operations may read through the cache.** Never the read half
//! of a read-modify-write.
//!
//! A stale entry served to `search-vault` is a stale snippet: wrong, visible,
//! and gone on the next change. The same stale entry served to `edit-note`
//! would be read, edited and written back — silently destroying whatever the
//! other writer had put there. One is a glitch, the other is data loss, and the
//! difference is the caller. So mutating methods keep reading from disk, and
//! this module is not reachable from them.
//!
//! # What `(mtime, len)` cannot see
//!
//! A write that lands in the same mtime tick as the previous one *and* leaves
//! the length unchanged. On APFS, ext4 and NTFS the timestamp is
//! sub-microsecond, so this needs two writes within the same tick — but on a
//! filesystem with one-second timestamps (HFS+, some network mounts) it is
//! reachable. The cost is a stale search snippet until the next edit, which is
//! the same trade every build system on the planet makes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// Stop caching past this much text.
///
/// Not an eviction policy — a blast radius. A 5,000-note vault holds about 4 MB
/// of text, so this is a vault two orders of magnitude larger than the ones this
/// is for, and the point is only that an enormous vault degrades to today's
/// behaviour instead of exhausting memory.
const MAX_CACHED_BYTES: usize = 256 * 1024 * 1024;

/// What the filesystem says about a file, as far as we can tell without reading
/// it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Stamp {
    mtime: Option<SystemTime>,
    len: u64,
}

impl Stamp {
    pub(crate) fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Self {
            mtime: meta.modified().ok(),
            len: meta.len(),
        })
    }
}

/// File contents, keyed by path, valid only while the stamp still matches.
#[derive(Default, Debug)]
pub(crate) struct ContentCache {
    entries: Mutex<HashMap<PathBuf, (Stamp, Arc<str>)>>,
}

impl ContentCache {
    /// The file's contents, from memory when nothing about it has changed.
    ///
    /// `None` when the file cannot be read at all — the same answer the direct
    /// read would have given, so callers that skip unreadable files keep doing
    /// exactly that.
    pub(crate) fn read(&self, path: &Path) -> Option<Arc<str>> {
        let stamp = Stamp::of(path)?;

        if let Ok(entries) = self.entries.lock()
            && let Some((cached, text)) = entries.get(path)
            && *cached == stamp
        {
            return Some(Arc::clone(text));
        }

        let text: Arc<str> = Arc::from(std::fs::read_to_string(path).ok()?);

        if let Ok(mut entries) = self.entries.lock() {
            let held: usize = entries.values().map(|(_, t)| t.len()).sum();
            // Over budget: serve the read, remember nothing. Dropping the whole
            // map instead would make every call re-read everything, which is the
            // behaviour this exists to avoid.
            if held + text.len() <= MAX_CACHED_BYTES {
                entries.insert(path.to_path_buf(), (stamp, Arc::clone(&text)));
            }
        }

        Some(text)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.lock().map(|e| e.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write(path: &Path, text: &str) {
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(text.as_bytes()).unwrap();
        f.sync_all().unwrap();
    }

    #[test]
    fn an_unchanged_file_is_served_from_memory() {
        let dir = tempfile::tempdir().unwrap();
        let note = dir.path().join("a.md");
        write(&note, "first");

        let cache = ContentCache::default();
        assert_eq!(&*cache.read(&note).unwrap(), "first");

        // Rewrite behind the cache's back without touching mtime *or* length —
        // same five bytes — to prove the second answer really came from memory
        // rather than from the disk.
        let stamp = Stamp::of(&note).unwrap();
        std::fs::write(&note, "SECND").unwrap();
        filetime_restore(&note, stamp);

        assert_eq!(&*cache.read(&note).unwrap(), "first");
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn a_file_written_by_someone_else_is_re_read() {
        let dir = tempfile::tempdir().unwrap();
        let note = dir.path().join("a.md");
        write(&note, "first");

        let cache = ContentCache::default();
        assert_eq!(&*cache.read(&note).unwrap(), "first");

        // What Obsidian, another agent or an arriving sync would do.
        std::thread::sleep(std::time::Duration::from_millis(10));
        write(&note, "second, longer");

        assert_eq!(&*cache.read(&note).unwrap(), "second, longer");
    }

    #[test]
    fn a_change_of_length_alone_is_enough() {
        let dir = tempfile::tempdir().unwrap();
        let note = dir.path().join("a.md");
        write(&note, "first");

        let cache = ContentCache::default();
        cache.read(&note).unwrap();

        // Same instant, different size: the length half of the stamp has to
        // catch this on filesystems whose timestamps are too coarse to.
        let stamp = Stamp::of(&note).unwrap();
        std::fs::write(&note, "first plus more").unwrap();
        filetime_restore(&note, stamp);

        assert_eq!(&*cache.read(&note).unwrap(), "first plus more");
    }

    #[test]
    fn an_unreadable_file_is_reported_as_such_and_not_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ContentCache::default();

        assert!(cache.read(&dir.path().join("missing.md")).is_none());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn a_deleted_file_stops_being_served() {
        let dir = tempfile::tempdir().unwrap();
        let note = dir.path().join("a.md");
        write(&note, "here");

        let cache = ContentCache::default();
        cache.read(&note).unwrap();
        std::fs::remove_file(&note).unwrap();

        assert!(cache.read(&note).is_none());
    }

    /// Put a file's mtime back to what it was, so a test can change contents
    /// without the stamp noticing.
    fn filetime_restore(path: &Path, stamp: Stamp) {
        let Some(mtime) = stamp.mtime else { return };
        let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        f.set_modified(mtime).unwrap();
    }
}
