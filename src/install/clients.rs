use std::path::PathBuf;

/// Which AI client to target
#[derive(Debug, Clone, PartialEq, Eq, clap::ValueEnum)]
pub enum ClientKind {
    /// Claude Desktop application
    #[value(name = "claude")]
    Claude,
    /// Claude Code CLI
    #[value(name = "claude-code")]
    ClaudeCode,
    /// Cursor IDE
    #[value(name = "cursor")]
    Cursor,
    /// OpenClaw
    #[value(name = "openclaw")]
    OpenClaw,
}

/// How the MCP entry is encoded in a given config file
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigFormat {
    /// { "mcpServers": { "obsidian": { "command": ..., "args": [...] } } }
    Standard,
    /// ~/.claude.json: same root but entry has "type": "stdio"
    ClaudeApp,
    /// { "mcp": { "servers": { "obsidian": { "command": ..., "args": [...], "transport": "stdio" } } } }
    OpenClaw,
}

/// One concrete install location (a specific config file)
#[derive(Debug, Clone)]
pub struct InstallTarget {
    pub kind: ClientKind,
    /// Display name shown to the user
    pub name: String,
    /// Path to the config file (may not exist yet)
    pub config_path: PathBuf,
    pub format: ConfigFormat,
    /// True when the client application directory was found on disk
    pub detected: bool,
    /// True for project-local configs (.mcp.json, .cursor/mcp.json in CWD)
    pub is_local: bool,
}

impl InstallTarget {
    /// Label for interactive selection UI
    pub fn label(&self) -> String {
        if !self.is_local && !self.detected {
            format!("{} (not found)", self.name)
        } else {
            self.name.clone()
        }
    }
}

/// Return all possible install targets for this machine
pub fn all_targets() -> Vec<InstallTarget> {
    let mut out = vec![];

    // ── Claude Desktop ─────────────────────────────────────────────────────────
    if let Some(path) = claude_desktop_config_path() {
        let detected = path.parent().is_some_and(|p| p.exists());
        out.push(InstallTarget {
            kind: ClientKind::Claude,
            name: "Claude Desktop".into(),
            config_path: path,
            format: ConfigFormat::Standard,
            detected,
            is_local: false,
        });
    }

    // ── Claude Code – local (.mcp.json in CWD) ────────────────────────────────
    out.push(InstallTarget {
        kind: ClientKind::ClaudeCode,
        name: "Claude Code – local (.mcp.json)".into(),
        config_path: std::env::current_dir()
            .unwrap_or_default()
            .join(".mcp.json"),
        format: ConfigFormat::Standard,
        detected: true, // always possible to create
        is_local: true,
    });

    // ── Claude Code – global (~/.claude.json) ────────────────────────────────
    if let Some(path) = claude_app_config_path() {
        let detected = path.exists();
        out.push(InstallTarget {
            kind: ClientKind::ClaudeCode,
            name: "Claude Code – global (~/.claude.json)".into(),
            config_path: path,
            format: ConfigFormat::ClaudeApp,
            detected,
            is_local: false,
        });
    }

    // ── Cursor – local (.cursor/mcp.json in CWD) ─────────────────────────────
    out.push(InstallTarget {
        kind: ClientKind::Cursor,
        name: "Cursor – local (.cursor/mcp.json)".into(),
        config_path: std::env::current_dir()
            .unwrap_or_default()
            .join(".cursor/mcp.json"),
        format: ConfigFormat::Standard,
        detected: true,
        is_local: true,
    });

    // ── Cursor – global (~/.cursor/mcp.json) ─────────────────────────────────
    if let Some(path) = cursor_global_config_path() {
        let detected = path.parent().is_some_and(|p| p.exists());
        out.push(InstallTarget {
            kind: ClientKind::Cursor,
            name: "Cursor – global (~/.cursor/mcp.json)".into(),
            config_path: path,
            format: ConfigFormat::Standard,
            detected,
            is_local: false,
        });
    }

    // ── OpenClaw ──────────────────────────────────────────────────────────────
    if let Some(path) = openclaw_config_path() {
        let detected = path.parent().is_some_and(|p| p.exists());
        out.push(InstallTarget {
            kind: ClientKind::OpenClaw,
            name: "OpenClaw".into(),
            config_path: path,
            format: ConfigFormat::OpenClaw,
            detected,
            is_local: false,
        });
    }

    out
}

// ── Config path resolvers ──────────────────────────────────────────────────────

pub fn claude_desktop_config_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir()
            .map(|h| h.join("Library/Application Support/Claude/claude_desktop_config.json"))
    }
    #[cfg(target_os = "windows")]
    {
        dirs::config_dir().map(|c| c.join("Claude/claude_desktop_config.json"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        dirs::config_dir().map(|c| c.join("Claude/claude_desktop_config.json"))
    }
}

/// ~/.claude.json — the Claude desktop/web app global MCP config
pub fn claude_app_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude.json"))
}

pub fn cursor_global_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".cursor/mcp.json"))
}

pub fn openclaw_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".openclaw/openclaw.json"))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Expand leading `~` to the user's home directory
pub fn expand_tilde(s: &str) -> PathBuf {
    if s == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    }
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(s)
}

/// Shorten an absolute path to a `~/…` form for display
pub fn display_path(path: &std::path::Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rel) = path.strip_prefix(&home) {
            return format!("~/{}", rel.display());
        }
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_tilde_alone() {
        let result = expand_tilde("~");
        // Should not literally be "~"
        assert_ne!(result.to_string_lossy(), "~");
    }

    #[test]
    fn expand_tilde_with_path() {
        let result = expand_tilde("~/documents/vault");
        let s = result.to_string_lossy();
        assert!(!s.starts_with('~'));
        assert!(s.ends_with("documents/vault"));
    }

    #[test]
    fn expand_tilde_no_tilde() {
        let result = expand_tilde("/absolute/path");
        assert_eq!(result, std::path::PathBuf::from("/absolute/path"));
    }

    #[test]
    fn expand_tilde_relative_no_tilde() {
        let result = expand_tilde("relative/path");
        assert_eq!(result, std::path::PathBuf::from("relative/path"));
    }

    #[test]
    fn display_path_inside_home() {
        if let Some(home) = dirs::home_dir() {
            let p = home.join("Documents/vault");
            let s = display_path(&p);
            assert!(s.starts_with("~/"));
            assert!(s.ends_with("Documents/vault"));
        }
    }

    #[test]
    fn display_path_outside_home() {
        let p = std::path::PathBuf::from("/tmp/vault");
        let s = display_path(&p);
        assert_eq!(s, "/tmp/vault");
    }

    #[test]
    fn all_targets_returns_non_empty() {
        let targets = all_targets();
        assert!(!targets.is_empty());
    }

    #[test]
    fn all_targets_includes_claude_desktop() {
        let targets = all_targets();
        assert!(targets.iter().any(|t| t.kind == ClientKind::Claude));
    }

    #[test]
    fn all_targets_includes_claude_code_local() {
        let targets = all_targets();
        let local = targets.iter().find(|t| t.kind == ClientKind::ClaudeCode && t.is_local);
        assert!(local.is_some());
    }

    #[test]
    fn all_targets_includes_cursor_local() {
        let targets = all_targets();
        let local = targets.iter().find(|t| t.kind == ClientKind::Cursor && t.is_local);
        assert!(local.is_some());
    }

    #[test]
    fn all_targets_local_ones_are_always_detected() {
        let targets = all_targets();
        for t in targets.iter().filter(|t| t.is_local) {
            assert!(t.detected, "local target {} should always be detected", t.name);
        }
    }

    #[test]
    fn install_target_label_appends_not_found() {
        let t = InstallTarget {
            kind: ClientKind::Claude,
            name: "Claude Desktop".into(),
            config_path: std::path::PathBuf::from("/nonexistent"),
            format: ConfigFormat::Standard,
            detected: false,
            is_local: false,
        };
        assert!(t.label().contains("not found"));
    }

    #[test]
    fn install_target_label_no_suffix_when_detected() {
        let t = InstallTarget {
            kind: ClientKind::Claude,
            name: "Claude Desktop".into(),
            config_path: std::path::PathBuf::from("/some/path"),
            format: ConfigFormat::Standard,
            detected: true,
            is_local: false,
        };
        assert_eq!(t.label(), "Claude Desktop");
    }

    #[test]
    fn install_target_label_no_suffix_when_local() {
        let t = InstallTarget {
            kind: ClientKind::Cursor,
            name: "Cursor local".into(),
            config_path: std::path::PathBuf::from(".cursor/mcp.json"),
            format: ConfigFormat::Standard,
            detected: false, // local ones have detected=true normally but test the branch
            is_local: true,
        };
        // is_local=true, so no "not found" suffix even if detected=false
        assert_eq!(t.label(), "Cursor local");
    }
}
