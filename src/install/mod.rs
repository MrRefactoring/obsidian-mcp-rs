pub mod clients;
mod writer;

use std::path::PathBuf;

use anyhow::{Result, bail};
use console::style;
use dialoguer::{Confirm, Input, MultiSelect, theme::ColorfulTheme};

use clap::ValueEnum as _;
use clients::{InstallTarget, all_targets, display_path, expand_tilde};
use writer::{
    InstallStatus, WriteOutcome, check_status, discard_backup, remove_entry, write_entry,
};

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
            check_vaults(&vaults)?;
            let targets = resolve_targets(kind, args.global);
            if targets.is_empty() {
                bail!("No config path found for this client on your system.");
            }
            let mut wrote_any = false;
            for target in &targets {
                wrote_any |= install_one(target, &vaults, args.dry_run, args.force, args.no_edit)?;
            }
            // Telling the user to restart when we changed nothing is how someone
            // re-running with --no-edit came away believing they were read-only.
            if wrote_any {
                println!();
                println!(
                    "{} Restart your AI client(s) for changes to take effect.",
                    style("→").cyan()
                );
            }
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
            // Saying "not set" here sent the user to `install`, which then failed
            // on the very parse error we had just swallowed. Name the problem.
            (InstallStatus::Unparseable, _) => (
                style("!").red().bold().to_string(),
                style("unreadable").red().to_string(),
                format!("{} — cannot parse this file", display_path(&t.config_path)),
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
        // "will be configured anyway" was the old behaviour, and it is how a typo
        // became a vault that is permanently, silently empty. We are still at a
        // prompt — just ask again.
        if !path.exists() {
            println!(
                "  {} No such directory: {} — check for typos and try again.",
                style("✗").red(),
                path.display()
            );
            continue;
        }
        if !path.join(".obsidian").is_dir() {
            println!(
                "  {} {} has no .obsidian/ folder — that is not a vault root. Using it anyway.",
                style("!").yellow(),
                path.display()
            );
        }
        println!("  {} {}", style("→").dim(), path.display());
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
        WriteOutcome::AlreadyInstalled { differs } if differs => {
            // The dangerous case, and the reason this branch exists: someone who
            // gave the AI write access, thought better of it, and re-ran with
            // --no-edit used to be told "already installed" and "restart your
            // client" — and walked away believing they were read-only.
            println!(
                "  {} {}  {}",
                style("!").yellow().bold(),
                target.name,
                style(format!(
                    "already installed in {pd}, but with different settings than you asked for"
                ))
                .yellow()
            );
            println!(
                "      {}",
                style("nothing was changed — re-run with --force to replace that entry").dim()
            );
            Ok(false)
        }
        WriteOutcome::AlreadyInstalled { .. } => {
            println!(
                "  {} {}  {}",
                style("○").dim(),
                target.name,
                style(format!("already installed in {pd} — nothing to do")).dim()
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
        WriteOutcome::Written { created, backup } => {
            let verb = if created { "created" } else { "updated" };
            println!(
                "  {} {}  {} {}",
                style("✓").green().bold(),
                target.name,
                style(verb).green(),
                style(&pd).dim()
            );
            // We just edited a file the user wrote. Saying where the original
            // went is the cheapest trust we can buy — and it was being made all
            // along, in silence.
            if let Some(bak) = backup {
                println!(
                    "      {}",
                    style(format!("previous config saved to {}", display_path(&bak))).dim()
                );
            }
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
        true => {
            println!(
                "  {} {}  {} {}",
                style("✓").green().bold(),
                target.name,
                style("removed").green(),
                style(&pd).dim()
            );
            // Our entry is gone, so the backup we took before writing it is of no
            // further use — and in `.cursor/` or `.vscode/` it is a file sitting
            // in the user's git repo. Uninstall should leave nothing behind.
            if let Some(bak) = discard_backup(&target.config_path) {
                println!(
                    "      {}",
                    style(format!("removed backup {}", display_path(&bak))).dim()
                );
            }
        }
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
                // Global-only clients — --global flag has no effect
                ClientKind::Claude
                | ClientKind::OpenClaw
                | ClientKind::Windsurf
                | ClientKind::Antigravity
                | ClientKind::Cline
                | ClientKind::LmStudio
                | ClientKind::Goose => true,
                // local by default; --global selects the global config
                ClientKind::ClaudeCode
                | ClientKind::Cursor
                | ClientKind::VSCode
                | ClientKind::Gemini
                | ClientKind::Kiro
                | ClientKind::Factory
                | ClientKind::Amp
                | ClientKind::OpenCode
                | ClientKind::Codex => {
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

/// Refuse to write a config pointing at a directory that isn't there, and say so
/// if it doesn't look like an Obsidian vault.
///
/// This is where the mistake is actually made, and — crucially — the only place
/// the user is still looking at a terminal. Once the path is in the config, the
/// server starts fine, every search returns nothing, and the assistant cheerfully
/// reports "I looked through your vault and found nothing there".
fn check_vaults(paths: &[PathBuf]) -> Result<()> {
    for path in paths {
        if !path.exists() {
            bail!(
                "{} does not exist.\n\
                 Check the path for typos — a config pointing at a directory that isn't there\n\
                 produces a server that starts happily and finds nothing in it.",
                path.display()
            );
        }
        if !path.is_dir() {
            bail!(
                "{} is a file, not a directory. Point this at the vault folder itself.",
                path.display()
            );
        }
        if !path.join(".obsidian").is_dir() {
            // Not fatal: pointing at one folder *inside* a vault is a legitimate
            // way to narrow what the assistant can reach.
            println!(
                "  {} {}",
                style("!").yellow().bold(),
                style(format!(
                    "{} has no .obsidian/ folder, so it is not an Obsidian vault root.\n      \
                     Continuing — but if you meant the vault itself, check the path.",
                    path.display()
                ))
                .yellow()
            );
        }
    }
    Ok(())
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
        // Use a per-OS absolute path: `/…` is absolute on Unix, `C:\…` on Windows.
        #[cfg(unix)]
        let input = "/nonexistent/path/vault";
        #[cfg(not(unix))]
        let input = r"C:\nonexistent\path\vault";
        let paths = vec![PathBuf::from(input)];
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
