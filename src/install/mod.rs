pub mod clients;
mod writer;

use std::path::PathBuf;

use anyhow::{Result, bail};
use console::style;
use dialoguer::{Confirm, Input, MultiSelect, theme::ColorfulTheme};

use clap::ValueEnum as _;
use clients::{InstallTarget, all_targets, display_path, expand_tilde};
use writer::{InstallStatus, WriteOutcome, check_status, remove_entry, write_entry};

pub use clients::ClientKind;

// ── Public arg structs (used by clap in main.rs) ──────────────────────────────

#[derive(Debug, clap::Args)]
pub struct InstallArgs {
    /// Target client. Omit to run the interactive wizard.
    pub client: Option<ClientKind>,

    /// Vault path(s) to configure (required when client is specified)
    #[arg(value_name = "VAULT_PATH")]
    pub vaults: Vec<PathBuf>,

    /// Use global config (for claude-code: ~/.claude.json; for cursor: ~/.cursor/mcp.json)
    #[arg(long, short = 'g')]
    pub global: bool,

    /// Show what would change without writing any files
    #[arg(long)]
    pub dry_run: bool,

    /// Overwrite existing entry without prompting
    #[arg(long)]
    pub force: bool,

    /// Embed --no-edit in the generated config so the server starts in read-only mode
    #[arg(long)]
    pub no_edit: bool,
}

#[derive(Debug, clap::Args)]
pub struct UninstallArgs {
    /// Target client. Omit to run the interactive wizard.
    pub client: Option<ClientKind>,

    /// Target global config (for claude-code / cursor)
    #[arg(long, short = 'g')]
    pub global: bool,

    /// Show what would change without writing any files
    #[arg(long)]
    pub dry_run: bool,

    /// Remove without a confirmation prompt
    #[arg(long)]
    pub force: bool,
}

// ── Public entry points ───────────────────────────────────────────────────────

pub fn run_install(args: InstallArgs) -> Result<()> {
    match &args.client {
        None => interactive_install(args.dry_run, args.force, args.no_edit),
        Some(kind) => {
            if args.vaults.is_empty() {
                bail!(
                    "vault path(s) required when specifying a client\n\
                     Usage: obsidian-mcp-rs install {} /path/to/vault",
                    kind.to_possible_value().unwrap().get_name()
                );
            }
            let vaults = normalize_vaults(&args.vaults);
            let targets = resolve_targets(kind, args.global);
            if targets.is_empty() {
                bail!("No config path found for this client on your system.");
            }
            for target in &targets {
                install_one(target, &vaults, args.dry_run, args.force, args.no_edit)?;
            }
            println!();
            println!(
                "{} Restart your AI client(s) for changes to take effect.",
                style("→").cyan()
            );
            Ok(())
        }
    }
}

pub fn run_uninstall(args: UninstallArgs) -> Result<()> {
    match &args.client {
        None => interactive_uninstall(args.dry_run, args.force),
        Some(kind) => {
            let targets = resolve_targets(kind, args.global);
            if targets.is_empty() {
                bail!("No config path found for this client on your system.");
            }
            for target in &targets {
                uninstall_one(target, args.dry_run, args.force)?;
            }
            Ok(())
        }
    }
}

pub fn run_list() -> Result<()> {
    let targets = all_targets();
    println!();
    println!("{}", style("Installation status:").bold());
    println!();

    let name_w = targets.iter().map(|t| t.name.len()).max().unwrap_or(20);

    for t in &targets {
        let status = check_status(&t.config_path, &t.format);
        let (icon, label, path_hint) = match (&status, t.is_local) {
            (InstallStatus::Installed, _) => (
                style("✓").green().bold().to_string(),
                style("installed").green().to_string(),
                display_path(&t.config_path),
            ),
            (InstallStatus::NotInstalled, _) => (
                style("✗").yellow().to_string(),
                style("not set  ").yellow().to_string(),
                display_path(&t.config_path),
            ),
            (InstallStatus::FileNotFound, true) => (
                style("○").dim().to_string(),
                style("no file  ").dim().to_string(),
                display_path(&t.config_path),
            ),
            (InstallStatus::FileNotFound, false) => (
                style("○").dim().to_string(),
                style("not found").dim().to_string(),
                String::new(),
            ),
        };
        println!(
            "  {} {:<width$}  {}  {}",
            icon,
            t.name,
            label,
            style(path_hint).dim(),
            width = name_w
        );
    }
    println!();
    Ok(())
}

// ── Interactive install wizard ────────────────────────────────────────────────

fn interactive_install(dry_run: bool, force: bool, no_edit: bool) -> Result<()> {
    if !console::user_attended() {
        bail!(
            "Interactive mode requires a TTY.\n\
             Usage: obsidian-mcp-rs install <client> /path/to/vault"
        );
    }

    let theme = ColorfulTheme::default();
    let targets = all_targets();

    // 1. Print detection summary (global targets only)
    println!();
    println!("{}", style("Scanning for AI clients…").bold());
    println!();
    for t in targets.iter().filter(|t| !t.is_local) {
        let (icon, note) = if t.detected {
            (
                style("✓").green().bold().to_string(),
                display_path(&t.config_path),
            )
        } else {
            (style("✗").dim().to_string(), "not found".to_string())
        };
        println!("  {} {:<36}  {}", icon, t.name, style(note).dim());
    }
    println!();

    // 2. Multi-select — pre-select detected global targets
    let labels: Vec<String> = targets.iter().map(|t| t.label()).collect();
    let defaults: Vec<bool> = targets.iter().map(|t| t.detected && !t.is_local).collect();

    let chosen = MultiSelect::with_theme(&theme)
        .with_prompt("Select where to install obsidian-mcp-rs")
        .items(&labels)
        .defaults(&defaults)
        .interact()?;

    if chosen.is_empty() {
        println!("{} Nothing selected — exiting.", style("!").yellow());
        return Ok(());
    }

    // 3. Collect vault paths
    println!();
    println!("{}", style("Vault paths").bold());
    println!("Enter the path(s) to your Obsidian vault(s). Leave blank to finish.");
    println!();

    let mut raw: Vec<PathBuf> = vec![];
    loop {
        let prompt = if raw.is_empty() {
            "Vault path".to_string()
        } else {
            "Another vault path (empty to finish)".to_string()
        };

        let input: String = Input::with_theme(&theme)
            .with_prompt(&prompt)
            .allow_empty(true)
            .interact_text()?;

        let s = input.trim();
        if s.is_empty() {
            if raw.is_empty() {
                println!(
                    "{} At least one vault path is required.",
                    style("Error:").red()
                );
                continue;
            }
            break;
        }

        let path = expand_tilde(s);
        if !path.exists() {
            println!(
                "  {} Path does not exist: {} (will be configured anyway)",
                style("⚠").yellow(),
                path.display()
            );
        } else {
            println!("  {} {}", style("→").dim(), path.display());
        }
        raw.push(path);
    }

    let vaults = normalize_vaults(&raw);

    // 4. Install
    println!();
    if dry_run {
        println!(
            "{}",
            style("Dry run — no files will be written.").yellow().bold()
        );
        println!();
    }

    let mut wrote_any = false;
    for &i in &chosen {
        if install_one(&targets[i], &vaults, dry_run, force, no_edit)? {
            wrote_any = true;
        }
    }

    if wrote_any {
        println!();
        println!(
            "{} Restart your AI client(s) for changes to take effect.",
            style("→").cyan()
        );
    }
    Ok(())
}

// ── Interactive uninstall wizard ──────────────────────────────────────────────

fn interactive_uninstall(dry_run: bool, force: bool) -> Result<()> {
    if !console::user_attended() {
        bail!(
            "Interactive mode requires a TTY.\n\
             Usage: obsidian-mcp-rs uninstall <client>"
        );
    }

    let installed: Vec<InstallTarget> = all_targets()
        .into_iter()
        .filter(|t| check_status(&t.config_path, &t.format) == InstallStatus::Installed)
        .collect();

    if installed.is_empty() {
        println!();
        println!(
            "{} obsidian-mcp-rs is not installed in any detected client config.",
            style("✓").green()
        );
        return Ok(());
    }

    println!();
    let labels: Vec<String> = installed
        .iter()
        .map(|t| format!("{} ({})", t.name, display_path(&t.config_path)))
        .collect();

    let chosen = MultiSelect::with_theme(&ColorfulTheme::default())
        .with_prompt("Select configs to remove obsidian-mcp-rs from")
        .items(&labels)
        .interact()?;

    if chosen.is_empty() {
        println!("{} Nothing selected — exiting.", style("!").yellow());
        return Ok(());
    }

    for &i in &chosen {
        uninstall_one(&installed[i], dry_run, force)?;
    }

    Ok(())
}

// ── Per-target helpers ────────────────────────────────────────────────────────

/// Returns true if a file was actually written.
fn install_one(
    target: &InstallTarget,
    vaults: &[PathBuf],
    dry_run: bool,
    force: bool,
    no_edit: bool,
) -> Result<bool> {
    let pd = display_path(&target.config_path);
    match write_entry(
        &target.config_path,
        &target.format,
        vaults,
        dry_run,
        force,
        no_edit,
    )? {
        WriteOutcome::AlreadyInstalled => {
            println!(
                "  {} {}  {}",
                style("○").dim(),
                target.name,
                style(format!(
                    "already installed in {pd} — use --force to overwrite"
                ))
                .dim()
            );
            Ok(false)
        }
        WriteOutcome::DryRun { would_create } => {
            let verb = if would_create {
                "would create"
            } else {
                "would update"
            };
            println!(
                "  {} {}  {} {}",
                style("~").yellow(),
                target.name,
                style(verb).yellow(),
                style(&pd).dim()
            );
            Ok(false)
        }
        WriteOutcome::Written { created } => {
            let verb = if created { "created" } else { "updated" };
            println!(
                "  {} {}  {} {}",
                style("✓").green().bold(),
                target.name,
                style(verb).green(),
                style(&pd).dim()
            );
            Ok(true)
        }
    }
}

fn uninstall_one(target: &InstallTarget, dry_run: bool, force: bool) -> Result<()> {
    let pd = display_path(&target.config_path);

    if !force && !dry_run && console::user_attended() {
        let ok = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(format!(
                "Remove obsidian-mcp-rs from {} ({})?",
                target.name, pd
            ))
            .default(false)
            .interact()?;
        if !ok {
            println!("  {} {} — skipped", style("○").dim(), target.name);
            return Ok(());
        }
    }

    match remove_entry(&target.config_path, &target.format, dry_run)? {
        true if dry_run => println!(
            "  {} {}  {} {}",
            style("~").yellow(),
            target.name,
            style("would remove").yellow(),
            style(&pd).dim()
        ),
        true => println!(
            "  {} {}  {} {}",
            style("✓").green().bold(),
            target.name,
            style("removed").green(),
            style(&pd).dim()
        ),
        false => println!(
            "  {} {}  {} {}",
            style("○").dim(),
            target.name,
            style("not installed").dim(),
            style(&pd).dim()
        ),
    }

    Ok(())
}

// ── Utilities ─────────────────────────────────────────────────────────────────

fn resolve_targets(kind: &ClientKind, global: bool) -> Vec<InstallTarget> {
    all_targets()
        .into_iter()
        .filter(|t| {
            if &t.kind != kind {
                return false;
            }
            match kind {
                // No local/global distinction for these clients
                ClientKind::Claude | ClientKind::OpenClaw => true,
                // local by default; --global selects the global config
                ClientKind::ClaudeCode | ClientKind::Cursor => {
                    if global {
                        !t.is_local
                    } else {
                        t.is_local
                    }
                }
            }
        })
        .collect()
}

fn normalize_vaults(paths: &[PathBuf]) -> Vec<PathBuf> {
    paths
        .iter()
        .map(|p| {
            let expanded = expand_tilde(&p.to_string_lossy());
            std::fs::canonicalize(&expanded).unwrap_or(expanded)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    // ── resolve_targets ───────────────────────────────────────────────────────

    #[test]
    fn resolve_targets_claude_always_global() {
        let targets = resolve_targets(&ClientKind::Claude, false);
        assert!(!targets.is_empty());
        assert!(targets.iter().all(|t| t.kind == ClientKind::Claude));
    }

    #[test]
    fn resolve_targets_claude_global_flag_same() {
        // Claude has no local variant, --global makes no difference
        let without = resolve_targets(&ClientKind::Claude, false);
        let with_global = resolve_targets(&ClientKind::Claude, true);
        assert_eq!(without.len(), with_global.len());
    }

    #[test]
    fn resolve_targets_claude_code_default_is_local() {
        let targets = resolve_targets(&ClientKind::ClaudeCode, false);
        assert!(targets.iter().all(|t| t.is_local));
    }

    #[test]
    fn resolve_targets_claude_code_global_is_not_local() {
        let targets = resolve_targets(&ClientKind::ClaudeCode, true);
        assert!(targets.iter().all(|t| !t.is_local));
    }

    #[test]
    fn resolve_targets_cursor_default_is_local() {
        let targets = resolve_targets(&ClientKind::Cursor, false);
        assert!(targets.iter().all(|t| t.is_local));
    }

    #[test]
    fn resolve_targets_cursor_global_is_not_local() {
        let targets = resolve_targets(&ClientKind::Cursor, true);
        assert!(targets.iter().all(|t| !t.is_local));
    }

    #[test]
    fn resolve_targets_openclaw_always_global() {
        let t_false = resolve_targets(&ClientKind::OpenClaw, false);
        let t_true = resolve_targets(&ClientKind::OpenClaw, true);
        assert_eq!(t_false.len(), t_true.len());
    }

    // ── normalize_vaults ──────────────────────────────────────────────────────

    #[test]
    fn normalize_vaults_expands_existing_path() {
        let dir = TempDir::new().unwrap();
        let paths = vec![dir.path().to_path_buf()];
        let normalized = normalize_vaults(&paths);
        // An existing path gets canonicalized (resolves symlinks etc)
        assert_eq!(normalized.len(), 1);
        assert!(normalized[0].is_absolute());
    }

    #[test]
    fn normalize_vaults_keeps_nonexistent_path() {
        let paths = vec![PathBuf::from("/nonexistent/path/vault")];
        let normalized = normalize_vaults(&paths);
        assert_eq!(normalized.len(), 1);
        assert!(normalized[0].is_absolute());
    }

    // ── run_list ──────────────────────────────────────────────────────────────

    #[test]
    fn run_list_succeeds() {
        // Just ensure it doesn't panic — output goes to stdout
        assert!(run_list().is_ok());
    }

    // ── run_install direct mode ────────────────────────────────────────────────

    #[test]
    fn run_install_requires_vaults_when_client_specified() {
        let args = InstallArgs {
            client: Some(ClientKind::Claude),
            vaults: vec![],
            global: false,
            dry_run: false,
            force: false,
            no_edit: false,
        };
        assert!(run_install(args).is_err());
    }

    #[test]
    fn run_install_direct_dry_run() {
        let dir = TempDir::new().unwrap();
        let args = InstallArgs {
            client: Some(ClientKind::ClaudeCode),
            vaults: vec![dir.path().to_path_buf()],
            global: false,
            dry_run: true,
            force: false,
            no_edit: false,
        };
        // Dry run: succeeds without writing
        assert!(run_install(args).is_ok());
    }

    // ── run_uninstall direct mode ──────────────────────────────────────────────

    #[test]
    fn run_uninstall_direct_missing_client_ok() {
        let args = UninstallArgs {
            client: Some(ClientKind::ClaudeCode),
            global: false,
            dry_run: true,
            force: true,
        };
        assert!(run_uninstall(args).is_ok());
    }

    // ── interactive mode without TTY ──────────────────────────────────────────

    #[test]
    fn run_install_interactive_no_tty_returns_err() {
        let args = InstallArgs {
            client: None, // triggers interactive
            vaults: vec![],
            global: false,
            dry_run: false,
            force: false,
            no_edit: false,
        };
        // No TTY in test environment → bail immediately
        let result = run_install(args);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("TTY"));
    }

    #[test]
    fn run_uninstall_interactive_no_tty_returns_err() {
        let args = UninstallArgs {
            client: None,
            global: false,
            dry_run: false,
            force: false,
        };
        let result = run_uninstall(args);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("TTY"));
    }

    // ── install_one / uninstall_one direct ────────────────────────────────────

    #[test]
    fn install_one_written_creates_file() {
        let dir = TempDir::new().unwrap();
        let config = dir.path().join("cursor_mcp.json");
        let target = clients::InstallTarget {
            kind: ClientKind::Cursor,
            name: "Test Cursor".into(),
            config_path: config.clone(),
            format: clients::ConfigFormat::Standard,
            detected: true,
            is_local: true,
        };
        let vaults = normalize_vaults(&[dir.path().to_path_buf()]);
        let wrote = install_one(&target, &vaults, false, false, false).unwrap();
        assert!(wrote);
        assert!(config.exists());
    }

    #[test]
    fn install_one_already_installed_returns_false() {
        let dir = TempDir::new().unwrap();
        let config = dir.path().join("conf.json");
        let vaults = normalize_vaults(&[dir.path().to_path_buf()]);
        let target = clients::InstallTarget {
            kind: ClientKind::Cursor,
            name: "Test".into(),
            config_path: config.clone(),
            format: clients::ConfigFormat::Standard,
            detected: true,
            is_local: true,
        };
        install_one(&target, &vaults, false, false, false).unwrap();
        let wrote = install_one(&target, &vaults, false, false, false).unwrap(); // already installed
        assert!(!wrote);
    }

    #[test]
    fn install_one_dry_run_returns_false() {
        let dir = TempDir::new().unwrap();
        let config = dir.path().join("conf.json");
        let vaults = normalize_vaults(&[dir.path().to_path_buf()]);
        let target = clients::InstallTarget {
            kind: ClientKind::Cursor,
            name: "Test".into(),
            config_path: config.clone(),
            format: clients::ConfigFormat::Standard,
            detected: true,
            is_local: true,
        };
        let wrote = install_one(&target, &vaults, true, false, false).unwrap();
        assert!(!wrote);
        assert!(!config.exists()); // dry_run → no file written
    }

    #[test]
    fn install_one_written_updates_existing() {
        let dir = TempDir::new().unwrap();
        let config = dir.path().join("conf.json");
        std::fs::write(&config, "{}").unwrap();
        let vaults = normalize_vaults(&[dir.path().to_path_buf()]);
        let target = clients::InstallTarget {
            kind: ClientKind::Cursor,
            name: "Test".into(),
            config_path: config.clone(),
            format: clients::ConfigFormat::Standard,
            detected: true,
            is_local: true,
        };
        let wrote = install_one(&target, &vaults, false, false, false).unwrap();
        assert!(wrote);
    }

    #[test]
    fn uninstall_one_removes_entry() {
        let dir = TempDir::new().unwrap();
        let config = dir.path().join("conf.json");
        std::fs::write(
            &config,
            r#"{"mcpServers":{"obsidian":{"command":"npx","args":[]}}}"#,
        )
        .unwrap();
        let target = clients::InstallTarget {
            kind: ClientKind::Cursor,
            name: "Test".into(),
            config_path: config.clone(),
            format: clients::ConfigFormat::Standard,
            detected: true,
            is_local: true,
        };
        // force=true to skip Confirm prompt in test
        uninstall_one(&target, false, true).unwrap();
        let val: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        assert!(val["mcpServers"]["obsidian"].is_null());
    }

    #[test]
    fn uninstall_one_not_installed_is_ok() {
        let dir = TempDir::new().unwrap();
        let config = dir.path().join("conf.json");
        std::fs::write(&config, r#"{"mcpServers":{}}"#).unwrap();
        let target = clients::InstallTarget {
            kind: ClientKind::Cursor,
            name: "Test".into(),
            config_path: config.clone(),
            format: clients::ConfigFormat::Standard,
            detected: true,
            is_local: true,
        };
        assert!(uninstall_one(&target, false, true).is_ok());
    }

    #[test]
    fn uninstall_one_dry_run() {
        let dir = TempDir::new().unwrap();
        let config = dir.path().join("conf.json");
        let orig = r#"{"mcpServers":{"obsidian":{}}}"#;
        std::fs::write(&config, orig).unwrap();
        let target = clients::InstallTarget {
            kind: ClientKind::Cursor,
            name: "Test".into(),
            config_path: config.clone(),
            format: clients::ConfigFormat::Standard,
            detected: true,
            is_local: true,
        };
        uninstall_one(&target, true, true).unwrap();
        // file unchanged
        assert_eq!(std::fs::read_to_string(&config).unwrap(), orig);
    }
}
