use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
#[cfg(all(unix, not(feature = "http")))]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

const NEW_VERSION: &str = "9.9.9";

#[derive(Clone)]
struct Reply {
    status: u16,
    location: Option<String>,
    body: Vec<u8>,
}

impl Reply {
    fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            location: None,
            body: body.into(),
        }
    }

    #[cfg(all(unix, not(feature = "http")))]
    fn redirect(to: &str) -> Self {
        Self {
            status: 302,
            location: Some(to.to_string()),
            body: Vec::new(),
        }
    }
}

/// What the fake feed saw. Shared by every connection, so a test can assert on
/// "nothing was asked for at all" as well as on how many downloads happened.
#[derive(Clone, Default)]
struct Hits {
    requests: Arc<AtomicUsize>,
    downloads: Arc<AtomicUsize>,
    slow_asset: bool,
}

#[cfg(all(unix, not(feature = "http")))]
fn serve(routes: HashMap<String, Reply>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
    let base = format!("http://{}", listener.local_addr().unwrap());
    listen(listener, routes, Hits::default());
    base
}

/// One accept loop for every fake feed, so the counters handed in here are the
/// same objects every connection increments.
fn listen(listener: TcpListener, routes: HashMap<String, Reply>, hits: Hits) {
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let routes = routes.clone();
            let hits = hits.clone();
            std::thread::spawn(move || answer(stream, &routes, &hits));
        }
    });
}

fn answer(mut stream: TcpStream, routes: &HashMap<String, Reply>, hits: &Hits) {
    let mut buf = [0u8; 4096];
    let Ok(n) = stream.read(&mut buf) else { return };
    let request = String::from_utf8_lossy(&buf[..n]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/")
        .to_string();

    hits.requests.fetch_add(1, Ordering::SeqCst);
    if path == "/asset" {
        hits.downloads.fetch_add(1, Ordering::SeqCst);
        if hits.slow_asset {
            // Hold the winner's download open long enough that the losers are
            // certainly inside `apply`, so the update lock is what stops them
            // rather than the race simply not happening.
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    let reply = routes.get(&path).cloned().unwrap_or(Reply {
        status: 404,
        location: None,
        body: b"no such route".to_vec(),
    });

    let mut head = format!(
        "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        reply.status,
        reply.body.len()
    );
    if let Some(location) = &reply.location {
        head.push_str(&format!("Location: {location}\r\n"));
    }
    head.push_str("\r\n");

    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&reply.body);
    let _ = stream.flush();
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn replacement_binary() -> Vec<u8> {
    format!("#!/bin/sh\necho \"obsidian-mcp-rs {NEW_VERSION}\"\n").into_bytes()
}

fn feed(tag: &str, published: &str, body: &[u8]) -> String {
    feed_counted(tag, published, body, false).0
}

fn feed_counted(tag: &str, published: &str, body: &[u8], slow_asset: bool) -> (String, Hits) {
    let asset = obsidian_mcp_rs::update::asset_name().expect("this platform publishes a release");
    let checksums = format!("{}  {asset}\n", sha256_hex(body));
    let hits = Hits {
        slow_asset,
        ..Hits::default()
    };
    let base = serve_with(
        &mut HashMap::new(),
        body,
        &checksums,
        &asset,
        tag,
        published,
        hits.clone(),
    );
    (base, hits)
}

fn serve_with(
    routes: &mut HashMap<String, Reply>,
    body: &[u8],
    checksums: &str,
    asset: &str,
    tag: &str,
    published: &str,
    hits: Hits,
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
    let base = format!("http://{}", listener.local_addr().unwrap());

    let json = format!(
        r#"{{"tag_name":"{tag}","published_at":"{published}","assets":[
             {{"name":"{asset}","browser_download_url":"{base}/asset"}},
             {{"name":"checksums.txt","browser_download_url":"{base}/checksums"}}
           ]}}"#
    );
    routes.insert("/releases/latest".to_string(), Reply::ok(json));
    routes.insert("/asset".to_string(), Reply::ok(body.to_vec()));
    routes.insert("/checksums".to_string(), Reply::ok(checksums.to_string()));

    listen(listener, routes.clone(), hits);
    base
}

struct Installed {
    _home: tempfile::TempDir,
    home: PathBuf,
    exe: PathBuf,
}

fn install_into_a_temporary_home() -> Installed {
    let home = tempfile::tempdir().expect("a temp home");
    let bin = data_local(home.path()).join("obsidian-mcp-rs").join("bin");
    std::fs::create_dir_all(&bin).unwrap();

    let exe = bin.join(exe_name());
    std::fs::copy(env!("CARGO_BIN_EXE_obsidian-mcp-rs"), &exe).expect("place the server");

    Installed {
        home: home.path().to_path_buf(),
        _home: home,
        exe,
    }
}

#[cfg(target_os = "macos")]
fn data_local(home: &Path) -> PathBuf {
    home.join("Library").join("Application Support")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn data_local(home: &Path) -> PathBuf {
    home.join(".local").join("share")
}

#[cfg(windows)]
fn data_local(home: &Path) -> PathBuf {
    home.join("AppData").join("Local")
}

#[cfg(target_os = "macos")]
fn cache_home(home: &Path) -> PathBuf {
    home.join("Library").join("Caches")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn cache_home(home: &Path) -> PathBuf {
    home.join(".cache")
}

#[cfg(windows)]
fn cache_home(home: &Path) -> PathBuf {
    home.join("AppData").join("Local")
}

/// Nothing beside the installed copy: the staged download carries the pid of
/// the run that made it, so a fixed-name check would pass whatever happened.
#[cfg(all(unix, not(feature = "http")))]
fn no_leftovers(exe: &Path) -> bool {
    let (Some(parent), Some(name)) = (exe.parent(), exe.file_name().and_then(|n| n.to_str()))
    else {
        return true;
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return true;
    };
    !entries.flatten().any(|entry| {
        entry
            .file_name()
            .to_str()
            .and_then(|found| found.strip_prefix(name))
            .is_some_and(|suffix| suffix.starts_with(".download"))
    })
}

fn exe_name() -> &'static str {
    if cfg!(windows) {
        "obsidian-mcp-rs.exe"
    } else {
        "obsidian-mcp-rs"
    }
}

/// `spawn`, retried past ETXTBSY.
///
/// Linux refuses to exec a file any process still holds open for writing. These
/// tests copy the server into a throwaway home and run it, in parallel, so one
/// test's `fs::copy` can be in flight while another forks — and the forked
/// child inherits that write descriptor for the instant before it execs. The
/// window is tiny with a release binary and wide enough to hit reliably under
/// `cargo llvm-cov`, where the instrumented binary is several times the size.
fn spawn_once_free(cmd: &mut Command) -> std::io::Result<Child> {
    for _ in 0..50 {
        match cmd.spawn() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(Duration::from_millis(100));
            }
            other => return other,
        }
    }
    cmd.spawn()
}

fn update(exe: &Path, home: Option<&Path>, endpoint: &str, extra: &[&str]) -> Output {
    let mut cmd = Command::new(exe);
    cmd.arg("update").args(extra);
    cmd.env("OBSIDIAN_MCP_UPDATE_ENDPOINT", endpoint);
    if let Some(home) = home {
        cmd.env("HOME", home);
        cmd.env("XDG_DATA_HOME", data_local(home));
        cmd.env("XDG_CACHE_HOME", cache_home(home));
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    spawn_once_free(&mut cmd)
        .expect("run the updater")
        .wait_with_output()
        .expect("collect the updater's output")
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn a_copy_that_install_did_not_place_is_refused_rather_than_replaced() {
    let built = PathBuf::from(env!("CARGO_BIN_EXE_obsidian-mcp-rs"));
    let before = std::fs::metadata(&built).unwrap().len();

    let out = update(&built, None, "http://127.0.0.1:1/never-reached", &[]);

    assert!(
        out.status.success(),
        "should decline quietly: {}",
        said(&out)
    );
    assert!(
        said(&out).contains("not the installed copy"),
        "unexpected message: {}",
        said(&out)
    );
    assert_eq!(
        std::fs::metadata(&built).unwrap().len(),
        before,
        "the binary under test was modified"
    );
}

#[cfg(all(unix, not(feature = "http")))]
#[test]
fn an_update_past_the_soak_window_replaces_the_installed_copy() {
    let body = replacement_binary();
    let it = install_into_a_temporary_home();
    let base = feed("v9.9.9", "2020-01-01T00:00:00Z", &body);

    let out = update(&it.exe, Some(&it.home), &base, &[]);
    assert!(out.status.success(), "{}", said(&out));
    assert!(said(&out).contains("updated"), "{}", said(&out));

    assert_eq!(
        std::fs::read(&it.exe).unwrap(),
        body,
        "the swap did not land"
    );

    let mut reporting = Command::new(&it.exe);
    reporting.arg("--version").stdout(Stdio::piped());
    let reported = spawn_once_free(&mut reporting)
        .expect("run the replacement")
        .wait_with_output()
        .expect("collect the replacement's output");
    assert!(
        String::from_utf8_lossy(&reported.stdout).contains(NEW_VERSION),
        "the replacement does not run"
    );
    assert!(no_leftovers(&it.exe), "the download was left behind");
}

#[cfg(all(unix, not(feature = "http")))]
#[test]
fn a_release_still_inside_the_soak_window_is_held_back() {
    let body = replacement_binary();
    let it = install_into_a_temporary_home();
    let before = std::fs::read(&it.exe).unwrap();
    let published = chrono::Utc::now() - chrono::TimeDelta::try_hours(1).unwrap();
    let base = feed("v9.9.9", &published.to_rfc3339(), &body);

    let out = update(&it.exe, Some(&it.home), &base, &[]);

    assert!(out.status.success(), "{}", said(&out));
    assert!(said(&out).contains("holding until"), "{}", said(&out));
    assert_eq!(std::fs::read(&it.exe).unwrap(), before, "it updated anyway");
}

#[cfg(all(unix, not(feature = "http")))]
#[test]
fn check_reports_the_available_release_without_touching_anything() {
    let body = replacement_binary();
    let it = install_into_a_temporary_home();
    let before = std::fs::read(&it.exe).unwrap();
    let base = feed("v9.9.9", "2020-01-01T00:00:00Z", &body);

    let out = update(&it.exe, Some(&it.home), &base, &["--check"]);

    assert!(out.status.success(), "{}", said(&out));
    assert!(said(&out).contains("9.9.9"), "{}", said(&out));
    assert_eq!(std::fs::read(&it.exe).unwrap(), before, "--check wrote");
}

#[cfg(all(unix, not(feature = "http")))]
#[test]
fn a_download_that_does_not_match_its_checksum_is_thrown_away() {
    let it = install_into_a_temporary_home();
    let before = std::fs::read(&it.exe).unwrap();

    let asset = obsidian_mcp_rs::update::asset_name().unwrap();
    let mut routes = HashMap::new();
    let honest = replacement_binary();
    let checksums = format!("{}  {asset}\n", sha256_hex(&honest));
    let base = serve_with(
        &mut routes,
        b"this is not the binary that was hashed",
        &checksums,
        &asset,
        "v9.9.9",
        "2020-01-01T00:00:00Z",
        Hits::default(),
    );

    let out = update(&it.exe, Some(&it.home), &base, &[]);

    assert!(!out.status.success(), "a bad download should fail loudly");
    assert!(
        said(&out).contains("checksum"),
        "unexpected message: {}",
        said(&out)
    );
    assert_eq!(
        std::fs::read(&it.exe).unwrap(),
        before,
        "it installed anyway"
    );
    assert!(no_leftovers(&it.exe));
}

#[cfg(all(unix, not(feature = "http")))]
#[test]
fn a_feed_that_redirects_off_its_own_host_is_refused() {
    let it = install_into_a_temporary_home();
    let before = std::fs::read(&it.exe).unwrap();

    let mut routes = HashMap::new();
    routes.insert(
        "/releases/latest".to_string(),
        Reply::redirect("https://evil.example.com/payload"),
    );
    let base = serve(routes);

    let out = update(&it.exe, Some(&it.home), &base, &[]);

    assert!(!out.status.success(), "{}", said(&out));
    assert!(
        said(&out).contains("not an address this updater trusts"),
        "unexpected message: {}",
        said(&out)
    );
    assert_eq!(std::fs::read(&it.exe).unwrap(), before);
}

#[cfg(all(unix, not(feature = "http")))]
fn spawn_server(it: &Installed, endpoint: &str, vault: &Path) -> Child {
    let mut cmd = Command::new(&it.exe);
    cmd.arg(vault)
        .env("OBSIDIAN_MCP_UPDATE_ENDPOINT", endpoint)
        .env("HOME", &it.home)
        .env("XDG_DATA_HOME", data_local(&it.home))
        .env("XDG_CACHE_HOME", cache_home(&it.home))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    spawn_once_free(&mut cmd).expect("start the server")
}

#[cfg(all(unix, not(feature = "http")))]
fn wait_until(deadline: Duration, mut done: impl FnMut() -> bool) -> bool {
    let started = std::time::Instant::now();
    while started.elapsed() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

#[cfg(all(unix, not(feature = "http")))]
fn stop(mut server: Child) -> Output {
    drop(server.stdin.take());
    server
        .wait_with_output()
        .expect("collect the server output")
}

#[cfg(all(unix, not(feature = "http")))]
#[test]
fn a_running_server_replaces_itself_without_being_asked() {
    let body = replacement_binary();
    let it = install_into_a_temporary_home();
    let base = feed("v9.9.9", "2020-01-01T00:00:00Z", &body);
    let vault = tempfile::tempdir().unwrap();

    let server = spawn_server(&it, &base, vault.path());
    let replaced = wait_until(Duration::from_secs(20), || {
        std::fs::read(&it.exe).is_ok_and(|b| b == body)
    });
    let out = stop(server);

    assert!(
        replaced,
        "the server did not update itself in the background"
    );

    // stdout carries the MCP JSON-RPC stream and nothing else. This is the only
    // test where the update path runs to completion, so it is the only place a
    // stray `println!` in `apply` or `smoke_test` would ever be caught.
    assert!(
        out.stdout.is_empty(),
        "the update path wrote to stdout: {:?}",
        String::from_utf8_lossy(&out.stdout)
    );

    let state = cache_home(&it.home)
        .join("obsidian-mcp-rs")
        .join("update-check.json");
    let recorded = std::fs::read_to_string(&state).expect("the check should have been recorded");
    assert!(recorded.contains("9.9.9"), "unexpected state: {recorded}");
}

#[cfg(all(unix, not(feature = "http")))]
#[test]
fn a_server_that_was_opted_out_leaves_itself_alone() {
    let body = replacement_binary();
    let it = install_into_a_temporary_home();
    let before = std::fs::read(&it.exe).unwrap();
    std::fs::write(
        data_local(&it.home)
            .join("obsidian-mcp-rs")
            .join("no-auto-update"),
        b"",
    )
    .unwrap();

    let (base, hits) = feed_counted("v9.9.9", "2020-01-01T00:00:00Z", &body, false);
    let vault = tempfile::tempdir().unwrap();

    let server = spawn_server(&it, &base, vault.path());
    // Wait on the same budget the positive test gets, and measure the thing the
    // name promises: a request that never happened, not a file that is not there
    // yet because a loaded runner was slow.
    let looked = wait_until(Duration::from_secs(20), || {
        hits.requests.load(Ordering::SeqCst) > 0
    });
    stop(server);

    assert!(
        !looked,
        "opting out should stop the check before it reaches the network"
    );
    assert_eq!(
        hits.requests.load(Ordering::SeqCst),
        0,
        "an opted-out server still asked the feed for something"
    );
    assert_eq!(std::fs::read(&it.exe).unwrap(), before, "it updated anyway");
}

#[cfg(unix)]
#[test]
fn configuring_another_client_does_not_switch_auto_update_back_on() {
    let it = install_into_a_temporary_home();
    let vault = tempfile::tempdir().unwrap();
    let marker = data_local(&it.home)
        .join("obsidian-mcp-rs")
        .join("no-auto-update");

    let install = |extra: &[&str]| {
        let mut cmd = Command::new(&it.exe);
        cmd.args(["install", "claude-code", "--global", "--force"])
            .args(extra)
            .arg(vault.path())
            .env("HOME", &it.home)
            .env("XDG_DATA_HOME", data_local(&it.home))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        spawn_once_free(&mut cmd)
            .expect("run the installer")
            .wait_with_output()
            .expect("collect the installer's output")
    };

    assert!(install(&["--no-auto-update"]).status.success());
    assert!(marker.exists(), "opting out was not recorded");

    assert!(install(&[]).status.success());
    assert!(
        marker.exists(),
        "a later install with no opinion about auto-update turned it back on"
    );

    assert!(install(&["--auto-update"]).status.success());
    assert!(!marker.exists(), "opting back in did not take");
}

#[cfg(all(unix, not(feature = "http")))]
#[test]
fn force_takes_a_release_that_is_still_inside_the_hold() {
    let body = replacement_binary();
    let it = install_into_a_temporary_home();
    let published = chrono::Utc::now() - chrono::TimeDelta::try_hours(1).unwrap();
    let base = feed("v9.9.9", &published.to_rfc3339(), &body);

    let held = update(&it.exe, Some(&it.home), &base, &[]);
    assert!(said(&held).contains("holding until"), "{}", said(&held));
    assert_ne!(
        std::fs::read(&it.exe).unwrap(),
        body,
        "the hold did not hold"
    );

    let forced = update(&it.exe, Some(&it.home), &base, &["--force"]);

    assert!(forced.status.success(), "{}", said(&forced));
    assert_eq!(
        std::fs::read(&it.exe).unwrap(),
        body,
        "--force did not lift the hold"
    );
}

#[cfg(all(unix, not(feature = "http")))]
#[test]
fn racing_processes_download_the_release_exactly_once() {
    let body = replacement_binary();
    let it = install_into_a_temporary_home();
    let (base, hits) = feed_counted("v9.9.9", "2020-01-01T00:00:00Z", &body, true);

    let racers: Vec<Child> = (0..3)
        .map(|_| {
            let mut cmd = Command::new(&it.exe);
            cmd.arg("update")
                .env("OBSIDIAN_MCP_UPDATE_ENDPOINT", &base)
                .env("HOME", &it.home)
                .env("XDG_DATA_HOME", data_local(&it.home))
                .env("XDG_CACHE_HOME", cache_home(&it.home))
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            spawn_once_free(&mut cmd).expect("start a racer")
        })
        .collect();

    for racer in racers {
        let out = racer.wait_with_output().expect("wait for a racer");
        assert!(
            out.status.success(),
            "a racer failed: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    assert_eq!(std::fs::read(&it.exe).unwrap(), body, "nobody applied it");
    assert_eq!(
        hits.downloads.load(Ordering::SeqCst),
        1,
        "the lock did not stop the losers from downloading too"
    );
    // All three reached the decision, so the single download is the lock's doing
    // and not three runs that happened to be sequential.
    assert_eq!(
        hits.requests.load(Ordering::SeqCst),
        5,
        "the racers did not all reach the feed"
    );
}

#[cfg(all(unix, feature = "http"))]
#[test]
fn a_build_with_features_the_releases_lack_refuses_to_replace_itself() {
    let body = replacement_binary();
    let it = install_into_a_temporary_home();
    let before = std::fs::read(&it.exe).unwrap();
    let base = feed("v9.9.9", "2020-01-01T00:00:00Z", &body);

    let out = update(&it.exe, Some(&it.home), &base, &[]);

    assert!(out.status.success(), "{}", said(&out));
    assert!(
        said(&out).contains("features the published binaries do not"),
        "unexpected message: {}",
        said(&out)
    );
    assert_eq!(
        std::fs::read(&it.exe).unwrap(),
        before,
        "an http build replaced itself with one that has no http"
    );
}

#[cfg(all(unix, not(feature = "http")))]
#[test]
fn a_download_that_will_not_run_is_thrown_away() {
    let it = install_into_a_temporary_home();
    let before = std::fs::read(&it.exe).unwrap();
    let broken = b"#!/bin/sh\nexit 1\n".to_vec();
    let base = feed("v9.9.9", "2020-01-01T00:00:00Z", &broken);

    let out = update(&it.exe, Some(&it.home), &base, &[]);

    assert!(!out.status.success(), "{}", said(&out));
    assert!(
        said(&out).contains("--version"),
        "the smoke test should name what it asked: {}",
        said(&out)
    );
    assert_eq!(
        std::fs::read(&it.exe).unwrap(),
        before,
        "it installed anyway"
    );
    assert!(no_leftovers(&it.exe));
}

#[cfg(all(unix, not(feature = "http")))]
#[test]
fn a_download_that_reports_another_version_is_thrown_away() {
    let it = install_into_a_temporary_home();
    let before = std::fs::read(&it.exe).unwrap();
    let impostor = b"#!/bin/sh\necho \"obsidian-mcp-rs 1.2.3\"\n".to_vec();
    let base = feed("v9.9.9", "2020-01-01T00:00:00Z", &impostor);

    let out = update(&it.exe, Some(&it.home), &base, &[]);

    assert!(!out.status.success(), "{}", said(&out));
    assert!(
        said(&out).contains("1.2.3") && said(&out).contains("9.9.9"),
        "the message should name both versions: {}",
        said(&out)
    );
    assert_eq!(
        std::fs::read(&it.exe).unwrap(),
        before,
        "it installed anyway"
    );
    assert!(no_leftovers(&it.exe));
}

#[cfg(all(unix, not(feature = "http")))]
#[test]
fn a_release_staged_by_an_earlier_run_is_installed_at_the_next_start() {
    let body = replacement_binary();
    let it = install_into_a_temporary_home();
    let pending = PathBuf::from(format!("{}.pending", it.exe.display()));
    std::fs::write(&pending, &body).unwrap();
    std::fs::set_permissions(&pending, std::fs::Permissions::from_mode(0o755)).unwrap();

    let vault = tempfile::tempdir().unwrap();
    let base = feed("v0.0.1", "2020-01-01T00:00:00Z", &body);
    let server = spawn_server(&it, &base, vault.path());
    let installed = wait_until(Duration::from_secs(20), || {
        std::fs::read(&it.exe).is_ok_and(|b| b == body)
    });
    stop(server);

    assert!(installed, "the staged release was never put in place");
    assert!(!pending.exists(), "the staged file was left behind");
}
