use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

    fn redirect(to: &str) -> Self {
        Self {
            status: 302,
            location: Some(to.to_string()),
            body: Vec::new(),
        }
    }
}

fn serve(routes: HashMap<String, Reply>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
    let base = format!("http://{}", listener.local_addr().unwrap());

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let routes = routes.clone();
            std::thread::spawn(move || answer(stream, &routes));
        }
    });

    base
}

fn answer(mut stream: TcpStream, routes: &HashMap<String, Reply>) {
    let mut buf = [0u8; 4096];
    let Ok(n) = stream.read(&mut buf) else { return };
    let request = String::from_utf8_lossy(&buf[..n]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/")
        .to_string();

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
    let asset = obsidian_mcp_rs::update::asset_name().expect("this platform publishes a release");
    let checksums = format!("{}  {asset}\n", sha256_hex(body));
    serve_with(
        &mut HashMap::new(),
        body,
        &checksums,
        &asset,
        tag,
        published,
    )
}

fn serve_with(
    routes: &mut HashMap<String, Reply>,
    body: &[u8],
    checksums: &str,
    asset: &str,
    tag: &str,
    published: &str,
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

    let table = routes.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let table = table.clone();
            std::thread::spawn(move || answer(stream, &table));
        }
    });

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

fn exe_name() -> &'static str {
    if cfg!(windows) {
        "obsidian-mcp-rs.exe"
    } else {
        "obsidian-mcp-rs"
    }
}

fn update(exe: &Path, home: Option<&Path>, endpoint: &str, extra: &[&str]) -> Output {
    let mut cmd = Command::new(exe);
    cmd.arg("update").args(extra);
    cmd.env("OBSIDIAN_MCP_UPDATE_ENDPOINT", endpoint);
    if let Some(home) = home {
        cmd.env("HOME", home);
        cmd.env("XDG_DATA_HOME", data_local(home));
        cmd.env("XDG_CACHE_HOME", home.join(".cache"));
    }
    cmd.output().expect("run the updater")
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

#[cfg(unix)]
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

    let reported = Command::new(&it.exe).arg("--version").output().unwrap();
    assert!(
        String::from_utf8_lossy(&reported.stdout).contains(NEW_VERSION),
        "the replacement does not run"
    );
    assert!(
        !it.exe.with_extension("download").exists(),
        "the download was left behind"
    );
}

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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
    assert!(!it.exe.with_extension("download").exists());
}

#[cfg(unix)]
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
