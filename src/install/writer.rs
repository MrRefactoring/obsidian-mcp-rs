use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Value, json};

use super::clients::ConfigFormat;

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
    if !path.exists() {
        return InstallStatus::FileNotFound;
    }
    let Ok(cfg) = read_config(path) else {
        return InstallStatus::NotInstalled;
    };
    let has_entry = match format {
        ConfigFormat::Standard | ConfigFormat::ClaudeApp => {
            cfg["mcpServers"]["obsidian"].is_object()
        }
        ConfigFormat::OpenClaw => cfg["mcp"]["servers"]["obsidian"].is_object(),
    };
    if has_entry {
        InstallStatus::Installed
    } else {
        InstallStatus::NotInstalled
    }
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
    let file_exists = path.exists();
    let mut cfg = if file_exists {
        read_config(path)?
    } else {
        Value::Object(Default::default())
    };

    let already = match format {
        ConfigFormat::Standard | ConfigFormat::ClaudeApp => {
            cfg["mcpServers"]["obsidian"].is_object()
        }
        ConfigFormat::OpenClaw => cfg["mcp"]["servers"]["obsidian"].is_object(),
    };

    if already && !force {
        return Ok(WriteOutcome::AlreadyInstalled);
    }

    let vault_strings: Vec<String> = vaults
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();

    let entry = build_entry(format, &vault_strings, no_edit);
    insert_entry(&mut cfg, format, entry);

    if dry_run {
        return Ok(WriteOutcome::DryRun {
            would_create: !file_exists,
        });
    }

    // Create parent directories if needed
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Cannot create directory {}", parent.display()))?;
    }

    // Backup existing file
    if file_exists {
        let bak = backup_path(path);
        std::fs::copy(path, &bak)
            .with_context(|| format!("Cannot write backup to {}", bak.display()))?;
    }

    let content = serde_json::to_string_pretty(&cfg)?;
    std::fs::write(path, content + "\n")
        .with_context(|| format!("Cannot write {}", path.display()))?;

    Ok(WriteOutcome::Written {
        created: !file_exists,
    })
}

/// Remove the obsidian-mcp-rs entry from the config file.
/// Returns `true` if an entry was found and removed.
pub fn remove_entry(path: &Path, format: &ConfigFormat, dry_run: bool) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let mut cfg = read_config(path)?;

    let removed = match format {
        ConfigFormat::Standard | ConfigFormat::ClaudeApp => cfg["mcpServers"]
            .as_object_mut()
            .map(|o| o.remove("obsidian").is_some())
            .unwrap_or(false),

        ConfigFormat::OpenClaw => {
            // Nested mutable access without triggering borrow checker issues
            let mut found = false;
            if let Some(mcp) = cfg.get_mut("mcp")
                && let Some(servers) = mcp.get_mut("servers")
                && let Some(obj) = servers.as_object_mut()
            {
                found = obj.remove("obsidian").is_some();
            }
            found
        }
    };

    if removed && !dry_run {
        let bak = backup_path(path);
        std::fs::copy(path, &bak)
            .with_context(|| format!("Cannot write backup to {}", bak.display()))?;
        let content = serde_json::to_string_pretty(&cfg)?;
        std::fs::write(path, content + "\n")
            .with_context(|| format!("Cannot write {}", path.display()))?;
    }

    Ok(removed)
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn read_config(path: &Path) -> Result<Value> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("Cannot read {}", path.display()))?;
    serde_json::from_str(&content).with_context(|| format!("Invalid JSON in {}", path.display()))
}

fn build_entry(format: &ConfigFormat, vault_strings: &[String], no_edit: bool) -> Value {
    let mut args: Vec<Value> = vec![json!("-y"), json!("obsidian-mcp-rs")];
    if no_edit {
        args.push(json!("--no-edit"));
    }
    args.extend(vault_strings.iter().map(|s| json!(s)));

    match format {
        ConfigFormat::Standard => json!({ "command": "npx", "args": args }),
        ConfigFormat::ClaudeApp => json!({ "type": "stdio", "command": "npx", "args": args }),
        ConfigFormat::OpenClaw => json!({ "command": "npx", "args": args, "transport": "stdio" }),
    }
}

fn insert_entry(cfg: &mut Value, format: &ConfigFormat, entry: Value) {
    match format {
        ConfigFormat::Standard | ConfigFormat::ClaudeApp => {
            if !cfg["mcpServers"].is_object() {
                cfg["mcpServers"] = json!({});
            }
            cfg["mcpServers"]["obsidian"] = entry;
        }
        ConfigFormat::OpenClaw => {
            if !cfg["mcp"].is_object() {
                cfg["mcp"] = json!({});
            }
            if !cfg["mcp"]["servers"].is_object() {
                cfg["mcp"]["servers"] = json!({});
            }
            cfg["mcp"]["servers"]["obsidian"] = entry;
        }
    }
}

fn backup_path(path: &Path) -> PathBuf {
    // Find a non-colliding backup name: file.json.bak, file.json.bak.1, …
    let base = path.with_extension("json.bak");
    if !base.exists() {
        return base;
    }
    for i in 1u32.. {
        let candidate = path.with_extension(format!("json.bak.{i}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    base
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
}
