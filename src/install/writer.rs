use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use jsonc_parser::ParseOptions;
use jsonc_parser::cst::{CstInputValue, CstObject, CstRootNode};
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
    /// The file is there but we cannot read it. Reporting this as `NotInstalled`
    /// sent the user to `install`, which then failed on the same parse error —
    /// a dead end. Say so instead.
    Unparseable,
}

pub enum WriteOutcome {
    /// An entry is already there and `--force` was not given, so we left it
    /// alone. `differs` says whether it is the entry the user just asked for:
    /// re-running with `--no-edit` used to be a silent no-op that still printed
    /// "restart your client", leaving people convinced they were read-only when
    /// their config still granted full write access.
    AlreadyInstalled {
        differs: bool,
    },
    Written {
        created: bool,
        backup: Option<PathBuf>,
    },
    DryRun {
        would_create: bool,
    },
}

/// Check whether obsidian-mcp-rs is registered in the given config file
pub fn check_status(path: &Path, format: &ConfigFormat) -> InstallStatus {
    backend(format).check_status(path)
}

/// Add (or overwrite) the obsidian-mcp-rs entry in the config file.
///
/// When `force` is true, an existing entry is replaced. When false,
/// `WriteOutcome::AlreadyInstalled { .. }` is returned and the file is left untouched.
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

/// Delete the backup taken alongside `path`, if there is one. Called after a
/// successful uninstall: our entry is gone, so the backup of the file that had
/// it is worthless, and in a project-local config it is litter in the user's
/// repo. Returns where it was.
pub fn discard_backup(path: &Path) -> Option<PathBuf> {
    let bak = backup_path(path);
    if bak.exists() && std::fs::remove_file(&bak).is_ok() {
        Some(bak)
    } else {
        None
    }
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

/// Parse a config as JSONC, keeping every byte we don't mean to change.
///
/// Comments are legal here on purpose: VS Code's `mcp.json` officially permits
/// them, and refusing to parse one turned a supported config into a dead end
/// (`list` said "not set", `install` then failed on "line 2 column 3").
fn parse_jsonc(path: &Path, text: &str) -> Result<CstRootNode> {
    CstRootNode::parse(text, &ParseOptions::default())
        .with_context(|| format!("Cannot parse {}", path.display()))
}

fn read_to_string(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("Cannot read {}", path.display()))
}

// ── JSON backend (6 formats differ only by entry path + entry shape) ──────────

struct JsonBackend {
    /// Key path to the obsidian entry, e.g. `["mcpServers", "obsidian"]`.
    entry_path: &'static [&'static str],
    /// Builds the entry value for this format.
    build: fn(&[String], bool) -> Value,
}

/// Walk `path` down the CST, creating the intermediate objects that don't exist.
/// Returns the object that should hold the entry, and the entry's key.
fn descend<'a>(root: &CstRootNode, path: &'a [&'a str]) -> (CstObject, &'a str) {
    let (last, parents) = path.split_last().expect("entry_path must be non-empty");
    let mut obj = root.object_value_or_set();
    for key in parents {
        obj = obj.object_value_or_set(key);
    }
    (obj, last)
}

impl ConfigBackend for JsonBackend {
    fn check_status(&self, path: &Path) -> InstallStatus {
        if !path.exists() {
            return InstallStatus::FileNotFound;
        }
        let Ok(text) = read_to_string(path) else {
            return InstallStatus::Unparseable;
        };
        let Ok(root) = CstRootNode::parse(&text, &ParseOptions::default()) else {
            return InstallStatus::Unparseable;
        };
        let Some(mut obj) = root.object_value() else {
            return InstallStatus::NotInstalled;
        };
        let (last, parents) = self
            .entry_path
            .split_last()
            .expect("entry_path must be non-empty");
        for key in parents {
            match obj.object_value(key) {
                Some(next) => obj = next,
                None => return InstallStatus::NotInstalled,
            }
        }
        match obj.object_value(last) {
            Some(_) => InstallStatus::Installed,
            None => InstallStatus::NotInstalled,
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
        let text = if file_exists {
            read_to_string(path)?
        } else {
            "{}\n".to_string()
        };
        let root = parse_jsonc(path, &text)?;
        let (obj, key) = descend(&root, self.entry_path);

        let vault_strings: Vec<String> = vaults
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let wanted = (self.build)(&vault_strings, no_edit);

        let existing = obj.get(key);
        if let Some(prop) = &existing
            && !force
        {
            return Ok(WriteOutcome::AlreadyInstalled {
                differs: prop.to_serde_value().as_ref() != Some(&wanted),
            });
        }

        let entry = to_cst(&wanted);

        // Only this key is touched. Every other line of the user's config —
        // their other servers, their comments, their key order, their
        // indentation — is carried through untouched. Rewriting the whole file
        // through serde would have been a fraction of the code and would have
        // reformatted a file the user did not ask us to reformat.
        match existing {
            Some(prop) => {
                prop.replace_with(key, entry);
            }
            None => {
                obj.append(key, entry);
            }
        }

        if dry_run {
            return Ok(WriteOutcome::DryRun {
                would_create: !file_exists,
            });
        }

        let backup = write_with_backup(path, &root.to_string())?;
        Ok(WriteOutcome::Written {
            created: !file_exists,
            backup,
        })
    }

    fn remove_entry(&self, path: &Path, dry_run: bool) -> Result<bool> {
        if !path.exists() {
            return Ok(false);
        }
        let text = read_to_string(path)?;
        let root = parse_jsonc(path, &text)?;
        let Some(mut obj) = root.object_value() else {
            return Ok(false);
        };
        let (last, parents) = self
            .entry_path
            .split_last()
            .expect("entry_path must be non-empty");
        for key in parents {
            match obj.object_value(key) {
                Some(next) => obj = next,
                None => return Ok(false),
            }
        }
        let Some(prop) = obj.get(last) else {
            return Ok(false);
        };
        prop.remove();
        if !dry_run {
            write_with_backup(path, &root.to_string())?;
        }
        Ok(true)
    }
}

/// A `serde_json::Value` as something the CST can splice in. The entry builders
/// stay written in `json!`, which is far easier to read than a CST literal.
fn to_cst(v: &Value) -> CstInputValue {
    match v {
        Value::Null => CstInputValue::Null,
        Value::Bool(b) => CstInputValue::Bool(*b),
        Value::Number(n) => CstInputValue::Number(n.to_string()),
        Value::String(s) => CstInputValue::String(s.clone()),
        Value::Array(a) => CstInputValue::Array(a.iter().map(to_cst).collect()),
        Value::Object(o) => {
            CstInputValue::Object(o.iter().map(|(k, v)| (k.clone(), to_cst(v))).collect())
        }
    }
}

/// The arguments `npx` is handed, in order. The single source of truth for what
/// the installed entry runs — the JSON, TOML and YAML backends all encode this.
fn npx_argv(vaults: &[PathBuf], no_edit: bool) -> Vec<String> {
    let mut args = vec!["-y".to_string(), "obsidian-mcp-rs".to_string()];
    if no_edit {
        args.push("--no-edit".to_string());
    }
    args.extend(vaults.iter().map(|v| v.to_string_lossy().into_owned()));
    args
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
///
/// Returns where the previous contents were saved, if there were any. The caller
/// is expected to *tell the user*: an installer that edits files you did not
/// write earns its trust by saying what it did and where the original went.
fn write_with_backup(path: &Path, content: &str) -> Result<Option<PathBuf>> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Cannot create directory {}", parent.display()))?;
    }
    let backup = if path.exists() {
        let bak = backup_path(path);
        std::fs::copy(path, &bak)
            .with_context(|| format!("Cannot write backup to {}", bak.display()))?;
        Some(bak)
    } else {
        None
    };
    std::fs::write(path, content).with_context(|| format!("Cannot write {}", path.display()))?;
    Ok(backup)
}

/// Where the pre-edit contents go: `mcp.json` → `mcp.json.bak`.
///
/// One backup per config, overwritten each time. It used to hunt for a free
/// `.bak.1`, `.bak.2`, … which meant every re-install left another file behind —
/// inside `.cursor/` and `.vscode/`, that is litter in the user's git repo. The
/// backup that matters is the most recent one.
fn backup_path(path: &Path) -> PathBuf {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("bak");
    path.with_extension(format!("{ext}.bak"))
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
            // Every backend encodes the same invocation, so comparing the argv is
            // what answers "is this the entry you just asked for?" — including
            // whether it carries --no-edit.
            let installed = doc
                .get("mcp_servers")
                .and_then(|s| s.get("obsidian"))
                .and_then(|o| o.get("args"))
                .and_then(|a| a.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect::<Vec<_>>()
                });
            return Ok(WriteOutcome::AlreadyInstalled {
                differs: installed.as_deref() != Some(&npx_argv(vaults, no_edit)),
            });
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

        let backup = write_with_backup(path, &doc.to_string())?;
        Ok(WriteOutcome::Written {
            created: !file_exists,
            backup,
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
            let installed = doc
                .get("extensions")
                .and_then(|e| e.as_sequence())
                .and_then(|seq| {
                    seq.iter()
                        .find(|i| i.get("name").and_then(|n| n.as_str()) == Some("obsidian"))
                })
                .and_then(|e| e.get("args"))
                .and_then(|a| a.as_sequence())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect::<Vec<_>>()
                });
            return Ok(WriteOutcome::AlreadyInstalled {
                differs: installed.as_deref() != Some(&npx_argv(vaults, no_edit)),
            });
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

        let backup = write_with_backup(path, &serde_yml::to_string(&doc)?)?;
        Ok(WriteOutcome::Written {
            created: !file_exists,
            backup,
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
        assert!(matches!(
            outcome,
            WriteOutcome::Written { created: true, .. }
        ));
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
        assert!(matches!(outcome, WriteOutcome::AlreadyInstalled { .. }));
    }

    #[test]
    fn write_entry_force_overwrites() {
        let (_dir, path) = temp_cfg(r#"{"mcpServers":{"obsidian":{"command":"old"}}}"#);
        let vaults = vec![std::path::PathBuf::from("/vault")];
        let outcome =
            write_entry(&path, &ConfigFormat::Standard, &vaults, false, true, false).unwrap();
        assert!(matches!(
            outcome,
            WriteOutcome::Written { created: false, .. }
        ));
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
        assert!(matches!(
            outcome,
            WriteOutcome::Written { created: true, .. }
        ));
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
        assert!(matches!(outcome, WriteOutcome::AlreadyInstalled { .. }));
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
        assert!(matches!(
            outcome,
            WriteOutcome::Written { created: true, .. }
        ));
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
        assert!(matches!(outcome, WriteOutcome::AlreadyInstalled { .. }));
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
