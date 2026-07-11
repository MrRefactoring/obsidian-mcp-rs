use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::clients::ConfigFormat;

/// Serialization shape for a Goose extension entry in config.yaml
#[derive(Serialize, Deserialize, Clone)]
struct GooseExtension {
    name: String,
    #[serde(rename = "type")]
    ext_type: String,
    cmd: String,
    args: Vec<String>,
    enabled: bool,
    timeout: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallStatus {
    /// obsidian-mcp-rs entry present in config
    Installed,
    /// Config file exists but no obsidian entry
    NotInstalled,
    /// Config file does not exist at all
    FileNotFound,
}

pub enum WriteOutcome {
    AlreadyInstalled,
    Written { created: bool },
    DryRun { would_create: bool },
}

/// Check whether obsidian-mcp-rs is registered in the given config file
pub fn check_status(path: &Path, format: &ConfigFormat) -> InstallStatus {
    backend(format).check_status(path)
}

/// Add (or overwrite) the obsidian-mcp-rs entry in the config file.
///
/// When `force` is true, an existing entry is replaced. When false,
/// `WriteOutcome::AlreadyInstalled` is returned and the file is left untouched.
pub fn write_entry(
    path: &Path,
    format: &ConfigFormat,
    vaults: &[PathBuf],
    dry_run: bool,
    force: bool,
    no_edit: bool,
) -> Result<WriteOutcome> {
    backend(format).write_entry(path, vaults, dry_run, force, no_edit)
}

/// Remove the obsidian-mcp-rs entry from the config file.
/// Returns `true` if an entry was found and removed.
pub fn remove_entry(path: &Path, format: &ConfigFormat, dry_run: bool) -> Result<bool> {
    backend(format).remove_entry(path, dry_run)
}

// ── Backend dispatch ──────────────────────────────────────────────────────────

/// One config-file encoding strategy. A new client format is wired up by
/// mapping it to a backend in [`backend`] — existing impls stay untouched (OCP).
trait ConfigBackend {
    fn check_status(&self, path: &Path) -> InstallStatus;
    fn write_entry(
        &self,
        path: &Path,
        vaults: &[PathBuf],
        dry_run: bool,
        force: bool,
        no_edit: bool,
    ) -> Result<WriteOutcome>;
    fn remove_entry(&self, path: &Path, dry_run: bool) -> Result<bool>;
}

fn backend(format: &ConfigFormat) -> Box<dyn ConfigBackend> {
    match format {
        ConfigFormat::Standard => Box::new(JsonBackend {
            entry_path: &["mcpServers", "obsidian"],
            build: build_standard,
        }),
        ConfigFormat::ClaudeApp => Box::new(JsonBackend {
            entry_path: &["mcpServers", "obsidian"],
            build: build_claude_app,
        }),
        ConfigFormat::OpenClaw => Box::new(JsonBackend {
            entry_path: &["mcp", "servers", "obsidian"],
            build: build_openclaw,
        }),
        ConfigFormat::VSCode => Box::new(JsonBackend {
            entry_path: &["servers", "obsidian"],
            build: build_vscode,
        }),
        ConfigFormat::Amp => Box::new(JsonBackend {
            entry_path: &["amp.mcpServers", "obsidian"],
            build: build_standard,
        }),
        ConfigFormat::OpenCode => Box::new(JsonBackend {
            entry_path: &["mcp", "obsidian"],
            build: build_opencode,
        }),
        ConfigFormat::Codex => Box::new(TomlBackend),
        ConfigFormat::Goose => Box::new(YamlBackend),
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn read_config(path: &Path) -> Result<Value> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("Cannot read {}", path.display()))?;
    serde_json::from_str(&content).with_context(|| format!("Invalid JSON in {}", path.display()))
}

// ── JSON backend (6 formats differ only by entry path + entry shape) ──────────

struct JsonBackend {
    /// Key path to the obsidian entry, e.g. `["mcpServers", "obsidian"]`.
    entry_path: &'static [&'static str],
    /// Builds the entry value for this format.
    build: fn(&[String], bool) -> Value,
}

impl ConfigBackend for JsonBackend {
    fn check_status(&self, path: &Path) -> InstallStatus {
        if !path.exists() {
            return InstallStatus::FileNotFound;
        }
        let Ok(cfg) = read_config(path) else {
            return InstallStatus::NotInstalled;
        };
        if json_has_object(&cfg, self.entry_path) {
            InstallStatus::Installed
        } else {
            InstallStatus::NotInstalled
        }
    }

    fn write_entry(
        &self,
        path: &Path,
        vaults: &[PathBuf],
        dry_run: bool,
        force: bool,
        no_edit: bool,
    ) -> Result<WriteOutcome> {
        let file_exists = path.exists();
        let mut cfg = if file_exists {
            read_config(path)?
        } else {
            Value::Object(Default::default())
        };

        if json_has_object(&cfg, self.entry_path) && !force {
            return Ok(WriteOutcome::AlreadyInstalled);
        }

        let vault_strings: Vec<String> = vaults
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let entry = (self.build)(&vault_strings, no_edit);
        json_insert(&mut cfg, self.entry_path, entry);

        if dry_run {
            return Ok(WriteOutcome::DryRun {
                would_create: !file_exists,
            });
        }

        write_with_backup(path, &(serde_json::to_string_pretty(&cfg)? + "\n"))?;
        Ok(WriteOutcome::Written {
            created: !file_exists,
        })
    }

    fn remove_entry(&self, path: &Path, dry_run: bool) -> Result<bool> {
        if !path.exists() {
            return Ok(false);
        }
        let mut cfg = read_config(path)?;
        let removed = json_remove(&mut cfg, self.entry_path);
        if removed && !dry_run {
            write_with_backup(path, &(serde_json::to_string_pretty(&cfg)? + "\n"))?;
        }
        Ok(removed)
    }
}

/// Navigate `path` and report whether the final node is a JSON object.
fn json_has_object(cfg: &Value, path: &[&str]) -> bool {
    let mut cur = cfg;
    for key in path {
        match cur.get(key) {
            Some(v) => cur = v,
            None => return false,
        }
    }
    cur.is_object()
}

/// Set `entry` at `path`, creating intermediate objects as needed.
fn json_insert(cfg: &mut Value, path: &[&str], entry: Value) {
    let (last, parents) = path.split_last().expect("entry_path must be non-empty");
    let mut cur = cfg;
    for key in parents {
        if !cur[*key].is_object() {
            cur[*key] = json!({});
        }
        cur = &mut cur[*key];
    }
    cur[*last] = entry;
}

/// Remove the key at `path`. Returns whether something was removed.
fn json_remove(cfg: &mut Value, path: &[&str]) -> bool {
    let (last, parents) = path.split_last().expect("entry_path must be non-empty");
    let mut cur = cfg;
    for key in parents {
        match cur.get_mut(key) {
            Some(v) => cur = v,
            None => return false,
        }
    }
    cur.as_object_mut()
        .map(|o| o.remove(*last).is_some())
        .unwrap_or(false)
}

fn npx_args(vaults: &[String], no_edit: bool) -> Vec<Value> {
    let mut args: Vec<Value> = vec![json!("-y"), json!("obsidian-mcp-rs")];
    if no_edit {
        args.push(json!("--no-edit"));
    }
    args.extend(vaults.iter().map(|s| json!(s)));
    args
}

fn build_standard(vaults: &[String], no_edit: bool) -> Value {
    json!({ "command": "npx", "args": npx_args(vaults, no_edit) })
}

fn build_claude_app(vaults: &[String], no_edit: bool) -> Value {
    json!({ "type": "stdio", "command": "npx", "args": npx_args(vaults, no_edit) })
}

fn build_openclaw(vaults: &[String], no_edit: bool) -> Value {
    json!({ "command": "npx", "args": npx_args(vaults, no_edit), "transport": "stdio" })
}

fn build_vscode(vaults: &[String], no_edit: bool) -> Value {
    json!({ "type": "stdio", "command": "npx", "args": npx_args(vaults, no_edit) })
}

fn build_opencode(vaults: &[String], no_edit: bool) -> Value {
    // opencode merges command + args into a single array
    let mut cmd: Vec<Value> = vec![json!("npx"), json!("-y"), json!("obsidian-mcp-rs")];
    if no_edit {
        cmd.push(json!("--no-edit"));
    }
    cmd.extend(vaults.iter().map(|s| json!(s)));
    json!({ "type": "local", "command": cmd })
}

/// Create parent dirs, back up any existing file, then write `content`.
/// Shared by every backend so the dir/backup/write dance lives in one place.
fn write_with_backup(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Cannot create directory {}", parent.display()))?;
    }
    if path.exists() {
        let bak = backup_path(path);
        std::fs::copy(path, &bak)
            .with_context(|| format!("Cannot write backup to {}", bak.display()))?;
    }
    std::fs::write(path, content).with_context(|| format!("Cannot write {}", path.display()))?;
    Ok(())
}

fn backup_path(path: &Path) -> PathBuf {
    // Find a non-colliding backup name: file.json.bak / file.toml.bak / file.yaml.bak, then .bak.1, …
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("bak");
    let bak_ext = format!("{ext}.bak");
    let base = path.with_extension(&bak_ext);
    if !base.exists() {
        return base;
    }
    for i in 1u32.. {
        let candidate = path.with_extension(format!("{bak_ext}.{i}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    base
}

// ── TOML backend (Codex CLI) ──────────────────────────────────────────────────

struct TomlBackend;

impl ConfigBackend for TomlBackend {
    fn check_status(&self, path: &Path) -> InstallStatus {
        if !path.exists() {
            return InstallStatus::FileNotFound;
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            return InstallStatus::NotInstalled;
        };
        let Ok(doc) = content.parse::<toml_edit::DocumentMut>() else {
            return InstallStatus::NotInstalled;
        };
        if toml_has_obsidian(&doc) {
            InstallStatus::Installed
        } else {
            InstallStatus::NotInstalled
        }
    }

    fn write_entry(
        &self,
        path: &Path,
        vaults: &[PathBuf],
        dry_run: bool,
        force: bool,
        no_edit: bool,
    ) -> Result<WriteOutcome> {
        let file_exists = path.exists();
        let mut doc: toml_edit::DocumentMut = if file_exists {
            let content = std::fs::read_to_string(path)
                .with_context(|| format!("Cannot read {}", path.display()))?;
            content
                .parse()
                .with_context(|| format!("Invalid TOML in {}", path.display()))?
        } else {
            toml_edit::DocumentMut::new()
        };

        if toml_has_obsidian(&doc) && !force {
            return Ok(WriteOutcome::AlreadyInstalled);
        }
        if dry_run {
            return Ok(WriteOutcome::DryRun {
                would_create: !file_exists,
            });
        }

        let mut args_arr = toml_edit::Array::new();
        args_arr.push("-y");
        args_arr.push("obsidian-mcp-rs");
        if no_edit {
            args_arr.push("--no-edit");
        }
        for v in vaults {
            args_arr.push(v.to_string_lossy().as_ref());
        }

        let mut obsidian = toml_edit::Table::new();
        obsidian.insert("command", toml_edit::value("npx"));
        obsidian.insert("args", toml_edit::value(args_arr));

        if !doc.contains_key("mcp_servers") {
            doc.insert(
                "mcp_servers",
                toml_edit::Item::Table(toml_edit::Table::new()),
            );
        }
        if let Some(servers) = doc
            .get_mut("mcp_servers")
            .and_then(|item| item.as_table_mut())
        {
            servers.insert("obsidian", toml_edit::Item::Table(obsidian));
        }

        write_with_backup(path, &doc.to_string())?;
        Ok(WriteOutcome::Written {
            created: !file_exists,
        })
    }

    fn remove_entry(&self, path: &Path, dry_run: bool) -> Result<bool> {
        if !path.exists() {
            return Ok(false);
        }
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Cannot read {}", path.display()))?;
        let mut doc: toml_edit::DocumentMut = content
            .parse()
            .with_context(|| format!("Invalid TOML in {}", path.display()))?;

        let removed = doc
            .get_mut("mcp_servers")
            .and_then(|item| item.as_table_mut())
            .map(|t| t.remove("obsidian").is_some())
            .unwrap_or(false);

        if removed && !dry_run {
            write_with_backup(path, &doc.to_string())?;
        }
        Ok(removed)
    }
}

fn toml_has_obsidian(doc: &toml_edit::DocumentMut) -> bool {
    doc.get("mcp_servers")
        .and_then(|item| item.as_table())
        .map(|t| t.contains_key("obsidian"))
        .unwrap_or(false)
}

// ── YAML backend (Goose) ──────────────────────────────────────────────────────

struct YamlBackend;

impl ConfigBackend for YamlBackend {
    fn check_status(&self, path: &Path) -> InstallStatus {
        if !path.exists() {
            return InstallStatus::FileNotFound;
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            return InstallStatus::NotInstalled;
        };
        let Ok(doc) = serde_yml::from_str::<serde_yml::Value>(&content) else {
            return InstallStatus::NotInstalled;
        };
        if yaml_has_obsidian(&doc) {
            InstallStatus::Installed
        } else {
            InstallStatus::NotInstalled
        }
    }

    fn write_entry(
        &self,
        path: &Path,
        vaults: &[PathBuf],
        dry_run: bool,
        force: bool,
        no_edit: bool,
    ) -> Result<WriteOutcome> {
        let file_exists = path.exists();
        let mut doc: serde_yml::Value = if file_exists {
            let content = std::fs::read_to_string(path)
                .with_context(|| format!("Cannot read {}", path.display()))?;
            serde_yml::from_str(&content)
                .with_context(|| format!("Invalid YAML in {}", path.display()))?
        } else {
            serde_yml::Value::Mapping(serde_yml::Mapping::new())
        };

        if yaml_has_obsidian(&doc) && !force {
            return Ok(WriteOutcome::AlreadyInstalled);
        }
        if dry_run {
            return Ok(WriteOutcome::DryRun {
                would_create: !file_exists,
            });
        }

        // Build the extension entry via serde to avoid Value API uncertainty
        let goose_ext = GooseExtension {
            name: "obsidian".into(),
            ext_type: "stdio".into(),
            cmd: "npx".into(),
            args: {
                let mut a = vec!["-y".into(), "obsidian-mcp-rs".into()];
                if no_edit {
                    a.push("--no-edit".into());
                }
                a.extend(vaults.iter().map(|v| v.to_string_lossy().into_owned()));
                a
            },
            enabled: true,
            timeout: 300,
        };
        let entry: serde_yml::Value = serde_yml::to_value(&goose_ext)?;

        // Append to the extensions sequence, or create it if absent / not a list.
        if let Some(seq) = doc.get_mut("extensions").and_then(|v| v.as_sequence_mut()) {
            if force {
                seq.retain(|item| item.get("name").and_then(|n| n.as_str()) != Some("obsidian"));
            }
            seq.push(entry);
        } else if let Some(mapping) = doc.as_mapping_mut() {
            mapping.insert(
                serde_yml::Value::String("extensions".into()),
                serde_yml::Value::Sequence(vec![entry]),
            );
        }

        write_with_backup(path, &serde_yml::to_string(&doc)?)?;
        Ok(WriteOutcome::Written {
            created: !file_exists,
        })
    }

    fn remove_entry(&self, path: &Path, dry_run: bool) -> Result<bool> {
        if !path.exists() {
            return Ok(false);
        }
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Cannot read {}", path.display()))?;
        let mut doc: serde_yml::Value = serde_yml::from_str(&content)
            .with_context(|| format!("Invalid YAML in {}", path.display()))?;

        let removed = if let Some(seq) = doc.get_mut("extensions").and_then(|v| v.as_sequence_mut())
        {
            let before = seq.len();
            seq.retain(|item| item.get("name").and_then(|n| n.as_str()) != Some("obsidian"));
            seq.len() < before
        } else {
            false
        };

        if removed && !dry_run {
            write_with_backup(path, &serde_yml::to_string(&doc)?)?;
        }
        Ok(removed)
    }
}

fn yaml_has_obsidian(doc: &serde_yml::Value) -> bool {
    doc.get("extensions")
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .any(|item| item.get("name").and_then(|n| n.as_str()) == Some("obsidian"))
        })
        .unwrap_or(false)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn temp_cfg(content: &str) -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, content).unwrap();
        (dir, path)
    }

    // ── check_status ─────────────────────────────────────────────────────────

    #[test]
    fn check_status_file_not_found() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("missing.json");
        assert_eq!(
            check_status(&path, &ConfigFormat::Standard),
            InstallStatus::FileNotFound
        );
    }

    #[test]
    fn check_status_not_installed_empty_object() {
        let (_dir, path) = temp_cfg("{}");
        assert_eq!(
            check_status(&path, &ConfigFormat::Standard),
            InstallStatus::NotInstalled
        );
    }

    #[test]
    fn check_status_installed_standard() {
        let (_dir, path) = temp_cfg(r#"{"mcpServers":{"obsidian":{}}}"#);
        assert_eq!(
            check_status(&path, &ConfigFormat::Standard),
            InstallStatus::Installed
        );
    }

    #[test]
    fn check_status_installed_openclaw() {
        let (_dir, path) = temp_cfg(r#"{"mcp":{"servers":{"obsidian":{}}}}"#);
        assert_eq!(
            check_status(&path, &ConfigFormat::OpenClaw),
            InstallStatus::Installed
        );
    }

    // ── write_entry ───────────────────────────────────────────────────────────

    #[test]
    fn write_entry_creates_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("new.json");
        let vaults = vec![std::path::PathBuf::from("/vault")];
        let outcome =
            write_entry(&path, &ConfigFormat::Standard, &vaults, false, false, false).unwrap();
        assert!(matches!(outcome, WriteOutcome::Written { created: true }));
        assert!(path.exists());
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(content["mcpServers"]["obsidian"].is_object());
    }

    #[test]
    fn write_entry_already_installed_no_force() {
        let (_dir, path) = temp_cfg(r#"{"mcpServers":{"obsidian":{"command":"npx"}}}"#);
        let vaults = vec![std::path::PathBuf::from("/vault")];
        let outcome =
            write_entry(&path, &ConfigFormat::Standard, &vaults, false, false, false).unwrap();
        assert!(matches!(outcome, WriteOutcome::AlreadyInstalled));
    }

    #[test]
    fn write_entry_force_overwrites() {
        let (_dir, path) = temp_cfg(r#"{"mcpServers":{"obsidian":{"command":"old"}}}"#);
        let vaults = vec![std::path::PathBuf::from("/vault")];
        let outcome =
            write_entry(&path, &ConfigFormat::Standard, &vaults, false, true, false).unwrap();
        assert!(matches!(outcome, WriteOutcome::Written { created: false }));
    }

    #[test]
    fn write_entry_dry_run_does_not_write() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("dry.json");
        let vaults = vec![std::path::PathBuf::from("/vault")];
        let outcome =
            write_entry(&path, &ConfigFormat::Standard, &vaults, true, false, false).unwrap();
        assert!(matches!(
            outcome,
            WriteOutcome::DryRun { would_create: true }
        ));
        assert!(!path.exists());
    }

    #[test]
    fn write_entry_no_edit_flag_included_in_args() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.json");
        let vaults = vec![std::path::PathBuf::from("/vault")];
        write_entry(&path, &ConfigFormat::Standard, &vaults, false, false, true).unwrap();
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let args = content["mcpServers"]["obsidian"]["args"]
            .as_array()
            .unwrap();
        let has_no_edit = args.iter().any(|v| v.as_str() == Some("--no-edit"));
        assert!(has_no_edit, "expected --no-edit in args: {args:?}");
    }

    #[test]
    fn write_entry_no_edit_false_not_in_args() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.json");
        let vaults = vec![std::path::PathBuf::from("/vault")];
        write_entry(&path, &ConfigFormat::Standard, &vaults, false, false, false).unwrap();
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let args = content["mcpServers"]["obsidian"]["args"]
            .as_array()
            .unwrap();
        let has_no_edit = args.iter().any(|v| v.as_str() == Some("--no-edit"));
        assert!(
            !has_no_edit,
            "--no-edit should not appear when no_edit=false"
        );
    }

    #[test]
    fn write_entry_openclaw_format() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("openclaw.json");
        let vaults = vec![std::path::PathBuf::from("/vault")];
        write_entry(&path, &ConfigFormat::OpenClaw, &vaults, false, false, false).unwrap();
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(content["mcp"]["servers"]["obsidian"].is_object());
        assert_eq!(content["mcp"]["servers"]["obsidian"]["transport"], "stdio");
    }

    #[test]
    fn write_entry_claude_app_format_has_type_stdio() {
        // Claude Code configs (`.mcp.json` local + `~/.claude.json` global) both
        // use the ClaudeApp format: `mcpServers` root, entry with `type: stdio`.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".mcp.json");
        let vaults = vec![std::path::PathBuf::from("/vault")];
        write_entry(
            &path,
            &ConfigFormat::ClaudeApp,
            &vaults,
            false,
            false,
            false,
        )
        .unwrap();
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let entry = &content["mcpServers"]["obsidian"];
        assert!(entry.is_object());
        assert_eq!(entry["type"], "stdio");
        assert_eq!(entry["command"], "npx");
    }

    #[test]
    fn write_entry_creates_backup() {
        let (_dir, path) = temp_cfg(r#"{"mcpServers":{}}"#);
        let vaults = vec![std::path::PathBuf::from("/vault")];
        write_entry(&path, &ConfigFormat::Standard, &vaults, false, false, false).unwrap();
        let bak = path.with_extension("json.bak");
        assert!(bak.exists());
    }

    // ── remove_entry ──────────────────────────────────────────────────────────

    #[test]
    fn remove_entry_missing_file_returns_false() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("missing.json");
        assert!(!remove_entry(&path, &ConfigFormat::Standard, false).unwrap());
    }

    #[test]
    fn remove_entry_not_installed_returns_false() {
        let (_dir, path) = temp_cfg(r#"{"mcpServers":{}}"#);
        assert!(!remove_entry(&path, &ConfigFormat::Standard, false).unwrap());
    }

    #[test]
    fn remove_entry_removes_standard() {
        let (_dir, path) = temp_cfg(r#"{"mcpServers":{"obsidian":{"command":"npx"}}}"#);
        assert!(remove_entry(&path, &ConfigFormat::Standard, false).unwrap());
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(content["mcpServers"]["obsidian"].is_null());
    }

    #[test]
    fn remove_entry_dry_run_does_not_modify() {
        let original = r#"{"mcpServers":{"obsidian":{"command":"npx"}}}"#;
        let (_dir, path) = temp_cfg(original);
        assert!(remove_entry(&path, &ConfigFormat::Standard, true).unwrap());
        // File should be unchanged
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn remove_entry_removes_openclaw() {
        let (_dir, path) =
            temp_cfg(r#"{"mcp":{"servers":{"obsidian":{"command":"npx","transport":"stdio"}}}}"#);
        assert!(remove_entry(&path, &ConfigFormat::OpenClaw, false).unwrap());
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(content["mcp"]["servers"]["obsidian"].is_null());
    }

    // ── TOML (Codex) ──────────────────────────────────────────────────────────

    fn temp_toml(content: &str) -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, content).unwrap();
        (dir, path)
    }

    #[test]
    fn check_status_toml_file_not_found() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("missing.toml");
        assert_eq!(
            check_status(&path, &ConfigFormat::Codex),
            InstallStatus::FileNotFound
        );
    }

    #[test]
    fn check_status_toml_not_installed() {
        let (_dir, path) = temp_toml("");
        assert_eq!(
            check_status(&path, &ConfigFormat::Codex),
            InstallStatus::NotInstalled
        );
    }

    #[test]
    fn check_status_toml_installed() {
        let (_dir, path) = temp_toml("[mcp_servers.obsidian]\ncommand = \"npx\"\n");
        assert_eq!(
            check_status(&path, &ConfigFormat::Codex),
            InstallStatus::Installed
        );
    }

    #[test]
    fn write_entry_toml_creates_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let vaults = vec![std::path::PathBuf::from("/vault")];
        let outcome =
            write_entry(&path, &ConfigFormat::Codex, &vaults, false, false, false).unwrap();
        assert!(matches!(outcome, WriteOutcome::Written { created: true }));
        assert!(path.exists());
        let content: toml_edit::DocumentMut =
            std::fs::read_to_string(&path).unwrap().parse().unwrap();
        assert!(
            content
                .get("mcp_servers")
                .and_then(|s| s.as_table())
                .map(|t| t.contains_key("obsidian"))
                .unwrap_or(false)
        );
    }

    #[test]
    fn write_entry_toml_already_installed_no_force() {
        let (_dir, path) = temp_toml("[mcp_servers.obsidian]\ncommand = \"npx\"\n");
        let vaults = vec![std::path::PathBuf::from("/vault")];
        let outcome =
            write_entry(&path, &ConfigFormat::Codex, &vaults, false, false, false).unwrap();
        assert!(matches!(outcome, WriteOutcome::AlreadyInstalled));
    }

    #[test]
    fn write_entry_toml_dry_run() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let vaults = vec![std::path::PathBuf::from("/vault")];
        let outcome =
            write_entry(&path, &ConfigFormat::Codex, &vaults, true, false, false).unwrap();
        assert!(matches!(
            outcome,
            WriteOutcome::DryRun { would_create: true }
        ));
        assert!(!path.exists());
    }

    #[test]
    fn remove_entry_toml_removes() {
        let (_dir, path) = temp_toml("[mcp_servers.obsidian]\ncommand = \"npx\"\n");
        assert!(remove_entry(&path, &ConfigFormat::Codex, false).unwrap());
        let content: toml_edit::DocumentMut =
            std::fs::read_to_string(&path).unwrap().parse().unwrap();
        assert!(
            !content
                .get("mcp_servers")
                .and_then(|s| s.as_table())
                .map(|t| t.contains_key("obsidian"))
                .unwrap_or(false)
        );
    }

    #[test]
    fn remove_entry_toml_not_installed() {
        let (_dir, path) = temp_toml("");
        assert!(!remove_entry(&path, &ConfigFormat::Codex, false).unwrap());
    }

    // ── YAML (Goose) ──────────────────────────────────────────────────────────

    fn temp_yaml(content: &str) -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(&path, content).unwrap();
        (dir, path)
    }

    #[test]
    fn check_status_yaml_file_not_found() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("missing.yaml");
        assert_eq!(
            check_status(&path, &ConfigFormat::Goose),
            InstallStatus::FileNotFound
        );
    }

    #[test]
    fn check_status_yaml_not_installed() {
        let (_dir, path) = temp_yaml("extensions: []\n");
        assert_eq!(
            check_status(&path, &ConfigFormat::Goose),
            InstallStatus::NotInstalled
        );
    }

    #[test]
    fn check_status_yaml_installed() {
        let (_dir, path) = temp_yaml(
            "extensions:\n  - name: obsidian\n    type: stdio\n    cmd: npx\n    args: []\n    enabled: true\n    timeout: 300\n",
        );
        assert_eq!(
            check_status(&path, &ConfigFormat::Goose),
            InstallStatus::Installed
        );
    }

    #[test]
    fn write_entry_yaml_creates_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.yaml");
        let vaults = vec![std::path::PathBuf::from("/vault")];
        let outcome =
            write_entry(&path, &ConfigFormat::Goose, &vaults, false, false, false).unwrap();
        assert!(matches!(outcome, WriteOutcome::Written { created: true }));
        assert!(path.exists());
        let doc: serde_yml::Value =
            serde_yml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let found = doc
            .get("extensions")
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .any(|item| item.get("name").and_then(|n| n.as_str()) == Some("obsidian"))
            })
            .unwrap_or(false);
        assert!(found);
    }

    #[test]
    fn write_entry_yaml_already_installed_no_force() {
        let (_dir, path) = temp_yaml(
            "extensions:\n  - name: obsidian\n    type: stdio\n    cmd: npx\n    args: []\n    enabled: true\n    timeout: 300\n",
        );
        let vaults = vec![std::path::PathBuf::from("/vault")];
        let outcome =
            write_entry(&path, &ConfigFormat::Goose, &vaults, false, false, false).unwrap();
        assert!(matches!(outcome, WriteOutcome::AlreadyInstalled));
    }

    #[test]
    fn write_entry_yaml_dry_run() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.yaml");
        let vaults = vec![std::path::PathBuf::from("/vault")];
        let outcome =
            write_entry(&path, &ConfigFormat::Goose, &vaults, true, false, false).unwrap();
        assert!(matches!(
            outcome,
            WriteOutcome::DryRun { would_create: true }
        ));
        assert!(!path.exists());
    }

    #[test]
    fn remove_entry_yaml_removes() {
        let (_dir, path) = temp_yaml(
            "extensions:\n  - name: obsidian\n    type: stdio\n    cmd: npx\n    args: []\n    enabled: true\n    timeout: 300\n",
        );
        assert!(remove_entry(&path, &ConfigFormat::Goose, false).unwrap());
        let doc: serde_yml::Value =
            serde_yml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let found = doc
            .get("extensions")
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .any(|item| item.get("name").and_then(|n| n.as_str()) == Some("obsidian"))
            })
            .unwrap_or(false);
        assert!(!found);
    }

    #[test]
    fn remove_entry_yaml_not_installed() {
        let (_dir, path) = temp_yaml("extensions: []\n");
        assert!(!remove_entry(&path, &ConfigFormat::Goose, false).unwrap());
    }
}
