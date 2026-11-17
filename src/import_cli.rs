use std::{
    path::PathBuf,
    process::{Command, Output},
};

use anyhow::{Context, Result, bail, ensure};
use clap::Parser;
use obsidian_mcp_rs::{
    install::{self, ClientKind},
    vault::{VaultManager, import::ImportRequest},
};

use crate::Cli;

#[derive(Debug, clap::Args)]
pub struct ImportArgs {
    #[doc = "MCP client whose existing connection supplies the vault and write permissions."]
    #[arg(long)]
    client: ClientKind,
    #[doc = "Read the client's global connection. Otherwise use the project connection when supported."]
    #[arg(long, short = 'g')]
    global: bool,
    #[doc = "Name of the destination vault from list_vaults."]
    #[arg(long)]
    vault: String,
    #[doc = "Local folder containing the Markdown export."]
    #[arg(long)]
    source: PathBuf,
    #[doc = "Destination folder relative to the vault root."]
    #[arg(long, default_value = ".")]
    folder: String,
    #[doc = "Source-relative path to exclude. Repeat to exclude more paths."]
    #[arg(long)]
    exclude: Vec<String>,
    #[doc = "Allow unresolved local attachments listed in the preview."]
    #[arg(long)]
    allow_missing_attachments: bool,
    #[doc = "Apply this confirmed plan ID. Omit to preview without writing."]
    #[arg(long, value_name = "PLAN_ID")]
    apply: Option<String>,
}

#[derive(Debug, clap::Args)]
pub struct OpenImporterArgs {
    #[doc = "Exact vault name as registered in the Obsidian desktop app."]
    #[arg(long)]
    vault: String,
}

pub fn import_markdown(args: ImportArgs) -> Result<()> {
    let (config, launch) = install::read_client_launch(&args.client, args.global)?;
    let connection_revision = serde_json::to_string(&(&config, &launch))?;
    let connection = Cli::try_parse_from(
        std::iter::once("obsidian-mcp-rs").chain(launch.iter().map(String::as_str)),
    )
    .context("The selected MCP connection has unsupported server arguments")?;
    ensure!(
        connection.command.is_none()
            && !connection.vaults.is_empty()
            && connection.vaults.iter().all(|path| path.is_absolute()),
        "The selected connection must start the MCP server with absolute vault paths"
    );
    let manager =
        VaultManager::new(connection.vaults).with_write_folders(&connection.write_folders)?;
    let source = args
        .source
        .to_str()
        .map_or_else(|| args.source.clone(), install::clients::expand_tilde);
    let report = manager.import_markdown(
        &ImportRequest {
            source,
            vault: args.vault,
            folder: args.folder,
            exclude: args.exclude,
            allow_missing_attachments: args.allow_missing_attachments,
            read_only: connection.no_edit,
            connection_revision,
        },
        args.apply.as_deref(),
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    if args.apply.is_some() && !report.applied {
        bail!("Import was not completed. Check the report.");
    }
    Ok(())
}

pub fn open_importer(args: OpenImporterArgs) -> Result<()> {
    ensure!(
        !args.vault.trim().is_empty() && !args.vault.chars().any(char::is_control),
        "Use the exact Obsidian vault name"
    );
    const COMMAND_ID: &str = "obsidian-importer:open-modal";
    let vault = format!("vault={}", args.vault);
    let selected = obsidian_command(&vault, &["vault", "info=name"])?;
    ensure!(
        String::from_utf8_lossy(&selected.stdout).trim() == args.vault,
        "Obsidian CLI did not confirm the requested vault. Use the exact vault name, or open the official Importer in the intended vault from the command palette."
    );
    let known_vaults = obsidian_command(&vault, &["vaults", "verbose"])?;
    let known_vaults = String::from_utf8_lossy(&known_vaults.stdout);
    ensure!(
        known_vaults
            .lines()
            .filter(|line| {
                let name = line.split('\t').next().unwrap_or("").trim();
                name.trim_start_matches("- ") == args.vault
            })
            .count()
            < 2,
        "More than one Obsidian vault has that name. Open the official Importer in the intended vault from the command palette."
    );
    let commands = obsidian_command(&vault, &["commands", "filter=obsidian-importer:"])?;
    let commands = String::from_utf8_lossy(&commands.stdout);
    ensure!(
        commands
            .lines()
            .any(|line| line.split_whitespace().next() == Some(COMMAND_ID)),
        "Importer is unavailable in this vault. Enable the official Importer under Obsidian Settings → Community plugins, then open it from the command palette."
    );
    obsidian_command(&vault, &["command", &format!("id={COMMAND_ID}")])?;
    println!(
        "Sent the command to open Obsidian Importer in '{}'. Choose the source and confirm the import in Obsidian.",
        args.vault
    );
    Ok(())
}

fn obsidian_command(vault: &str, args: &[&str]) -> Result<Output> {
    let output = Command::new("obsidian").arg(vault).args(args).output()
        .context("Obsidian CLI is unavailable. Open the official Importer from the Obsidian command palette.")?;
    let cli_error = [&output.stderr, &output.stdout]
        .into_iter()
        .find_map(|stream| {
            let text = String::from_utf8_lossy(stream);
            let message = text.trim().to_ascii_lowercase();
            (message.starts_with("error:")
                || message.starts_with("unknown command")
                || message.starts_with("command line interface is not enabled"))
            .then(|| text.into_owned())
        });
    if !output.status.success() || cli_error.is_some() {
        let detail = if output.stderr.is_empty() {
            &output.stdout
        } else {
            &output.stderr
        };
        let detail = cli_error.unwrap_or_else(|| String::from_utf8_lossy(detail).into_owned());
        bail!(
            "Obsidian CLI could not open Importer. {} Open the official Importer from the Obsidian command palette.",
            detail.trim()
        );
    }
    Ok(output)
}
