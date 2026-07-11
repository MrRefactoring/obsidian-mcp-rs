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
    /// Cursor IDE (local .cursor/mcp.json or global ~/.cursor/mcp.json)
    #[value(name = "cursor")]
    Cursor,
    /// OpenClaw
    #[value(name = "openclaw")]
    OpenClaw,
    /// Windsurf IDE (Codeium) — ~/.codeium/windsurf/mcp_config.json
    #[value(name = "windsurf")]
    Windsurf,
    /// VS Code / GitHub Copilot (local .vscode/mcp.json or global User/mcp.json)
    #[value(name = "vscode")]
    VSCode,
    /// Gemini CLI (local .gemini/settings.json or global ~/.gemini/settings.json)
    #[value(name = "gemini")]
    Gemini,
    /// Antigravity (Google AI IDE) — ~/.gemini/antigravity/mcp_config.json
    #[value(name = "antigravity")]
    Antigravity,
    /// Cline (VS Code extension) — VS Code globalStorage
    #[value(name = "cline")]
    Cline,
    /// Kiro (AWS IDE, local .kiro/settings/mcp.json or global ~/.kiro/settings/mcp.json)
    #[value(name = "kiro")]
    Kiro,
    /// LM Studio — ~/.lmstudio/mcp.json
    #[value(name = "lmstudio")]
    LmStudio,
    /// Factory (factory.ai droids, local .factory/mcp.json or global ~/.factory/mcp.json)
    #[value(name = "factory")]
    Factory,
    /// Amp coding assistant (local .amp/settings.json or global ~/.config/amp/settings.json)
    #[value(name = "amp")]
    Amp,
    /// opencode (local .opencode.json or global ~/.opencode.json)
    #[value(name = "opencode")]
    OpenCode,
    /// Codex CLI (local .codex/config.toml or global ~/.codex/config.toml)
    #[value(name = "codex")]
    Codex,
    /// Goose (Block) — ~/.config/goose/config.yaml
    #[value(name = "goose")]
    Goose,
}

/// How the MCP entry is encoded in a given config file
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigFormat {
    /// { "mcpServers": { "obsidian": { "command": ..., "args": [...] } } }
    Standard,
    /// Claude Code stdio entry (`.mcp.json` + `~/.claude.json`): `mcpServers`
    /// root, entry carries `"type": "stdio"`
    ClaudeApp,
    /// { "mcp": { "servers": { "obsidian": { "command": ..., "args": [...], "transport": "stdio" } } } }
    OpenClaw,
    /// VS Code / GitHub Copilot — { "servers": { "obsidian": { "type": "stdio", "command": ..., "args": [...] } } }
    VSCode,
    /// Amp — top-level dotted key: { "amp.mcpServers": { "obsidian": { "command": ..., "args": [...] } } }
    Amp,
    /// opencode — { "mcp": { "obsidian": { "type": "local", "command": ["npx", ...all-args] } } }
    OpenCode,
    /// Codex CLI — TOML: [mcp_servers.obsidian] with command/args keys
    Codex,
    /// Goose (Block) — YAML: extensions list with name/type/cmd/args/enabled/timeout
    Goose,
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
        // Claude Code's `.mcp.json` schema carries `"type": "stdio"` — same
        // entry shape as the global `~/.claude.json` writer below.
        format: ConfigFormat::ClaudeApp,
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

    // ── Windsurf ──────────────────────────────────────────────────────────────
    if let Some(path) = windsurf_config_path() {
        let detected = path.parent().is_some_and(|p| p.exists());
        out.push(InstallTarget {
            kind: ClientKind::Windsurf,
            name: "Windsurf".into(),
            config_path: path,
            format: ConfigFormat::Standard,
            detected,
            is_local: false,
        });
    }

    // ── VS Code – local (.vscode/mcp.json in CWD) ─────────────────────────────
    out.push(InstallTarget {
        kind: ClientKind::VSCode,
        name: "VS Code / Copilot – local (.vscode/mcp.json)".into(),
        config_path: std::env::current_dir()
            .unwrap_or_default()
            .join(".vscode/mcp.json"),
        format: ConfigFormat::VSCode,
        detected: true,
        is_local: true,
    });

    // ── VS Code – global (User/mcp.json) ──────────────────────────────────────
    if let Some(path) = vscode_global_config_path() {
        let detected = path.parent().is_some_and(|p| p.exists());
        out.push(InstallTarget {
            kind: ClientKind::VSCode,
            name: "VS Code / Copilot – global".into(),
            config_path: path,
            format: ConfigFormat::VSCode,
            detected,
            is_local: false,
        });
    }

    // ── Gemini CLI – local (.gemini/settings.json in CWD) ────────────────────
    out.push(InstallTarget {
        kind: ClientKind::Gemini,
        name: "Gemini CLI – local (.gemini/settings.json)".into(),
        config_path: std::env::current_dir()
            .unwrap_or_default()
            .join(".gemini/settings.json"),
        format: ConfigFormat::Standard,
        detected: true,
        is_local: true,
    });

    // ── Gemini CLI – global (~/.gemini/settings.json) ─────────────────────────
    if let Some(path) = gemini_global_config_path() {
        let detected = path.parent().is_some_and(|p| p.exists());
        out.push(InstallTarget {
            kind: ClientKind::Gemini,
            name: "Gemini CLI – global (~/.gemini/settings.json)".into(),
            config_path: path,
            format: ConfigFormat::Standard,
            detected,
            is_local: false,
        });
    }

    // ── Antigravity ───────────────────────────────────────────────────────────
    if let Some(path) = antigravity_config_path() {
        let detected = path.parent().is_some_and(|p| p.exists());
        out.push(InstallTarget {
            kind: ClientKind::Antigravity,
            name: "Antigravity".into(),
            config_path: path,
            format: ConfigFormat::Standard,
            detected,
            is_local: false,
        });
    }

    // ── Cline (VS Code extension) ─────────────────────────────────────────────
    if let Some(path) = cline_config_path() {
        let detected = path.parent().is_some_and(|p| p.exists());
        out.push(InstallTarget {
            kind: ClientKind::Cline,
            name: "Cline".into(),
            config_path: path,
            format: ConfigFormat::Standard,
            detected,
            is_local: false,
        });
    }

    // ── Kiro – local (.kiro/settings/mcp.json in CWD) ─────────────────────────
    out.push(InstallTarget {
        kind: ClientKind::Kiro,
        name: "Kiro – local (.kiro/settings/mcp.json)".into(),
        config_path: std::env::current_dir()
            .unwrap_or_default()
            .join(".kiro/settings/mcp.json"),
        format: ConfigFormat::Standard,
        detected: true,
        is_local: true,
    });

    // ── Kiro – global (~/.kiro/settings/mcp.json) ─────────────────────────────
    if let Some(path) = kiro_global_config_path() {
        let detected = path.parent().is_some_and(|p| p.exists());
        out.push(InstallTarget {
            kind: ClientKind::Kiro,
            name: "Kiro – global (~/.kiro/settings/mcp.json)".into(),
            config_path: path,
            format: ConfigFormat::Standard,
            detected,
            is_local: false,
        });
    }

    // ── LM Studio ─────────────────────────────────────────────────────────────
    if let Some(path) = lmstudio_config_path() {
        let detected = path.parent().is_some_and(|p| p.exists());
        out.push(InstallTarget {
            kind: ClientKind::LmStudio,
            name: "LM Studio".into(),
            config_path: path,
            format: ConfigFormat::Standard,
            detected,
            is_local: false,
        });
    }

    // ── Factory – local (.factory/mcp.json in CWD) ───────────────────────────
    out.push(InstallTarget {
        kind: ClientKind::Factory,
        name: "Factory – local (.factory/mcp.json)".into(),
        config_path: std::env::current_dir()
            .unwrap_or_default()
            .join(".factory/mcp.json"),
        format: ConfigFormat::Standard,
        detected: true,
        is_local: true,
    });

    // ── Factory – global (~/.factory/mcp.json) ────────────────────────────────
    if let Some(path) = factory_global_config_path() {
        let detected = path.parent().is_some_and(|p| p.exists());
        out.push(InstallTarget {
            kind: ClientKind::Factory,
            name: "Factory – global (~/.factory/mcp.json)".into(),
            config_path: path,
            format: ConfigFormat::Standard,
            detected,
            is_local: false,
        });
    }

    // ── Amp – local (.amp/settings.json in CWD) ───────────────────────────────
    out.push(InstallTarget {
        kind: ClientKind::Amp,
        name: "Amp – local (.amp/settings.json)".into(),
        config_path: std::env::current_dir()
            .unwrap_or_default()
            .join(".amp/settings.json"),
        format: ConfigFormat::Amp,
        detected: true,
        is_local: true,
    });

    // ── Amp – global (~/.config/amp/settings.json) ────────────────────────────
    if let Some(path) = amp_global_config_path() {
        let detected = path.parent().is_some_and(|p| p.exists());
        out.push(InstallTarget {
            kind: ClientKind::Amp,
            name: "Amp – global (~/.config/amp/settings.json)".into(),
            config_path: path,
            format: ConfigFormat::Amp,
            detected,
            is_local: false,
        });
    }

    // ── opencode – local (.opencode.json in CWD) ──────────────────────────────
    out.push(InstallTarget {
        kind: ClientKind::OpenCode,
        name: "opencode – local (.opencode.json)".into(),
        config_path: std::env::current_dir()
            .unwrap_or_default()
            .join(".opencode.json"),
        format: ConfigFormat::OpenCode,
        detected: true,
        is_local: true,
    });

    // ── opencode – global (~/.opencode.json) ──────────────────────────────────
    if let Some(path) = opencode_global_config_path() {
        out.push(InstallTarget {
            kind: ClientKind::OpenCode,
            name: "opencode – global (~/.opencode.json)".into(),
            config_path: path.clone(),
            format: ConfigFormat::OpenCode,
            detected: path.exists(),
            is_local: false,
        });
    }

    // ── Codex CLI – local (.codex/config.toml in CWD) ─────────────────────────
    out.push(InstallTarget {
        kind: ClientKind::Codex,
        name: "Codex CLI – local (.codex/config.toml)".into(),
        config_path: std::env::current_dir()
            .unwrap_or_default()
            .join(".codex/config.toml"),
        format: ConfigFormat::Codex,
        detected: true,
        is_local: true,
    });

    // ── Codex CLI – global (~/.codex/config.toml) ─────────────────────────────
    if let Some(path) = codex_global_config_path() {
        let detected = path.parent().is_some_and(|p| p.exists());
        out.push(InstallTarget {
            kind: ClientKind::Codex,
            name: "Codex CLI – global (~/.codex/config.toml)".into(),
            config_path: path,
            format: ConfigFormat::Codex,
            detected,
            is_local: false,
        });
    }

    // ── Goose (Block) ─────────────────────────────────────────────────────────
    if let Some(path) = goose_config_path() {
        let detected = path.parent().is_some_and(|p| p.exists());
        out.push(InstallTarget {
            kind: ClientKind::Goose,
            name: "Goose".into(),
            config_path: path,
            format: ConfigFormat::Goose,
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

// ── Config path resolvers (new clients) ──────────────────────────────────────

pub fn windsurf_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".codeium/windsurf/mcp_config.json"))
}

pub fn vscode_global_config_path() -> Option<PathBuf> {
    // macOS: ~/Library/Application Support/Code/User/mcp.json
    // Linux: ~/.config/Code/User/mcp.json
    // Windows: %APPDATA%\Code\User\mcp.json
    dirs::config_dir().map(|c| c.join("Code/User/mcp.json"))
}

pub fn gemini_global_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".gemini/settings.json"))
}

pub fn antigravity_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".gemini/antigravity/mcp_config.json"))
}

pub fn cline_config_path() -> Option<PathBuf> {
    // VS Code extension global storage
    // macOS: ~/Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json
    // Linux: ~/.config/Code/User/globalStorage/…
    // Windows: %APPDATA%\Code\User\globalStorage\…
    dirs::config_dir().map(|c| {
        c.join("Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json")
    })
}

pub fn kiro_global_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".kiro/settings/mcp.json"))
}

pub fn lmstudio_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".lmstudio/mcp.json"))
}

pub fn factory_global_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".factory/mcp.json"))
}

pub fn amp_global_config_path() -> Option<PathBuf> {
    // Uses XDG config dir: ~/.config/amp/settings.json on Linux/macOS
    // On macOS dirs::config_dir() returns ~/Library/Application Support — use home/.config instead
    #[cfg(target_os = "macos")]
    return dirs::home_dir().map(|h| h.join(".config/amp/settings.json"));
    #[cfg(not(target_os = "macos"))]
    return dirs::config_dir().map(|c| c.join("amp/settings.json"));
}

pub fn opencode_global_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".opencode.json"))
}

pub fn codex_global_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".codex/config.toml"))
}

pub fn goose_config_path() -> Option<PathBuf> {
    // Goose stores config at ~/.config/goose/ on macOS and Linux
    // (does not use ~/Library/Application Support on macOS)
    #[cfg(target_os = "macos")]
    return dirs::home_dir().map(|h| h.join(".config/goose/config.yaml"));
    #[cfg(not(target_os = "macos"))]
    return dirs::config_dir().map(|c| c.join("goose/config.yaml"));
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Expand leading `~` to the user's home directory
pub fn expand_tilde(s: &str) -> PathBuf {
    if s == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    }
    if let Some(rest) = s.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(s)
}

/// Shorten an absolute path to a `~/…` form for display
pub fn display_path(path: &std::path::Path) -> String {
    if let Some(home) = dirs::home_dir()
        && let Ok(rel) = path.strip_prefix(&home)
    {
        return format!("~/{}", rel.display());
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
        let local = targets
            .iter()
            .find(|t| t.kind == ClientKind::ClaudeCode && t.is_local);
        assert!(local.is_some());
    }

    #[test]
    fn all_targets_includes_cursor_local() {
        let targets = all_targets();
        let local = targets
            .iter()
            .find(|t| t.kind == ClientKind::Cursor && t.is_local);
        assert!(local.is_some());
    }

    #[test]
    fn all_targets_local_ones_are_always_detected() {
        let targets = all_targets();
        for t in targets.iter().filter(|t| t.is_local) {
            assert!(
                t.detected,
                "local target {} should always be detected",
                t.name
            );
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
