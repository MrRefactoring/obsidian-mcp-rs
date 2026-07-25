//! Exiting when the client that spawned us goes away.
//!
//! stdin EOF is the primary shutdown signal and the only portable one — the MCP
//! spec asks servers for exactly that, and we honour it: closing our stdin ends
//! `service.waiting()` and the process exits. The gap is that EOF is not
//! guaranteed to *arrive*. The write end of our stdin is a refcounted resource;
//! if any process other than the client holds a copy when the client dies, the
//! refcount never reaches zero, no EOF is ever delivered, and we sit there
//! forever — reparented to init, holding write access to someone's vault, one
//! more process for every unclean client exit.
//!
//! So we also watch the parent, and leave when it does.
//!
//! # What this deliberately does not do
//!
//! Each of these is a failure mode that other MCP servers shipped first:
//!
//! - **No idle timeout.** A server that exits after N quiet minutes is a server
//!   whose tools vanish mid-session from any client that does not reconnect on
//!   demand — which includes Claude Code, where the MCP connection is
//!   established once at session start and never re-established.
//! - **No stdin-state heuristics on top of this.** Polling flags like "is stdin
//!   destroyed" false-positives on Windows while the parent is busy, and real
//!   EOF is already handled by the transport.
//! - **No parent-identity check beyond the pid.** Comparing the parent's start
//!   time to catch pid reuse breaks when the system clock moves. On Unix the
//!   kernel already guarantees what we need — a ppid changes only when the
//!   parent dies — and on Windows we hold a handle to the process *object*,
//!   which a recycled pid cannot alias.
//!
//! # What it does not cover
//!
//! A process chain. Launched as `npx -y obsidian-mcp-rs`, our parent is npm's
//! process rather than the client, and the client's death leaves that parent
//! alive — so nothing here fires. That is fixed by not building the chain: the
//! installer writes a direct path to this binary, making it a child of the
//! client itself.

#[cfg(unix)]
use std::time::Duration;

/// How often the Unix watcher re-reads its parent pid. An orphan lives for
/// days, so seconds of detection latency cost nothing. Windows waits on a
/// handle instead and needs no interval.
#[cfg(unix)]
const POLL: Duration = Duration::from_secs(5);

/// What we found when we went looking for the process that spawned us.
enum Host {
    /// A live parent to watch.
    Alive(Parent),
    /// There was a parent, and it is already gone — we were orphaned in the
    /// window between being spawned and getting far enough to look.
    ///
    /// This is not a corner case to shrug at. It is the same race that makes
    /// `PR_SET_PDEATHSIG` unreliable on its own, and treating it as "nothing to
    /// watch" is how a watchdog ends up guarding every orphan except the ones
    /// created fastest.
    GoneAlready,
    /// We are the top of our own tree — pid 1 in a container, say. Nobody's
    /// death should reap us.
    None,
}

/// Was this process started by something whose death should reap us?
///
/// A parent of pid 1 means init/launchd adopted us, i.e. the real parent is
/// gone. The exception is being pid 1 ourselves (a container entrypoint), where
/// there is no parent to speak of — `getppid` reports 0 and nothing is wrong.
#[cfg(unix)]
fn classify_ppid(ppid: u32, own_pid: u32) -> Option<bool> {
    match ppid {
        _ if own_pid == 1 || ppid == 0 => None, // no tree above us
        1 => Some(false),                       // adopted by init: already orphaned
        _ => Some(true),                        // a real parent
    }
}

/// Watch the process that spawned us, and run `on_gone` once it is gone.
///
/// Runs `on_gone` immediately — on the calling thread — when we are already an
/// orphan by the time we look. Otherwise the watch runs on a detached thread
/// that never keeps the process alive by itself, so stdin EOF stays the primary
/// exit.
pub fn watch_parent<F>(on_gone: F)
where
    F: FnOnce(String) + Send + 'static,
{
    match Parent::detect() {
        Host::Alive(parent) => {
            std::thread::spawn(move || on_gone(parent.wait_until_gone()));
        }
        Host::GoneAlready => {
            on_gone("parent process exited before the watch started".to_string());
        }
        Host::None => tracing::debug!("no parent process to watch"),
    }
}

#[cfg(unix)]
use unix::Parent;
#[cfg(windows)]
use windows::Parent;

#[cfg(unix)]
mod unix {
    use super::{Host, POLL, classify_ppid, poll_until_gone};

    pub(super) struct Parent {
        boot_ppid: u32,
    }

    impl Parent {
        pub(super) fn detect() -> Host {
            let boot_ppid = current_ppid();
            match classify_ppid(boot_ppid, std::process::id()) {
                Some(true) => Host::Alive(Self { boot_ppid }),
                Some(false) => Host::GoneAlready,
                None => Host::None,
            }
        }

        pub(super) fn wait_until_gone(self) -> String {
            poll_until_gone(self.boot_ppid, current_ppid, POLL, std::thread::sleep)
        }
    }

    fn current_ppid() -> u32 {
        // SAFETY: `getppid` cannot fail and touches no memory we own.
        unsafe { libc::getppid() as u32 }
    }
}

/// The Unix watch loop, with its pid source and its clock injected so it can be
/// exercised without a real process tree.
#[cfg(unix)]
fn poll_until_gone(
    boot_ppid: u32,
    mut current_ppid: impl FnMut() -> u32,
    poll: Duration,
    mut sleep: impl FnMut(Duration),
) -> String {
    loop {
        sleep(poll);
        let now = current_ppid();
        if now != boot_ppid {
            return format!("parent process {boot_ppid} exited (reparented to {now})");
        }
    }
}

#[cfg(windows)]
mod windows {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcessId, INFINITE, OpenProcess, PROCESS_ACCESS_RIGHTS, WaitForSingleObject,
    };

    use super::Host;

    // windows-sys files the standard access rights under `Storage::FileSystem`,
    // so importing the constant would mean enabling a whole feature module for
    // one number that is fixed by the ABI.
    const SYNCHRONIZE: PROCESS_ACCESS_RIGHTS = 0x0010_0000;

    /// A handle to the parent rather than its pid: the handle names the process
    /// *object*, so a recycled pid cannot make us wait on a stranger.
    pub(super) struct Parent {
        handle: HANDLE,
        pid: u32,
    }

    // The handle is owned here and only waited on. Handing it to the watcher
    // thread is the whole point of the type.
    unsafe impl Send for Parent {}

    impl Parent {
        pub(super) fn detect() -> Host {
            // Windows never reparents, so a missing entry means the parent has
            // already exited rather than that we were adopted.
            let Some(pid) = parent_pid() else {
                return Host::GoneAlready;
            };
            if pid == 0 {
                // The System Idle Process: nothing above us to be reaped by.
                return Host::None;
            }
            // SAFETY: a plain Win32 call. A null return is checked below.
            let handle = unsafe { OpenProcess(SYNCHRONIZE, 0, pid) };
            if handle.is_null() {
                // It was there in the snapshot and is not there now.
                return Host::GoneAlready;
            }
            Host::Alive(Self { handle, pid })
        }

        pub(super) fn wait_until_gone(self) -> String {
            // SAFETY: a live SYNCHRONIZE handle we opened and still own.
            unsafe { WaitForSingleObject(self.handle, INFINITE) };
            format!("parent process {} exited", self.pid)
        }
    }

    impl Drop for Parent {
        fn drop(&mut self) {
            // SAFETY: closing a handle we opened, exactly once.
            unsafe { CloseHandle(self.handle) };
        }
    }

    /// Win32 has no `getppid`. Our parent's pid is only reachable by walking a
    /// snapshot of the process table and finding our own entry in it.
    fn parent_pid() -> Option<u32> {
        // SAFETY: the snapshot handle is checked before use and closed on every
        // path out; `entry` is zeroed and carries the `dwSize` the API requires.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return None;
            }
            let me = GetCurrentProcessId();
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

            let mut found = None;
            if Process32FirstW(snapshot, &mut entry) != 0 {
                loop {
                    if entry.th32ProcessID == me {
                        found = Some(entry.th32ParentProcessID);
                        break;
                    }
                    if Process32NextW(snapshot, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snapshot);
            found
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn an_ordinary_parent_is_watched() {
        assert_eq!(classify_ppid(4242, 4243), Some(true));
    }

    #[cfg(unix)]
    #[test]
    fn a_parent_of_init_means_we_are_already_an_orphan() {
        // The race this exists for: spawned, parent died, and only then did we
        // get far enough to look. Reading this as "nothing to watch" is how the
        // fastest-created orphans become the permanent ones.
        assert_eq!(classify_ppid(1, 4243), Some(false));
    }

    #[cfg(unix)]
    #[test]
    fn the_top_of_the_tree_has_no_host() {
        // pid 1 in a container: `getppid` reports 0 and nothing is wrong.
        assert_eq!(classify_ppid(0, 1), None);
        assert_eq!(classify_ppid(0, 4243), None);
    }

    #[cfg(unix)]
    #[test]
    fn the_watch_ends_when_the_parent_is_replaced() {
        let mut polls = 0;
        let reason = poll_until_gone(
            4242,
            || {
                polls += 1;
                if polls < 3 { 4242 } else { 1 }
            },
            Duration::ZERO,
            |_| {},
        );

        assert_eq!(reason, "parent process 4242 exited (reparented to 1)");
    }

    #[cfg(unix)]
    #[test]
    fn a_live_parent_keeps_the_watch_running() {
        // The loop only returns on a change, so bound it from the clock side and
        // assert it was still polling when the bound was reached.
        let mut rounds = 0;
        let ended = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            poll_until_gone(
                7,
                || 7,
                Duration::ZERO,
                |_| {
                    rounds += 1;
                    if rounds == 50 {
                        panic!("bound reached");
                    }
                },
            )
        }));

        assert!(ended.is_err(), "watch ended while the parent was alive");
    }
}
