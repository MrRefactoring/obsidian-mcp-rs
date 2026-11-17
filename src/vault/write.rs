use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::error::VaultError;
use sha2::{Digest, Sha256};

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let from = CString::new(from.as_os_str().as_bytes()).map_err(io::Error::other)?;
    let to = CString::new(to.as_os_str().as_bytes()).map_err(io::Error::other)?;
    #[cfg(target_os = "macos")]
    let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };

    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(windows)]
pub(crate) fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

    fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
        let path = std::path::absolute(path)?;
        let parent = path
            .parent()
            .ok_or(io::ErrorKind::InvalidInput)?
            .canonicalize()?;
        let path = parent.join(path.file_name().ok_or(io::ErrorKind::InvalidInput)?);
        let mut units: Vec<_> = path.as_os_str().encode_wide().collect();
        if units.contains(&0) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        units.push(0);
        Ok(units)
    }

    let from = wide_path(from)?;
    let to = wide_path(to)?;
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) } != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub(crate) fn rename_no_replace(_from: &Path, _to: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "This platform does not support moving notes without replacing an existing file",
    ))
}

/// Counter for temporary filenames. The PID distinguishes concurrent processes.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Write `contents` to `path` atomically.
///
/// Writes to a temp file in the *same directory*, flushes it, then `rename`s it
/// over the target. A crash or concurrent write mid-operation leaves the
/// original note fully intact (or absent) — never a half-written file. The temp
/// lives beside the target so the final `rename` stays on one filesystem
/// (cross-device renames aren't atomic and would fail).
pub(crate) fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), VaultError> {
    stage_write(path, contents, |from, to| fs::rename(from, to))
}

pub(crate) fn atomic_write_new(path: &Path, contents: &[u8]) -> Result<(), VaultError> {
    stage_write(path, contents, rename_no_replace)
}

pub(crate) fn atomic_write_new_using(
    path: &Path,
    write: impl FnOnce(&mut fs::File) -> io::Result<()>,
) -> Result<(), VaultError> {
    stage_write_using(path, write, rename_no_replace)
}

pub(crate) fn stream_revision(path: &Path) -> Result<String, VaultError> {
    stream_content(path, &mut io::sink())
        .map_err(|error| VaultError::io(path.display().to_string(), error))
}

pub(crate) fn copy_verified(
    source: &Path,
    output: &Path,
    revision: &str,
) -> Result<(), VaultError> {
    atomic_write_new_using(output, |file| {
        let actual = stream_content(source, file)?;
        if actual != revision {
            return Err(io::Error::other(format!(
                "Source changed: {}",
                source.display()
            )));
        }
        Ok(())
    })
}

fn stream_content(path: &Path, output: &mut impl Write) -> io::Result<String> {
    let mut input = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let length = input.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        let chunk = &buffer[..length];
        hash.update(chunk);
        output.write_all(chunk)?;
    }
    use std::fmt::Write as _;
    let mut revision = String::from("sha256:");
    for byte in hash.finalize() {
        write!(&mut revision, "{byte:02x}").expect("format import revision");
    }
    Ok(revision)
}

fn stage_write(
    path: &Path,
    contents: &[u8],
    rename: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> Result<(), VaultError> {
    stage_write_using(path, |file| file.write_all(contents), rename)
}

fn stage_write_using(
    path: &Path,
    write: impl FnOnce(&mut fs::File) -> io::Result<()>,
    rename: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> Result<(), VaultError> {
    let tmp = temp_path(path);

    // Close the file before the rename.
    {
        let mut file =
            fs::File::create_new(&tmp).map_err(|e| VaultError::io(tmp.display().to_string(), e))?;
        if let Err(e) = write(&mut file).and_then(|()| file.flush()) {
            drop(file);
            let _ = fs::remove_file(&tmp); // best-effort cleanup; keep original error
            return Err(VaultError::io(tmp.display().to_string(), e));
        }
    }

    rename(&tmp, path).map_err(|e| {
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
