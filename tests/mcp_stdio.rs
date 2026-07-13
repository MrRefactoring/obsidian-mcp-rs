//! End-to-end tests of the real MCP stdio transport.
//!
//! These spawn the built binary against a temp vault and drive a full JSON-RPC
//! handshake over stdin/stdout. That matters: they are the only tests that see
//! what a *client* sees. A unit test can assert on `ObsidianHandler`'s fields and
//! still miss the server advertising something else entirely over the wire —
//! which is exactly how `--no-edit` shipped in 0.5.0 still listing every write
//! tool. Closing stdin gives the server EOF, so the tests never block.

use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

/// Tools that only ever write. `--no-edit` must not advertise any of them.
const WRITE_TOOLS: [&str; 8] = [
    "create-note",
    "edit-note",
    "delete-note",
    "move-note",
    "create-directory",
    "add-tags",
    "remove-tags",
    "rename-tag",
];

/// Drive a real server over stdio and collect its responses by request id.
fn talk(extra_args: &[&str], vault: &std::path::Path, messages: &[Value]) -> HashMap<i64, Value> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_obsidian-mcp-rs"));
    cmd.arg("--log-file")
        .arg("-") // disable file logging so we don't touch the user's log dir
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
        // Drop closes stdin → server sees EOF and exits.
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
        // Every line on stdout must be protocol. A stray `println!` anywhere in
        // the server would corrupt the stream, and this is what proves none does.
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
        "params": {"name": "read-note", "arguments": {"vault": vault_name, "filename": "hello"}}
    }));

    let by_id = talk(&[], vault.path(), &msgs);

    let init = by_id.get(&1).expect("no response to initialize");
    assert!(init.get("result").is_some(), "initialize errored: {init}");

    let names = tool_names(&by_id);
    assert_eq!(names.len(), 15, "expected 15 tools, got {names:?}");
    for expected in [
        "read-note",
        "create-note",
        "edit-note",
        "delete-note",
        "move-note",
        "create-directory",
        "search-vault",
        "add-tags",
        "remove-tags",
        "rename-tag",
        "list-available-vaults",
        "wikilinks",
        "frontmatter",
        "vault-info",
        "periodic",
    ] {
        assert!(names.iter().any(|n| n == expected), "missing {expected}");
    }

    let text = by_id
        .get(&3)
        .and_then(|r| r.pointer("/result/content/0/text"))
        .and_then(Value::as_str)
        .expect("no text content in read-note response");
    assert!(text.contains("hi there"), "unexpected note content: {text}");
}

#[test]
fn no_edit_does_not_advertise_the_write_tools_over_the_wire() {
    // The regression that shipped in 0.5.0: `with_options` pruned the write tools
    // out of the router, but `#[tool_handler]` defaults to building a *fresh*
    // one, so `tools/list` advertised all eight anyway. The unit test passed the
    // whole time, because it asked the pruned field instead of the protocol.
    // Only a real client can answer this question.
    let (vault, vault_name) = vault_with_a_note();
    let mut msgs = handshake();
    msgs.push(json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "delete-note", "arguments": {"vault": vault_name, "filename": "hello"}}
    }));

    let by_id = talk(&["--no-edit"], vault.path(), &msgs);
    let names = tool_names(&by_id);

    for write_tool in WRITE_TOOLS {
        assert!(
            !names.iter().any(|n| n == write_tool),
            "--no-edit advertised '{write_tool}' to the client: {names:?}"
        );
    }
    assert_eq!(names.len(), 7, "expected the 7 read tools, got {names:?}");
    // `frontmatter` both reads and writes, so it stays listed — it is gated per
    // action, and `get` is a read.
    assert!(names.iter().any(|n| n == "frontmatter"));

    // The route is genuinely gone, not merely hidden: calling it still fails...
    let del = by_id.get(&3).expect("no response to delete-note");
    assert!(
        del.get("error").is_some(),
        "delete-note must be unreachable under --no-edit, got: {del}"
    );
    // ...and the note is still there.
    assert!(vault.path().join("hello.md").exists());
}

/// The five tools that answer with structured content used to return `Json<T>`,
/// which can only express success — so "that note does not exist" left the server
/// as a JSON-RPC protocol error, a shape the spec reserves for a request the
/// server could not process, and one a client may swallow instead of showing the
/// model. They now build the result themselves, which means two things have to
/// hold at once, and only a real client can see either: the `outputSchema` the
/// return type used to derive is still advertised, and a missing note comes back
/// as `isError`.
#[test]
fn the_structured_tools_keep_their_output_schema_and_can_report_is_error() {
    let vault = tempfile::tempdir().unwrap();
    std::fs::write(vault.path().join("hello.md"), "hi").unwrap();

    let mut messages = handshake();
    messages.push(json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {
            "name": "frontmatter",
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
        "frontmatter",
        "wikilinks",
        "search-vault",
        "periodic",
        "vault-info",
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
