//! The server must not outlive the client that spawned it.
//!
//! The unit tests in `src/parent.rs` use an injected PID source.
//! These tests use a real process tree. They check that the server exits when
//! its parent dies, even if stdin remains open.

#![cfg(unix)]

use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::{Duration, Instant};

/// Allow more than the poll interval for a busy CI runner.
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

/// Start a server under a shell that the test can kill, with stdin open.
///
/// The trailing `true` prevents `sh -c` from replacing itself with the server.
/// Do not use `&`: the shell can redirect stdin from `/dev/null`, causing EOF.
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

    // Keep stdin open without writing data. This prevents shutdown on EOF.
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

    // Let the server start before the test kills its parent. This exercises
    // the polling watch instead of only the already-orphaned startup path.
    std::thread::sleep(Duration::from_secs(1));

    (shell, stdin, server_pid)
}

#[test]
fn the_server_exits_when_its_parent_dies_without_closing_stdin() {
    let vault = vault_with_a_note();
    let (mut shell, _stdin, server_pid) = server_under_a_killable_shell(vault.path());

    assert!(is_alive(server_pid), "server should be up before we start");

    // SIGKILL simulates a crash without shell cleanup.
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

    // Wait longer than the poll interval to check that a live parent is safe.
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

    // Closing stdin must still stop the server when its parent is alive.
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
