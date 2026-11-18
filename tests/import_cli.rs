use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use serde_json::{Value, json};

const BINARY: &str = env!("CARGO_BIN_EXE_obsidian-mcp-rs");

fn fixture(connection_flags: &[&str]) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    let vault = directory.path().join("Vault");
    fs::create_dir(&vault).unwrap();
    fs::create_dir(directory.path().join("source")).unwrap();
    let mut argv: Vec<_> = connection_flags
        .iter()
        .map(|flag| flag.to_string())
        .collect();
    argv.push(vault.to_str().unwrap().into());
    fs::write(
        directory.path().join(".mcp.json"),
        serde_json::to_vec(&json!({
            "mcpServers": {"obsidian": {"command": BINARY, "args": argv}}
        }))
        .unwrap(),
    )
    .unwrap();
    directory
}

fn import(root: &Path, folder: &str, extra: &[&str]) -> Output {
    Command::new(BINARY)
        .current_dir(root)
        .args([
            "import-markdown",
            "--client",
            "claude-code",
            "--vault",
            "Vault",
            "--source",
            "source",
            "--folder",
            folder,
        ])
        .args(extra)
        .output()
        .unwrap()
}

fn report(result: &Output) -> Value {
    serde_json::from_slice(&result.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}; stderr: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    })
}

fn plan_id(report: &Value) -> &str {
    report["plan_id"].as_str().unwrap()
}

#[test]
fn import_markdown_cli_previews_and_writes_selected_notes_and_attachments() {
    let directory = fixture(&["--write-folder", "Vault=Inbox"]);
    let root = directory.path();
    fs::create_dir_all(root.join("source/project")).unwrap();
    fs::create_dir_all(root.join("source/media")).unwrap();
    fs::create_dir_all(root.join("Vault/Inbox")).unwrap();
    fs::write(
        root.join("source/project/Main.md"),
        "[next](Next.md#Heading)\n[[project/Next#Heading|next]]\n![image](../media/photo.png)\n",
    )
    .unwrap();
    fs::write(root.join("source/project/Next.md"), "# Heading\n").unwrap();
    fs::write(root.join("source/media/photo.png"), [0, 17, 255]).unwrap();
    fs::write(root.join("source/media/unused.png"), [99]).unwrap();

    let preview = import(root, "Inbox/Import", &[]);
    assert!(preview.status.success());
    let plan = report(&preview);
    assert_eq!(plan["files"].as_array().unwrap().len(), 3);
    assert!(plan["blockers"].as_array().unwrap().is_empty(), "{plan}");
    assert_eq!(plan["link_changes"].as_array().unwrap().len(), 1);
    assert!(!root.join("Vault/Inbox/Import").exists());

    let applied = import(root, "Inbox/Import", &["--apply", plan_id(&plan)]);
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    assert_eq!(report(&applied)["applied"], true);
    assert_eq!(
        fs::read(root.join("Vault/Inbox/Import/media/photo.png")).unwrap(),
        [0, 17, 255]
    );
    assert!(!root.join("Vault/Inbox/Import/media/unused.png").exists());
    assert_eq!(
        fs::read_to_string(root.join("Vault/Inbox/Import/project/Main.md")).unwrap(),
        "[next](Next.md#Heading)\n[[Inbox/Import/project/Next#Heading|next]]\n![image](../media/photo.png)\n"
    );
}

#[test]
fn import_markdown_cli_rejects_stale_plans_and_keeps_existing_destination() {
    let directory = fixture(&[]);
    let root = directory.path();
    fs::write(root.join("source/Note.md"), "one").unwrap();
    let preview = report(&import(root, ".", &[]));
    fs::write(root.join("source/Note.md"), "two").unwrap();
    let stale = import(root, ".", &["--apply", plan_id(&preview)]);
    assert!(!stale.status.success());
    assert!(
        report(&stale)["blockers"][0]
            .as_str()
            .unwrap()
            .contains("import changed")
    );
    assert!(!root.join("Vault/Note.md").exists());

    fs::write(root.join("Vault/Note.md"), "existing").unwrap();
    fs::write(root.join("source/Other.md"), "[old](Note.md)").unwrap();
    let collision = report(&import(root, ".", &[]));
    assert!(
        collision["blockers"][0]
            .as_str()
            .unwrap()
            .contains("already exists")
    );
    let selected = report(&import(root, ".", &["--exclude", "Note.md"]));
    assert!(selected["blockers"].as_array().unwrap().is_empty());
    assert!(
        selected["warnings"][0]
            .as_str()
            .unwrap()
            .contains("excluded")
    );
    let applied = import(
        root,
        ".",
        &["--exclude", "Note.md", "--apply", plan_id(&selected)],
    );
    assert!(applied.status.success());
    assert_eq!(
        fs::read_to_string(root.join("Vault/Note.md")).unwrap(),
        "existing"
    );
}

#[test]
fn import_markdown_cli_reports_missing_attachments_and_connection_limits() {
    let directory = fixture(&["--no-edit", "--write-folder", "Vault=Inbox"]);
    let root = directory.path();
    fs::write(root.join("source/Note.md"), "![[missing.png]]").unwrap();
    let plan = report(&import(root, "Elsewhere", &[]));
    let blockers = plan["blockers"].as_array().unwrap();
    assert!(
        blockers
            .iter()
            .any(|value| value.as_str().unwrap().contains("--no-edit"))
    );
    assert!(
        blockers
            .iter()
            .any(|value| value.as_str().unwrap().contains("missing.png"))
    );
    assert!(
        blockers
            .iter()
            .any(|value| value.as_str().unwrap().contains("not allowed"))
    );
    let plan = report(&import(root, "Inbox", &["--allow-missing-attachments"]));
    assert!(
        plan["warnings"][0]
            .as_str()
            .unwrap()
            .contains("missing.png")
    );
    let applied = import(
        root,
        "Inbox",
        &["--allow-missing-attachments", "--apply", plan_id(&plan)],
    );
    assert!(!applied.status.success());
    assert!(!root.join("Vault/Inbox").exists());
}

#[cfg(unix)]
#[test]
fn import_markdown_cli_does_not_follow_source_symlinks() {
    let directory = fixture(&[]);
    let root = directory.path();
    fs::write(root.join("source/Note.md"), "![image](leak.png)").unwrap();
    fs::write(root.join("secret.png"), "external bytes").unwrap();
    std::os::unix::fs::symlink(root.join("secret.png"), root.join("source/leak.png")).unwrap();
    let preview = report(&import(root, ".", &[]));
    assert_eq!(preview["files"].as_array().unwrap().len(), 1);
    assert!(
        preview["blockers"][0]
            .as_str()
            .unwrap()
            .contains("missing target")
    );
}
