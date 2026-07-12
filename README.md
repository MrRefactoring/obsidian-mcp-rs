<div align="center">
  <img alt="obsidian-mcp-rs logo" src="https://raw.githubusercontent.com/MrRefactoring/obsidian-mcp-rs/master/assets/logo.svg" width="120"/>

  <h1>obsidian-mcp-rs</h1>

  <a href="https://claude.ai" target="_blank" rel="noopener noreferrer"><img alt="Claude Ready" src="https://img.shields.io/badge/Claude-Ready-CC785C?style=flat-square&logo=anthropic&logoColor=white"/></a>
  <a href="https://cursor.com" target="_blank" rel="noopener noreferrer"><img alt="Cursor Ready" src="https://img.shields.io/badge/Cursor-Ready-000000?style=flat-square&logoColor=white"/></a>
  <img alt="MCP Native" src="https://img.shields.io/badge/MCP-Native-6366f1?style=flat-square"/>
  <img alt="Rust Powered" src="https://img.shields.io/badge/Rust-Powered-CE412B?style=flat-square&logo=rust&logoColor=white"/>
  <a href="https://www.npmjs.com/package/obsidian-mcp-rs" target="_blank" rel="noopener noreferrer"><img alt="npx Compatible" src="https://img.shields.io/badge/npx-Compatible-CB3837?style=flat-square&logo=npm&logoColor=white"/></a>

  <br/>
  <br/>

  <a href="https://github.com/MrRefactoring/obsidian-mcp-rs/actions/workflows/ci.yml" target="_blank" rel="noopener noreferrer"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/MrRefactoring/obsidian-mcp-rs/.github/workflows/ci.yml?branch=master&style=flat-square"/></a>
  <a href="https://www.npmjs.com/package/obsidian-mcp-rs" target="_blank" rel="noopener noreferrer"><img alt="npm version" src="https://img.shields.io/npm/v/obsidian-mcp-rs.svg?style=flat-square"/></a>
  <a href="https://www.npmjs.com/package/obsidian-mcp-rs" target="_blank" rel="noopener noreferrer"><img alt="npm downloads" src="https://img.shields.io/npm/dm/obsidian-mcp-rs.svg?style=flat-square"/></a>
  <a href="LICENSE" target="_blank" rel="noopener noreferrer"><img alt="License: MIT" src="https://img.shields.io/github/license/MrRefactoring/obsidian-mcp-rs?color=green&style=flat-square"/></a>
  <img alt="Platforms" src="https://img.shields.io/badge/platforms-macOS%20%7C%20Linux%20%7C%20Windows-blue?style=flat-square"/>
  <a href="https://codecov.io/gh/MrRefactoring/obsidian-mcp-rs" target="_blank" rel="noopener noreferrer"><img alt="Coverage" src="https://img.shields.io/codecov/c/github/mrrefactoring/obsidian-mcp-rs?style=flat-square"/></a>

  <br/>
  <br/>

  <span>Rust-based MCP server that connects your Obsidian vault to Claude, Cursor, and any AI client — single binary, zero runtime dependencies.</span>
</div>

<div align="center">

**English** | [Русский](README.ru.md)

</div>

<br/>

> [!WARNING]
> This MCP server has **full read and write access** to your Obsidian vault. It can create, edit, move, and delete notes without confirmation. Use at your own risk. Always keep backups of your vault before connecting it to an AI client.
>
> To restrict the server to read-only access, pass `--no-edit` — see [Read-only mode](#read-only-mode-no-edit).

## Setup

**The fastest way: just ask your AI agent to install it.** If you already work inside an agentic client (Claude Code, Cursor, Windsurf, …), you never touch a config file — paste one prompt and let the agent run the installer for you. Swap in your own vault path:

> Install the **obsidian-mcp-rs** MCP server for this editor. My Obsidian vault is at `~/Documents/Obsidian/MyVault`. Run the matching installer, e.g. `npx -y obsidian-mcp-rs install claude-code ~/Documents/Obsidian/MyVault` (use `cursor`, `windsurf`, `vscode`, `claude`, … for other clients), then tell me to restart the session and approve the server if the client asks.

**Claude Code** also ships a native MCP CLI, so you can instead ask it to run:

```bash
claude mcp add obsidian -- npx -y obsidian-mcp-rs ~/Documents/Obsidian/MyVault
# add `--scope user` to enable it in every project (writes ~/.claude.json)
```

> **Heads-up:** clients read MCP config at **session start**, so the agent can write it but can't hot-load it. After it installs the server, **restart** the client — and in Claude Code approve a project-scoped `.mcp.json` server via the `/mcp` panel — before the 15 tools appear. Only Claude Code has a native `mcp add` CLI; for every other client the agent just runs the `npx obsidian-mcp-rs install <client>` command above.

### Prefer a CLI? (or not using an agent)

Not inside an agentic client — e.g. **Claude Desktop**, which can't run shell commands — or just prefer to do it yourself? The interactive wizard scans for installed AI clients, lets you pick where to install, and writes the config automatically:

```bash
npx obsidian-mcp-rs install
```

Or install directly without interaction:

```bash
# Claude Desktop
npx obsidian-mcp-rs install claude ~/Documents/Obsidian/MyVault

# Claude Code – project-local (.mcp.json in current directory)
npx obsidian-mcp-rs install claude-code ~/vault

# Claude Code – global (~/.claude.json)
npx obsidian-mcp-rs install claude-code --global ~/vault

# Cursor – project-local (.cursor/mcp.json in current directory)
npx obsidian-mcp-rs install cursor ~/vault

# Cursor – global (~/.cursor/mcp.json)
npx obsidian-mcp-rs install cursor --global ~/vault

# OpenClaw
npx obsidian-mcp-rs install openclaw ~/vault

# Multiple vaults
npx obsidian-mcp-rs install claude ~/vault1 ~/vault2
```

Other management commands:

```bash
npx obsidian-mcp-rs list       # show installation status across all clients
npx obsidian-mcp-rs uninstall  # interactive removal wizard
npx obsidian-mcp-rs uninstall claude --dry-run  # preview changes without writing
```

## Features

- **15 tools** covering note CRUD, search, links, frontmatter, daily notes, directory management, and tag operations
- **Ranked search** — BM25 relevance with field boosts (a term in the title outranks the same term buried in a paragraph), returned best-first and capped so a common word can't flood the model's context
- **Link-aware moves** — renaming a note rewrites every `[[wikilink]]` and markdown link pointing at it, so moving a note never silently orphans references
- **Link graph** — `wikilinks` answers backlinks, outgoing, broken links and orphans
- **Section-scoped edits** — point `edit-note` at one heading or one `^block-id` and only those bytes are rewritten; the rest of the note is passed through untouched
- **Frontmatter access** — `frontmatter` reads and writes any YAML key, not just `tags`, and touches only the key you named
- **Multi-vault** support — pass multiple vault paths as arguments
- **Recoverable deletes** — `delete-note` moves the note to the vault's `.trash/` (as Obsidian does) rather than erasing it; a trashed note disappears from search and the link graph, but the user can still get it back
- **Daily notes** — `periodic` reads/creates daily…yearly notes using the vault's *own* Obsidian settings (name format, folder, template), so it writes to the note you actually keep
- **Vault orientation** — `vault-info` answers what tags exist, what changed recently, and how big the vault is
- **Read-only mode** — `--no-edit` removes every write tool from `tools/list` entirely, so a read-only server describes itself as one
- **Zero runtime dependencies** — single static binary, no Node.js required for execution
- **Cross-platform** — macOS (ARM64 + x64), Linux (x64 + ARM64 + musl), Windows (x64 + ARM64)
- **Tag search** via `tag:` prefix in queries
- **YAML frontmatter** tag management
- **Streamable HTTP** (optional) — `cargo install obsidian-mcp-rs --features http`, then `--http` serves several clients from one long-lived server. Validates the `Origin` header, as the MCP spec requires of local servers. stdio remains the default.
- **`npx` compatible** — runs instantly via npm

### Search

`search-vault` ranks hits with **BM25**, the same scoring family a full-text engine uses — but computed straight from the parallel vault walk, so there is no index to build, no watcher to keep in sync, and nothing to go stale when you edit a note in Obsidian.

Terms are weighted by where they occur: filename ×5, tags ×4, headings ×3, frontmatter ×2, body ×1. Rare terms count for more than common ones, so a query like `the kafka` ranks the note *about* Kafka above the note that merely says "the" a lot.

Results are paged (`limit`, default 20; `offset`) and each file quotes at most `maxMatchesPerFile` lines (default 3). Every response carries `total` and `truncated`, so the model can see that more matches exist without you paying for them in context.

Ranking answers "which notes are *about* this". Two questions it can't answer have their own arguments:

- **`regex: true`** — match a *shape* rather than words: a phone number, a `TODO(name)`, a URL. Hits are then ranked by how many lines matched, since relevance means nothing for a pattern.
- **`frontmatter: {"status": "active"}`** — keep only notes carrying those fields. A **list** field matches when it *contains* the value, so `{"tags": "work"}` finds a note with `tags: [work, urgent]`. Combine it with a query, or use it alone with an empty query as a pure metadata lookup ("every active note in this vault").

Both are computed inside the walk that already reads every note, so neither costs an extra pass.

## Performance

Vault-wide operations (`search-vault`, `rename-tag`) walk the vault with the [`ignore`](https://crates.io/crates/ignore) crate and process files in parallel via [`rayon`](https://crates.io/crates/rayon). Measured with the criterion suite in [`benches/`](benches/vault_bench.rs) on a synthetic vault, Apple Silicon (10 logical cores); "serial" is the same code pinned to one thread (`RAYON_NUM_THREADS=1`):

| Operation                  | Serial (1 thread) | Parallel  | Speedup |
| -------------------------- | ----------------- | --------- | ------- |
| Ranked search (2000 notes) | 52.8 ms           | 26.2 ms   | ~2.0×   |
| Tag search (2000 notes)    | 45.6 ms           | 24.4 ms   | ~1.9×   |
| Tag rename (500 notes)     | 84.3 ms           | 60.0 ms   | ~1.4×   |

Single-note operations (`read-note`, `create-note`, `edit-note`, …) touch one file and are unaffected. Numbers vary with core count and disk; reproduce locally with `cargo bench`.

## Installation

```bash
npm install -g obsidian-mcp-rs
```

Or use directly without installing (recommended):

```bash
npx obsidian-mcp-rs install   # wizard writes the config for you
```

## Configuration

> **Tip:** `npx obsidian-mcp-rs install` writes these configs automatically. The sections below are for manual setup or reference.

### Claude Desktop (`claude_desktop_config.json`)

```json
{
  "mcpServers": {
    "obsidian": {
      "command": "npx",
      "args": ["-y", "obsidian-mcp-rs", "/path/to/your/vault"]
    }
  }
}
```

### Multiple vaults

```json
{
  "mcpServers": {
    "obsidian": {
      "command": "npx",
      "args": [
        "-y",
        "obsidian-mcp-rs",
        "/path/to/vault1",
        "/path/to/vault2"
      ]
    }
  }
}
```

### Claude Code (`.mcp.json` / `~/.claude.json`)

Claude Code's config carries an explicit `"type": "stdio"` (Claude Desktop, above, omits it):

```json
{
  "mcpServers": {
    "obsidian": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "obsidian-mcp-rs", "~/Documents/Obsidian/MyVault"]
    }
  }
}
```

### Cursor

Add the server to Cursor's MCP settings via **Settings → MCP → Add Server**, or edit `~/.cursor/mcp.json` directly:

```json
{
  "mcpServers": {
    "obsidian": {
      "command": "npx",
      "args": ["-y", "obsidian-mcp-rs", "/path/to/your/vault"]
    }
  }
}
```

Once added, Cursor's AI will have access to all 12 vault tools. You can verify with the MCP panel in Settings.

### OpenClaw (`~/.openclaw/openclaw.json`)

```json
{
  "mcp": {
    "servers": {
      "obsidian": {
        "command": "npx",
        "args": ["-y", "obsidian-mcp-rs", "/path/to/your/vault"],
        "transport": "stdio"
      }
    }
  }
}
```

## Read-only mode (`--no-edit`)

Pass `--no-edit` to start the server in read-only mode. All write tools return an error immediately — no vault files are modified.

**Read-only tools (always available):**
- `read-note`, `search-vault`, `list-available-vaults`

**Blocked tools when `--no-edit` is set:**
- `create-note`, `edit-note`, `delete-note`, `move-note`, `create-directory`, `add-tags`, `remove-tags`, `rename-tag`

### Manual config with `--no-edit`

```json
{
  "mcpServers": {
    "obsidian": {
      "command": "npx",
      "args": ["-y", "obsidian-mcp-rs", "--no-edit", "/path/to/your/vault"]
    }
  }
}
```

### Via `install` wizard

```bash
npx obsidian-mcp-rs install claude --no-edit ~/Documents/Obsidian/MyVault
```

## Platform Support

| Platform | Architecture | Target triple |
|----------|-------------|---------------|
| macOS | ARM64 (Apple Silicon) | `aarch64-apple-darwin` |
| macOS | x64 (Intel) | `x86_64-apple-darwin` |
| Linux | x64 (glibc) | `x86_64-unknown-linux-gnu` |
| Linux | ARM64 (glibc) | `aarch64-unknown-linux-gnu` |
| Linux | x64 (musl / Alpine) | `x86_64-unknown-linux-musl` |
| Windows | x64 | `x86_64-pc-windows-msvc` |
| Windows | ARM64 | `aarch64-pc-windows-msvc` |

## Tool Reference

### `read-note`
Read the content of an existing note.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `vault` | string | ✓ | Vault name |
| `filename` | string | ✓ | Note filename (`.md` optional) |
| `folder` | string | | Subfolder path within vault |

### `create-note`
Create a new note with Markdown content.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `vault` | string | ✓ | Vault name |
| `filename` | string | ✓ | Note filename |
| `content` | string | ✓ | Markdown content |
| `folder` | string | | Subfolder path (created automatically) |

### `edit-note`
Edit an existing note.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `vault` | string | ✓ | Vault name |
| `filename` | string | ✓ | Note filename |
| `operation` | string | ✓ | `append`, `prepend`, `replace`, `find_and_replace` |
| `content` | string | ✓ | Content to apply |
| `folder` | string | | Subfolder path |
| `search` | string | | Search text (required for `find_and_replace`) |

### `delete-note`
Delete a note from the vault.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `vault` | string | ✓ | Vault name |
| `filename` | string | ✓ | Note filename |
| `folder` | string | | Subfolder path |

### `move-note`
Move or rename a note within the vault.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `vault` | string | ✓ | Vault name |
| `filename` | string | ✓ | Source filename |
| `folder` | string | | Source folder |
| `newFolder` | string | | Destination folder |
| `newFilename` | string | | New filename (same if omitted) |

### `create-directory`
Create a new directory in the vault.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `vault` | string | ✓ | Vault name |
| `path` | string | ✓ | Directory path relative to vault root |
| `recursive` | boolean | | Create parent dirs (default: `true`) |

### `search-vault`
Search notes by content, filename, or tag.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `vault` | string | ✓ | Vault name |
| `query` | string | ✓ | Search term. Use `tag:name` for tag search |
| `path` | string | | Limit search to subfolder |
| `caseSensitive` | boolean | | Default: `false` |
| `searchType` | string | | `content` (default), `filename`, `both` |

### `add-tags`
Add tags to notes in frontmatter and/or content.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `vault` | string | ✓ | Vault name |
| `files` | string[] | ✓ | Note filenames (include `.md`) |
| `tags` | string[] | ✓ | Tags to add |
| `location` | string | | `frontmatter`, `content`, `both` (default) |
| `normalize` | boolean | | Normalize tag format (default: `true`) |
| `position` | string | | `start` or `end` (default) for content tags |

### `remove-tags`
Remove tags from notes.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `vault` | string | ✓ | Vault name |
| `files` | string[] | ✓ | Note filenames |
| `tags` | string[] | ✓ | Tags to remove |

### `rename-tag`
Rename a tag across all notes in the vault.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `vault` | string | ✓ | Vault name |
| `oldTag` | string | ✓ | Current tag name |
| `newTag` | string | ✓ | New tag name |

### `list-available-vaults`
List all vaults configured for this server. Takes no parameters.

## Development

### Prerequisites

- [Rust](https://rustup.rs/) (stable, 1.94+)
- [Node.js](https://nodejs.org/) 22+ (for npm wrapper)

### Build from source

```bash
git clone https://github.com/MrRefactoring/obsidian-mcp-rs.git
cd obsidian-mcp-rs

# Build Rust binary
cargo build --release

# Build TypeScript wrapper
cd npm/obsidian-mcp-rs
npm install
npm run build

# Run directly
./target/release/obsidian-mcp-rs /path/to/your/vault
```

### Testing

```bash
cargo test               # all tests (lib + integration)
cargo test --lib         # library unit tests only
```

### Benchmarks

```bash
cargo bench                          # run the criterion suite in benches/
RAYON_NUM_THREADS=1 cargo bench      # single-threaded baseline for comparison
cargo bench --no-run                 # compile only (what CI runs)
```

### Cross-compilation

Linux cross-compilation requires [cross](https://github.com/cross-rs/cross):

```bash
cargo install cross --git https://github.com/cross-rs/cross

cross build --release --target aarch64-unknown-linux-gnu
cross build --release --target x86_64-unknown-linux-musl
```

### Environment variables

| Variable | Description |
|----------|-------------|
| `RUST_LOG` | Log level: `error`, `warn` (default), `info`, `debug`, `trace` |

Logs are written to **stderr** — stdout is reserved for MCP JSON-RPC.

## Troubleshooting

When the server runs as a background MCP process, stderr is captured by the client and may not be visible. obsidian-mcp-rs therefore writes **DEBUG logs to a file automatically** whenever it starts.

### Log file location

| Platform | Default path |
|----------|--------------|
| macOS | `~/Library/Logs/obsidian-mcp-rs/obsidian-mcp-rs.log` |
| Linux | `~/.local/share/obsidian-mcp-rs/obsidian-mcp-rs.log` |
| Windows | `%LOCALAPPDATA%\obsidian-mcp-rs\obsidian-mcp-rs.log` |

### View logs and get a bug-report link

```bash
npx obsidian-mcp-rs logs
```

Prints the log file path, the last 100 lines, and a link to open a GitHub issue.

### Verbose output to stderr

Useful when running the server manually in a terminal:

```bash
obsidian-mcp-rs --verbose /path/to/vault
# equivalent:
RUST_LOG=debug obsidian-mcp-rs /path/to/vault
```

### Custom log file

```bash
# Write to a specific path:
obsidian-mcp-rs --log-file /tmp/mcp-debug.log /path/to/vault

# Disable file logging entirely:
obsidian-mcp-rs --log-file - /path/to/vault
```

### Reporting a bug

1. Run `npx obsidian-mcp-rs logs`
2. Copy the output (or attach the log file)
3. Open an issue: <https://github.com/MrRefactoring/obsidian-mcp-rs/issues/new>

## Architecture

```
npx obsidian-mcp-rs /vault/path
          │
          ▼
  npm/obsidian-mcp-rs/bin/bin.js   ← TypeScript platform resolver
          │   detects OS + arch
          │   resolves @obsidian-mcp-rs/<platform>
          ▼
  obsidian-mcp-rs (Rust binary)   ← MCP server, stdio transport
          │
          ├── clap → CLI args parsing
          ├── VaultManager → filesystem operations
          ├── ObsidianHandler → 11 MCP tool implementations
          └── rmcp → JSON-RPC / MCP protocol
```

## Contributing

1. Fork the repository
2. Create a feature branch: `git checkout -b feat/my-feature`
3. Implement with tests
4. Ensure `cargo fmt` and `cargo clippy` pass
5. Submit a pull request

## License

MIT — see [LICENSE](LICENSE).
