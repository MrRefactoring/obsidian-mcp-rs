//! The server must not outlive the client that spawned it.
//!
//! The unit tests in `src/parent.rs` cover the watch loop with an injected pid
//! source. This one builds a real process tree and kills the middle of it, which
//! is the only way to prove the thing works: a server whose stdin never reaches
//! EOF, whose parent dies, and which is expected to leave anyway.

#![cfg(unix)]

use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::{Duration, Instant};

/// Comfortably longer than the watcher's poll interval, with room for a loaded
/// CI box.
const GIVE_UP_AFTER: Duration = Duration::from_secs(30);

fn is_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn vault_with_a_note() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp vault");
    std::fs::write(dir.path().join("note.md"), "# note\n").expect("seed note");
    dir
}

/// A server under a shell we can kill, holding a stdin that will never see EOF.
///
/// The shell matters twice over. It gives us a parent to kill that is not the
/// test runner — and it must survive spawning, so the server is a *child* rather
/// than the shell itself: hence the trailing `true`, without which `sh -c`
/// exec's the single command and replaces itself with it.
///
/// The server must not be backgrounded with `&` either: a shell redirects an
/// asynchronous command's stdin from `/dev/null`, which hands it an immediate
/// EOF and lets it exit for reasons that have nothing to do with this test.
fn server_under_a_killable_shell(vault: &std::path::Path) -> (Child, ChildStdin, u32) {
    let bin = env!("CARGO_BIN_EXE_obsidian-mcp-rs");
    let mut shell = Command::new("sh")
        .arg("-c")
        .arg(format!("'{}' '{}'; true", bin, vault.display()))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the middle shell");

    // Held for the lifetime of the test and never written to: an open pipe with
    // no data in it, so the ordinary shutdown signal never arrives and only the
    // parent watch can end this process.
    let stdin = shell.stdin.take().expect("shell stdin");

    let deadline = Instant::now() + Duration::from_secs(10);
    let server_pid = loop {
        let found = Command::new("pgrep")
            .args(["-P", &shell.id().to_string()])
            .output()
            .ok()
            .and_then(|out| {
                String::from_utf8_lossy(&out.stdout)
                    .split_whitespace()
                    .next()?
                    .parse()
                    .ok()
            });
        if let Some(pid) = found {
            break pid;
        }
        assert!(Instant::now() < deadline, "the server never started");
        std::thread::sleep(Duration::from_millis(100));
    };

    // Let the server get past start-up before anyone kills its parent. Without
    // this the tests would all resolve through the "already orphaned at boot"
    // path and the polling watch would never be exercised at all.
    std::thread::sleep(Duration::from_secs(1));

    (shell, stdin, server_pid)
}

#[test]
fn the_server_exits_when_its_parent_dies_without_closing_stdin() {
    let vault = vault_with_a_note();
    let (mut shell, _stdin, server_pid) = server_under_a_killable_shell(vault.path());

    assert!(is_alive(server_pid), "server should be up before we start");

    // SIGKILL, so the shell gets no chance to tidy up after itself. This is the
    // force-quit/crash case, not a clean shutdown.
    shell.kill().expect("kill the middle shell");
    shell.wait().expect("reap the middle shell");

    let deadline = Instant::now() + GIVE_UP_AFTER;
    while is_alive(server_pid) {
        if Instant::now() >= deadline {
            let _ = Command::new("kill")
                .args(["-9", &server_pid.to_string()])
                .status();
            panic!(
                "server {server_pid} outlived its parent by {GIVE_UP_AFTER:?} — it is an orphan"
            );
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[test]
fn a_living_parent_does_not_trip_the_watch() {
    let vault = vault_with_a_note();
    let (mut shell, _stdin, server_pid) = server_under_a_killable_shell(vault.path());

    // Well past the poll interval: a watch that fires on a healthy parent would
    // have done it by now.
    std::thread::sleep(Duration::from_secs(12));
    let survived = is_alive(server_pid);

    let _ = shell.kill();
    let _ = shell.wait();
    let _ = Command::new("kill")
        .args(["-9", &server_pid.to_string()])
        .status();

    assert!(
        survived,
        "server {server_pid} exited while its parent was alive"
    );
}

#[test]
fn the_server_still_exits_on_stdin_eof() {
    let vault = vault_with_a_note();

    let mut server = Command::new(env!("CARGO_BIN_EXE_obsidian-mcp-rs"))
        .arg(vault.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the server");

    // Closing stdin is what the spec asks a client to do first, and it has to
    // keep working: the parent watch is a backstop, never a replacement.
    drop(server.stdin.take());

    let deadline = Instant::now() + GIVE_UP_AFTER;
    loop {
        if server.try_wait().expect("poll the server").is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = server.kill();
            panic!("server ignored stdin EOF for {GIVE_UP_AFTER:?}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
