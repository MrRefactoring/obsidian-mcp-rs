//! End-to-end tests of the real MCP stdio transport.
//!
//! These tests start the binary with a temporary vault and send JSON-RPC requests
//! over stdio. They check responses visible to a client. Unit tests can miss
//! differences between handler fields and the protocol. In 0.5.0, `--no-edit`
//! still listed write tools. Closing stdin sends EOF to the server.

use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

/// Write-only tools that `--no-edit` must not advertise.
const WRITE_TOOLS: [&str; 13] = [
    "create_note",
    "edit_note",
    "delete_note",
    "move_note",
    "create_folder",
    "add_note_tags",
    "remove_note_tags",
    "rename_vault_tag",
    "create_task",
    "update_task",
    "apply_task_batch",
    "create_canvas",
    "edit_canvas",
];

/// Drive a real server over stdio and collect its responses by request id.
fn talk(extra_args: &[&str], vault: &std::path::Path, messages: &[Value]) -> HashMap<i64, Value> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_obsidian-mcp-rs"));
    cmd.arg("--log-file")
        .arg("-") // Disable file logging to protect the user's log directory.
        .args(extra_args)
        .arg(vault)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn().expect("failed to spawn server binary");
    {
        let mut stdin = child.stdin.take().unwrap();
        for m in messages {
            writeln!(stdin, "{}", serde_json::to_string(m).unwrap()).unwrap();
        }
        // Drop closes stdin, so the server receives EOF and exits.
    }

    let output = child.wait_with_output().expect("server did not exit");
    assert!(
        output.status.success(),
        "server exited with failure: {:?}",
        output.status
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let mut by_id = HashMap::new();
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        // Every stdout line must be protocol data. A `println!` would corrupt it.
        let v: Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("non-JSON line on stdout: {line:?} ({e})"));
        if let Some(id) = v.get("id").and_then(Value::as_i64) {
            by_id.insert(id, v);
        }
    }
    by_id
}

fn handshake() -> Vec<Value> {
    vec![
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "mcp-stdio-it", "version": "0"}
            }
        }),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
    ]
}

fn tool_names(by_id: &HashMap<i64, Value>) -> Vec<String> {
    by_id
        .get(&2)
        .and_then(|r| r.pointer("/result/tools"))
        .and_then(Value::as_array)
        .expect("no tools array in tools/list response")
        .iter()
        .filter_map(|t| t.get("name").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

fn vault_with_a_note() -> (tempfile::TempDir, String) {
    let vault = tempfile::tempdir().unwrap();
    std::fs::write(vault.path().join("hello.md"), "hi there").unwrap();
    // VaultManager keys each vault by its directory basename.
    let name = vault
        .path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    (vault, name)
}

#[test]
fn stdio_handshake_lists_tools_and_reads_a_note() {
    let (vault, vault_name) = vault_with_a_note();
    let mut msgs = handshake();
    msgs.push(json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "read_note", "arguments": {"vault": vault_name, "filename": "hello"}}
    }));

    let by_id = talk(&[], vault.path(), &msgs);

    let init = by_id.get(&1).expect("no response to initialize");
    assert!(init.get("result").is_some(), "initialize errored: {init}");
    assert!(
        init["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("get_writing_guide")
    );

    let names = tool_names(&by_id);
    assert_eq!(names.len(), 25, "expected 25 tools, got {names:?}");
    for expected in [
        "read_note",
        "create_note",
        "edit_note",
        "delete_note",
        "move_note",
        "create_folder",
        "search_notes",
        "add_note_tags",
        "remove_note_tags",
        "rename_vault_tag",
        "list_vaults",
        "vault_links",
        "note_properties",
        "vault_info",
        "periodic_notes",
        "get_writing_guide",
        "list_tasks",
        "preview_task_batch",
        "read_canvas",
        "batch_move_notes",
    ] {
        assert!(names.iter().any(|n| n == expected), "missing {expected}");
    }

    let text = by_id
        .get(&3)
        .and_then(|r| r.pointer("/result/content/0/text"))
        .and_then(Value::as_str)
        .expect("no text content in read_note response");
    assert!(text.contains("hi there"), "unexpected note content: {text}");
    let revision = by_id[&3]["result"]["content"][1]["text"].as_str().unwrap();
    assert!(revision.starts_with("Note revision: sha256:"));
}

#[test]
fn no_edit_does_not_advertise_the_write_tools_over_the_wire() {
    // In 0.5.0, `with_options` removed write tools from the router, but
    // `#[tool_handler]` built a new router and `tools/list` still advertised them.
    // This test checks the protocol response.
    let (vault, vault_name) = vault_with_a_note();
    let mut msgs = handshake();
    msgs.push(json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "delete_note", "arguments": {"vault": vault_name, "filename": "hello"}}
    }));
    msgs.push(json!({
        "jsonrpc": "2.0", "id": 4, "method": "tools/call",
        "params": {"name": "get_writing_guide", "arguments": {"topics": ["meeting", "callouts"]}}
    }));

    let by_id = talk(&["--no-edit"], vault.path(), &msgs);
    let names = tool_names(&by_id);

    for write_tool in WRITE_TOOLS {
        assert!(
            !names.iter().any(|n| n == write_tool),
            "--no-edit advertised '{write_tool}' to the client: {names:?}"
        );
    }
    assert_eq!(
        names.len(),
        12,
        "expected 12 available tools, got {names:?}"
    );
    // `frontmatter` stays listed because `get` reads without writing.
    assert!(names.iter().any(|n| n == "note_properties"));
    assert!(names.iter().any(|n| n == "get_writing_guide"));
    let guide = by_id[&4]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(guide.contains("## Decisions"));
    assert!(guide.contains("## Actions"));
    assert!(guide.contains("## callouts"));

    // The removed route must reject calls.
    let del = by_id.get(&3).expect("no response to delete_note");
    assert!(
        del.get("error").is_some(),
        "delete_note must be unreachable under --no-edit, got: {del}"
    );
    // The note must remain in the vault.
    assert!(vault.path().join("hello.md").exists());
}

/// Structured-content tools must advertise `outputSchema` and return `isError`
/// for a missing note. They previously returned that error as a JSON-RPC error.
#[test]
fn the_structured_tools_keep_their_output_schema_and_can_report_is_error() {
    let vault = tempfile::tempdir().unwrap();
    std::fs::write(vault.path().join("hello.md"), "hi").unwrap();

    let mut messages = handshake();
    messages.push(json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {
            "name": "note_properties",
            "arguments": {
                "vault": vault.path().file_name().unwrap().to_str().unwrap(),
                "filename": "ghost",
                "action": "get"
            }
        }
    }));
    let by_id = talk(&[], vault.path(), &messages);

    let tools = by_id[&2]["result"]["tools"].as_array().unwrap();
    for name in [
        "note_properties",
        "vault_links",
        "search_notes",
        "periodic_notes",
        "vault_info",
    ] {
        let tool = tools
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("{name} is not in tools/list"));
        assert!(
            tool["outputSchema"]["properties"].is_object(),
            "{name} lost its outputSchema when it stopped returning Json<T>: {tool}"
        );
    }

    // A missing note is an answer, not a protocol failure.
    let call = &by_id[&3];
    assert!(
        call.get("error").is_none(),
        "a missing note must not be a protocol error: {call}"
    );
    let result = &call["result"];
    assert_eq!(
        result["isError"],
        json!(true),
        "not flagged as an error: {result}"
    );
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("ghost"),
        "the model can only fix the name if we tell it the name: {text:?}"
    );
}

/// The log must not contain note content.
///
/// `rmcp` logs response payloads at DEBUG. Enabling DEBUG for dependencies
/// wrote note text to the log. The default filter enables DEBUG for this crate
/// only; `RUST_LOG` can enable other targets.
#[test]
fn note_contents_never_reach_the_log_file() {
    const SECRET: &str = "ZZTOPSECRETPHRASEZZ";

    let vault = tempfile::tempdir().unwrap();
    std::fs::write(
        vault.path().join("diary.md"),
        format!("# diary\n\n{SECRET}\n"),
    )
    .unwrap();
    // Add a second note and a search hit that includes the secret in its snippet.
    std::fs::write(
        vault.path().join("other.md"),
        format!("# other\n\nthe diary mentions {SECRET} in passing\n"),
    )
    .unwrap();
    let name = vault
        .path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let log_dir = tempfile::tempdir().unwrap();
    let log = log_dir.path().join("server.log");

    let mut messages = handshake();
    messages.push(json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "read_note", "arguments": {"vault": name, "filename": "diary.md"}}
    }));
    // Do not search for the secret itself. Queries are logged, but responses
    // from the vault must not be logged.
    messages.push(json!({
        "jsonrpc": "2.0", "id": 4, "method": "tools/call",
        "params": {"name": "search_notes", "arguments": {"vault": name, "query": "diary"}}
    }));

    let by_id = talk_with_log(&log, vault.path(), &messages);

    // Check that the server returned the note.
    let served = serde_json::to_string(by_id.get(&3).expect("no read_note response")).unwrap();
    assert!(
        served.contains(SECRET),
        "read_note did not return the note; the assertion below would pass for the wrong reason"
    );

    let written = std::fs::read_to_string(&log).expect("no log file was written");
    assert!(
        !written.contains(SECRET),
        "the note's contents were written to the log:\n{written}"
    );
    // The log must still identify the calls for diagnosis.
    assert!(
        written.contains("read_note"),
        "the log lost its diagnostic value entirely:\n{written}"
    );
}

/// Run `talk` with file logging directed to `log`.
fn talk_with_log(
    log: &std::path::Path,
    vault: &std::path::Path,
    messages: &[Value],
) -> HashMap<i64, Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_obsidian-mcp-rs"))
        .arg("--log-file")
        .arg(log)
        .arg(vault)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn server binary");
    {
        let mut stdin = child.stdin.take().unwrap();
        for m in messages {
            writeln!(stdin, "{}", serde_json::to_string(m).unwrap()).unwrap();
        }
    }
    let output = child.wait_with_output().expect("server did not exit");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let mut by_id = HashMap::new();
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: Value = serde_json::from_str(line).expect("non-JSON line on stdout");
        if let Some(id) = v.get("id").and_then(Value::as_i64) {
            by_id.insert(id, v);
        }
    }
    by_id
}

const NEW_LIFECYCLE_META: &str = "io.modelcontextprotocol/protocolVersion";

fn discover_meta() -> Value {
    json!({
        NEW_LIFECYCLE_META: "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {}
    })
}

#[test]
fn a_version_with_a_handshake_negotiates_to_itself() {
    let (vault, _) = vault_with_a_note();

    for requested in ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"] {
        let msgs = vec![json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": requested,
                "capabilities": {},
                "clientInfo": {"name": "negotiation-it", "version": "0"}
            }
        })];

        let agreed = talk(&[], vault.path(), &msgs)
            .get(&1)
            .and_then(|r| r.pointer("/result/protocolVersion"))
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("no protocolVersion in the answer to {requested}"))
            .to_string();

        assert_eq!(agreed, requested, "{requested} was answered with {agreed}");
    }
}

#[test]
fn a_version_without_a_handshake_falls_back_over_initialize() {
    let (vault, _) = vault_with_a_note();

    for requested in ["2026-07-28", "2099-01-01"] {
        let msgs = vec![json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": requested,
                "capabilities": {},
                "clientInfo": {"name": "negotiation-it", "version": "0"}
            }
        })];

        let answer = talk(&[], vault.path(), &msgs);
        let init = answer.get(&1).expect("no response to initialize");
        assert!(init.get("result").is_some(), "initialize errored: {init}");

        let agreed = init
            .pointer("/result/protocolVersion")
            .and_then(Value::as_str)
            .expect("no protocolVersion in the initialize result");

        assert_eq!(
            agreed, "2025-11-25",
            "{requested} should fall back to the newest version with a handshake"
        );
    }
}

#[test]
fn a_client_on_the_new_lifecycle_discovers_and_calls_tools() {
    let (vault, vault_name) = vault_with_a_note();
    let meta = discover_meta();

    let msgs = vec![
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "server/discover",
            "params": {"_meta": meta}
        }),
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/list",
            "params": {"_meta": meta}
        }),
        json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {
                "name": "read_note",
                "arguments": {"vault": vault_name, "filename": "hello"},
                "_meta": meta
            }
        }),
    ];

    let by_id = talk(&[], vault.path(), &msgs);

    let discover = by_id.get(&1).expect("no response to server/discover");
    assert!(
        discover.get("result").is_some(),
        "discover errored: {discover}"
    );

    let supported: Vec<&str> = discover
        .pointer("/result/supportedVersions")
        .and_then(Value::as_array)
        .expect("no supportedVersions in the discover result")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        supported.contains(&"2026-07-28"),
        "discover does not offer 2026-07-28: {supported:?}"
    );

    assert!(
        discover
            .pointer("/result/capabilities/tools")
            .is_some_and(|t| t.is_object()),
        "discover does not advertise the tools capability: {discover}"
    );

    assert_eq!(
        discover.pointer("/result/_meta/io.modelcontextprotocol~1serverInfo/name"),
        Some(&json!(env!("CARGO_PKG_NAME"))),
        "discover does not identify this server: {discover}"
    );

    assert_eq!(tool_names(&by_id).len(), 25);
    assert!(
        tool_names(&by_id)
            .iter()
            .any(|name| name == "get_writing_guide")
    );
    assert!(
        discover["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("get_writing_guide")
    );

    let text = by_id
        .get(&3)
        .and_then(|r| r.pointer("/result/content/0/text"))
        .and_then(Value::as_str)
        .expect("no text content in the read_note response");
    assert!(text.contains("hi there"), "unexpected note content: {text}");
}

#[test]
fn a_refused_handshake_still_keeps_stdout_to_protocol() {
    let (vault, _) = vault_with_a_note();

    let mut child = Command::new(env!("CARGO_BIN_EXE_obsidian-mcp-rs"))
        .arg("--log-file")
        .arg("-")
        .arg(vault.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn server binary");
    {
        let mut stdin = child.stdin.take().unwrap();
        let request = json!({
            "jsonrpc": "2.0", "id": 1, "method": "server/discover", "params": {}
        });
        writeln!(stdin, "{}", serde_json::to_string(&request).unwrap()).unwrap();
    }
    let output = child.wait_with_output().expect("server did not exit");
    assert_eq!(
        output.status.code(),
        Some(1),
        "a refused handshake should end the session as an error, not a clean shutdown"
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 1, "unexpected stdout: {stdout:?}");

    let answer: Value = serde_json::from_str(lines[0])
        .unwrap_or_else(|e| panic!("non-JSON line on stdout: {:?} ({e})", lines[0]));
    assert_eq!(
        answer.pointer("/error/code").and_then(Value::as_i64),
        Some(-32602),
        "unexpected answer: {answer}"
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        !stderr.trim().is_empty(),
        "the diagnosis did not reach stderr"
    );
}
