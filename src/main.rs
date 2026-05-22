use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use clap::{Parser, Subcommand};
use obsidian_mcp_rs::handler::ObsidianHandler;
use obsidian_mcp_rs::install;
use obsidian_mcp_rs::vault::VaultManager;
use rmcp::ServiceExt;

#[derive(Parser, Debug)]
#[command(
    name = "obsidian-mcp-rs",
    about = "A fast, Rust-based MCP server for Obsidian vaults",
    version,
    author,
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Vault path(s) — used when no subcommand is given to start the MCP server
    #[arg(value_name = "VAULT_PATH")]
    vaults: Vec<PathBuf>,

    /// Disable all write tools. Only read-note, search-vault, and
    /// list-available-vaults remain available. Any attempt to call a write
    /// tool returns an error.
    #[arg(long, default_value_t = false)]
    no_edit: bool,

    /// Enable verbose (debug-level) logging to stderr.
    #[arg(short = 'v', long, default_value_t = false)]
    verbose: bool,

    /// Write server logs to FILE instead of the default location.
    /// Pass '-' to disable file logging entirely.
    #[arg(long, value_name = "FILE")]
    log_file: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Install obsidian-mcp-rs into AI client config(s).
    /// Run without arguments for an interactive wizard.
    ///
    /// Examples:
    ///   obsidian-mcp-rs install
    ///   obsidian-mcp-rs install claude ~/Documents/Obsidian/MyVault
    ///   obsidian-mcp-rs install cursor --global ~/vault
    ///   obsidian-mcp-rs install claude-code ~/vault
    Install(install::InstallArgs),

    /// Remove obsidian-mcp-rs from AI client config(s).
    /// Run without arguments for an interactive wizard.
    Uninstall(install::UninstallArgs),

    /// Show installation status across all detected AI clients.
    List,

    /// Show the log file location and its most recent entries.
    /// Use this when reporting a bug.
    Logs,
}

// ── Logging setup ─────────────────────────────────────────────────────────────

/// Platform-specific default log file path.
fn default_log_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    let base = dirs::home_dir().map(|h| h.join("Library").join("Logs").join("obsidian-mcp-rs"));

    #[cfg(not(target_os = "macos"))]
    let base = dirs::data_local_dir().map(|d| d.join("obsidian-mcp-rs"));

    base.map(|d| d.join("obsidian-mcp-rs.log"))
}

/// A `MakeWriter` backed by a `Mutex<File>` so it can be shared across threads.
#[derive(Clone)]
struct FileWriter(Arc<Mutex<File>>);

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for FileWriter {
    type Writer = FileWriterGuard<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        FileWriterGuard(self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

struct FileWriterGuard<'a>(std::sync::MutexGuard<'a, File>);

impl Write for FileWriterGuard<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

/// Initialise the global tracing subscriber.
///
/// - **stderr**: WARN by default (DEBUG when `verbose = true`), overridden by `RUST_LOG`
/// - **file**: DEBUG always — captures everything for bug reports
fn setup_logging(verbose: bool, log_path: Option<PathBuf>) {
    use tracing_subscriber::{
        EnvFilter, Layer, fmt, layer::SubscriberExt, util::SubscriberInitExt,
    };

    let stderr_filter = if verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::from_default_env().add_directive(tracing::Level::WARN.into())
    };

    let stderr_layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_filter(stderr_filter);

    // Build an optional file layer — silently skip if the file can't be opened.
    let file_layer = log_path.as_ref().and_then(|path| {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()?;
        let writer = FileWriter(Arc::new(Mutex::new(file)));
        Some(
            fmt::layer()
                .with_writer(writer)
                .with_ansi(false)
                .with_filter(EnvFilter::new("debug"))
                .boxed(),
        )
    });

    tracing_subscriber::registry()
        .with(stderr_layer)
        .with(file_layer)
        .init();

    if let Some(path) = &log_path {
        eprintln!("obsidian-mcp-rs: logging to {}", path.display());
    }
}

/// Print the log file path and its most recent entries for bug reporting.
fn run_logs() -> anyhow::Result<()> {
    use console::style;

    let path = default_log_path().unwrap_or_else(|| PathBuf::from("obsidian-mcp-rs.log"));

    println!(
        "{} {}",
        style("Log file:").bold(),
        style(path.display()).cyan()
    );
    println!();

    if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        let lines: Vec<&str> = content.lines().collect();
        let tail = 100usize;
        let start = lines.len().saturating_sub(tail);

        if start > 0 {
            println!(
                "{}",
                style(format!(
                    "(showing last {} of {} lines)\n",
                    lines.len() - start,
                    lines.len()
                ))
                .dim()
            );
        }

        let sep = style("──────────────────────────────────────────────────────────").dim();
        println!("{sep}");
        for line in &lines[start..] {
            let colored = if line.contains(" ERROR") || line.contains("ERROR ") {
                style(*line).red().to_string()
            } else if line.contains(" WARN") || line.contains("WARN ") {
                style(*line).yellow().to_string()
            } else if line.contains(" DEBUG") || line.contains(" TRACE") {
                style(*line).dim().to_string()
            } else {
                (*line).to_string()
            };
            println!("{colored}");
        }
        println!("{sep}");
    } else {
        println!(
            "{}",
            style("(log file does not exist yet — start the MCP server first)").dim()
        );
        println!();
        println!("Tip: for verbose output, start the server with --verbose:");
        println!(
            "  {}",
            style("obsidian-mcp-rs --verbose /path/to/vault").cyan()
        );
    }

    println!();
    println!("{}", style("To report a bug:").bold());
    println!(
        "  {}",
        style("https://github.com/MrRefactoring/obsidian-mcp-rs/issues/new").cyan()
    );

    Ok(())
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // File logging is only enabled in server mode (no subcommand).
    let log_path = match &cli.command {
        None => match cli.log_file.as_ref() {
            Some(p) if p.as_os_str() == "-" => None,
            Some(p) => Some(p.clone()),
            None => default_log_path(),
        },
        Some(_) => None,
    };
    setup_logging(cli.verbose, log_path);

    match cli.command {
        Some(Commands::Install(args)) => install::run_install(args)?,
        Some(Commands::Uninstall(args)) => install::run_uninstall(args)?,
        Some(Commands::List) => install::run_list()?,
        Some(Commands::Logs) => run_logs()?,
        None => {
            if cli.vaults.is_empty() {
                eprintln!(
                    "error: at least one VAULT_PATH required\n\
                     \n\
                     Usage: obsidian-mcp-rs <VAULT_PATH>...\n\
                     \n\
                     For setup help, run:  obsidian-mcp-rs install\n\
                     For full help, run:   obsidian-mcp-rs --help"
                );
                std::process::exit(1);
            }
            run_server(cli.vaults, cli.no_edit).await?;
        }
    }

    Ok(())
}

async fn run_server(vaults: Vec<PathBuf>, no_edit: bool) -> anyhow::Result<()> {
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        pid = std::process::id(),
        no_edit,
        "obsidian-mcp-rs starting"
    );

    for path in &vaults {
        if !path.exists() {
            tracing::warn!(path = %path.display(), "vault path does not exist");
            eprintln!("Warning: vault path '{}' does not exist", path.display());
        } else {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
            tracing::info!(vault = name, path = %path.display(), "vault registered");
        }
    }

    let manager = VaultManager::new(vaults);
    let handler = ObsidianHandler::with_options(manager, no_edit);

    let transport = (tokio::io::stdin(), tokio::io::stdout());
    let service = handler.serve(transport).await?;
    service.waiting().await?;

    Ok(())
}
