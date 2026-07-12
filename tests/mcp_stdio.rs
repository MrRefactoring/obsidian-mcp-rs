//! End-to-end test of the real MCP stdio transport.
//!
//! Spawns the built binary against a temp vault, drives a full JSON-RPC
//! handshake over stdin/stdout (`initialize` → `initialized` → `tools/list` →
//! `tools/call read-note`), and asserts the server speaks the protocol and
//! exposes all 14 tools. Closing stdin gives the server EOF, which shuts it
//! down cleanly — so the test never blocks.

use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

#[test]
fn stdio_handshake_lists_tools_and_reads_a_note() {
    let vault = tempfile::tempdir().unwrap();
    std::fs::write(vault.path().join("hello.md"), "hi there").unwrap();
    // VaultManager keys each vault by its directory basename.
    let vault_name = vault
        .path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let mut child = Command::new(env!("CARGO_BIN_EXE_obsidian-mcp-rs"))
        .arg("--log-file")
        .arg("-") // disable file logging so we don't touch the user's log dir
        .arg(vault.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn server binary");

    let messages = [
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
        json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {
                "name": "read-note",
                "arguments": {"vault": vault_name, "filename": "hello"}
            }
        }),
    ];

    {
        let mut stdin = child.stdin.take().unwrap();
        for m in &messages {
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
    let mut by_id: HashMap<i64, Value> = HashMap::new();
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("non-JSON line on stdout: {line:?} ({e})"));
        if let Some(id) = v.get("id").and_then(Value::as_i64) {
            by_id.insert(id, v);
        }
    }

    // initialize handshake succeeded
    let init = by_id.get(&1).expect("no response to initialize");
    assert!(init.get("result").is_some(), "initialize errored: {init}");

    // tools/list exposes exactly the 14 documented tools
    let tools = by_id
        .get(&2)
        .and_then(|r| r.pointer("/result/tools"))
        .and_then(Value::as_array)
        .expect("no tools array in tools/list response");
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|t| t.get("name").and_then(Value::as_str))
        .collect();
    assert_eq!(names.len(), 14, "expected 14 tools, got {names:?}");
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
    ] {
        assert!(names.contains(&expected), "missing tool {expected}");
    }

    // read-note returned the note's content over the live transport
    let text = by_id
        .get(&3)
        .and_then(|r| r.pointer("/result/content/0/text"))
        .and_then(Value::as_str)
        .expect("no text content in read-note response");
    assert!(text.contains("hi there"), "unexpected note content: {text}");
}
