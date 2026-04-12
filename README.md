# obsidian-mcp-rs — Rust MCP Server for Obsidian Vaults

[![CI](https://github.com/MrRefactoring/obsidian-mcp-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/MrRefactoring/obsidian-mcp-rs/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/obsidian-mcp-rs)](https://www.npmjs.com/package/obsidian-mcp-rs)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Platforms](https://img.shields.io/badge/platforms-macOS%20%7C%20Linux%20%7C%20Windows-blue)](#platform-support)

A fast, production-ready **MCP (Model Context Protocol) server** for [Obsidian](https://obsidian.md) vaults — built in **Rust** for reliability, speed, and low memory footprint. Connect your Obsidian knowledge base to **Claude**, **Cursor**, or any MCP-compatible AI client.

Works as a drop-in replacement for `obsidian-mcp` with identical tool names and parameters.

## Features

- **12 tools** covering note CRUD, search, directory management, and tag operations
- **Multi-vault** support — pass multiple vault paths as arguments
- **Zero runtime dependencies** — single static binary, no Node.js required for execution
- **Cross-platform** — macOS (ARM64 + x64), Linux (x64 + ARM64 + musl), Windows (x64 + ARM64)
- **Tag search** via `tag:` prefix in queries
- **YAML frontmatter** tag management
- **`npx` compatible** — runs instantly via npm

## Installation

```bash
npx obsidian-mcp-rs /path/to/your/vault
```

Or install globally:

```bash
npm install -g obsidian-mcp-rs
obsidian-mcp-rs /path/to/your/vault
```

## Configuration

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

### Claude Code / CLAUDE.md

```json
{
  "mcpServers": {
    "obsidian": {
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

Once added, Cursor's AI will have access to all 11 vault tools. You can verify with the MCP panel in Settings.

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
| `operation` | string | ✓ | `append`, `prepend`, or `replace` |
| `content` | string | ✓ | Content to apply |
| `folder` | string | | Subfolder path |

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

- [Rust](https://rustup.rs/) (stable, 1.75+)
- [Node.js](https://nodejs.org/) 18+ (for npm wrapper)

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
cargo test
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
          ├── ObsidianHandler → 12 MCP tool implementations
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
