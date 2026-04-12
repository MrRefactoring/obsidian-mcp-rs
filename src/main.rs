#![allow(dead_code)]
mod error;
mod handler;
mod tools;
mod vault;

use std::path::PathBuf;

use clap::Parser;
use handler::ObsidianHandler;
use rmcp::ServiceExt;
use vault::VaultManager;

#[derive(Parser, Debug)]
#[command(
    name = "obsidian-mcp-rs",
    about = "A fast, Rust-based MCP server for Obsidian vaults",
    version,
    author
)]
struct Args {
    /// One or more paths to Obsidian vault directories
    #[arg(required = true, value_name = "VAULT_PATH")]
    vaults: Vec<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::WARN.into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();

    for path in &args.vaults {
        if !path.exists() {
            eprintln!("Warning: vault path '{}' does not exist", path.display());
        }
    }

    let manager = VaultManager::new(args.vaults);
    let handler = ObsidianHandler::new(manager);

    let transport = (tokio::io::stdin(), tokio::io::stdout());
    let service = handler.serve(transport).await?;
    service.waiting().await?;

    Ok(())
}
