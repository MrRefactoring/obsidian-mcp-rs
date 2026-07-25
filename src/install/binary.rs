//! Putting the server somewhere a client config can point at forever.
//!
//! The installer used to write `npx -y obsidian-mcp-rs` into every config. That
//! is three processes per client — npm, the Node wrapper, the server — of which
//! a client terminating "the server" only ever kills the first, and it defeats
//! the parent-liveness watch outright: our parent becomes npm's process rather
//! than the client, so the client's death is invisible to us.
//!
//! Hardcoding the resolved path instead is worse. It lands in npm's `_npx`
//! cache, whose directory name is a hash of the package spec and changes with
//! the version — four such directories were observed on one machine — so the
//! config silently stops working after an update, and the symptom is "the MCP
//! server disappeared".
//!
//! So the installer places the binary itself. `install` is a subcommand of the
//! very binary that needs installing, which makes `current_exe()` the whole of
//! the path resolution: no npm internals, no package layout, no guessing.
//! Updating is running `install` again from a newer package — the copy is
//! replaced and **the path in every config stays the same forever**.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// What the installed copy is called. Windows needs the extension or the file
/// is not executable.
const EXE_NAME: &str = if cfg!(windows) {
    "obsidian-mcp-rs.exe"
} else {
    "obsidian-mcp-rs"
};

/// Where the installed copy lives, or `None` when the OS offers no per-user data
/// directory to put it in.
pub(crate) fn stable_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("obsidian-mcp-rs").join("bin").join(EXE_NAME))
}

/// Copy the running binary to [`stable_path`] and return where it went.
///
/// Copying rather than symlinking on purpose: a symlink into npm's `_npx` cache
/// would dangle the moment npm cleaned up, which is the failure this exists to
/// remove.
pub(crate) fn install() -> Result<PathBuf> {
    let dest = stable_path().context(
        "no per-user data directory on this system, so there is nowhere stable to install to",
    )?;
    let src = std::env::current_exe().context("could not find the running executable")?;

    // Running the installed copy to reinstall: nothing to do, and `fs::copy`
    // onto itself would truncate the file we are executing.
    if is_same_file(&src, &dest) {
        return Ok(dest);
    }

    let parent = dest
        .parent()
        .context("the install path has no parent directory")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("could not create {}", parent.display()))?;

    std::fs::copy(&src, &dest).map_err(|e| in_use_hint(e, &dest))?;
    Ok(dest)
}

/// Remove the installed copy, and the directory if that leaves it empty.
///
/// Missing is success: uninstalling something that was never installed is not
/// an error worth reporting.
pub(crate) fn uninstall() -> Result<Option<PathBuf>> {
    let Some(path) = stable_path() else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    std::fs::remove_file(&path).map_err(|e| in_use_hint(e, &path))?;
    if let Some(parent) = path.parent() {
        // Best effort: only succeeds when it is empty, which is what we want.
        let _ = std::fs::remove_dir(parent);
    }
    Ok(Some(path))
}

/// The version the installed copy reports, by asking it.
///
/// The copy only changes when `install` runs, so it can lag the package the user
/// just updated. That skew is invisible unless something goes looking, which is
/// what `list` uses this for. `None` means it is not installed or would not run.
pub(crate) fn installed_version() -> Option<String> {
    let path = stable_path()?;
    let out = std::process::Command::new(&path)
        .arg("--version")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // clap prints "<name> <version>"; the version is all we want.
    String::from_utf8(out.stdout)
        .ok()?
        .split_whitespace()
        .next_back()
        .map(str::to_string)
}

/// Turn "the file is busy" into something a person can act on.
///
/// Windows refuses to overwrite a running executable, so the common way to hit
/// this is reinstalling while a client still has the server open — and the raw
/// error for that is `Access denied`, which says nothing about what to do.
fn in_use_hint(e: std::io::Error, path: &Path) -> anyhow::Error {
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => anyhow::anyhow!(
            "could not write {} — if an AI client is running, it may still have the server open. \
             Quit the client(s) and run this again.\n  ({e})",
            path.display()
        ),
        _ => anyhow::Error::new(e).context(format!("could not write {}", path.display())),
    }
}

fn is_same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Fail loudly rather than write a config pointing at a binary that is not there.
pub(crate) fn ensure_installed(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!(
            "{} is missing after installing it — refusing to write a config that points at nothing",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_installed_copy_lives_under_the_user_data_directory() {
        let Some(path) = stable_path() else {
            return; // no data dir on this machine
        };
        assert!(dirs::data_local_dir().is_some_and(|d| path.starts_with(d)));
        assert_eq!(path.file_name().unwrap(), EXE_NAME);
    }

    #[test]
    fn the_windows_copy_keeps_its_extension() {
        // Without `.exe` the copy is not executable on Windows, and the failure
        // shows up as a client that silently cannot start the server.
        if cfg!(windows) {
            assert!(EXE_NAME.ends_with(".exe"));
        } else {
            assert!(!EXE_NAME.contains('.'));
        }
    }

    #[test]
    fn a_missing_binary_is_refused_rather_than_written_into_a_config() {
        let dir = tempfile::tempdir().unwrap();
        assert!(ensure_installed(&dir.path().join("nope")).is_err());

        let there = dir.path().join("yes");
        std::fs::write(&there, b"").unwrap();
        assert!(ensure_installed(&there).is_ok());
    }

    #[test]
    fn a_file_is_the_same_as_itself_and_not_as_its_neighbour() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::write(&a, b"x").unwrap();
        std::fs::write(&b, b"x").unwrap();

        assert!(is_same_file(&a, &a));
        assert!(!is_same_file(&a, &b));
        // A path that does not exist cannot be "the same file" as anything.
        assert!(!is_same_file(&a, &dir.path().join("missing")));
    }

    #[test]
    fn a_busy_destination_says_what_to_do_about_it() {
        let e = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let msg = in_use_hint(e, Path::new("/tmp/x")).to_string();
        assert!(msg.contains("Quit the client"), "unhelpful message: {msg}");
    }
}
