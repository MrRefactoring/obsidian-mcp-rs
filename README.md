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

### Ask your AI to connect Obsidian

```text
Connect my Obsidian vault to this AI client. Follow https://github.com/MrRefactoring/obsidian-mcp-rs/blob/master/INSTALL.md. Ask me for the vault path and use my preferred language.
```

Official sources: [GitHub Releases](https://github.com/MrRefactoring/obsidian-mcp-rs/releases), [npm](https://www.npmjs.com/package/obsidian-mcp-rs), and [crates.io](https://crates.io/crates/obsidian-mcp-rs).

## Setup

Choose npm or Cargo to install the server. Both paths run the same installer, which copies the server to a fixed location and configures your client. Node is not needed to run the installed server.

### Cargo

Install from source with [Rust and Cargo](https://www.rust-lang.org/tools/install), then configure your client:

```bash
cargo install --locked obsidian-mcp-rs
obsidian-mcp-rs install claude-code ~/Documents/Obsidian/MyVault
```

Ensure Cargo’s bin directory is on your `PATH`. Replace `claude-code` and the vault path for your setup. You can also run `obsidian-mcp-rs install` for the interactive wizard.

If you have [cargo-binstall](https://github.com/cargo-bins/cargo-binstall), use `cargo binstall obsidian-mcp-rs` to install a prebuilt release instead of compiling. Then run the same `obsidian-mcp-rs install` command above.

### npm

The npm installer requires [Node.js](https://nodejs.org/) 22 or newer.

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
npx obsidian-mcp-rs list       # installation status across all clients, and which server version is installed
npx obsidian-mcp-rs uninstall  # interactive removal wizard
npx obsidian-mcp-rs uninstall claude --dry-run  # preview changes without writing
```

### What `install` actually writes

It copies the server binary to a fixed per-user location and writes **that absolute path** into your client config:

| Platform | Installed server |
|----------|------------------|
| macOS    | `~/Library/Application Support/obsidian-mcp-rs/bin/obsidian-mcp-rs` |
| Linux    | `~/.local/share/obsidian-mcp-rs/bin/obsidian-mcp-rs` |
| Windows  | `%LOCALAPPDATA%\obsidian-mcp-rs\bin\obsidian-mcp-rs.exe` |

Your config therefore runs **one process** — the server itself, as a direct child of your AI client. It does not run `npx`, which would start three (npm → a Node launcher → the server) and leave the extra two behind whenever a client terminates only the first. It also means the server can tell when your client goes away, and exit with it instead of lingering with write access to your vault.

Run the installer with either `npx obsidian-mcp-rs install` or `obsidian-mcp-rs install` after installing with Cargo. Neither command puts `npx` in the client config.

### Updating

**The server keeps itself up to date, from 0.8.0 onwards.** Once `install` has placed it, it checks for a new release at most once a day, in the background, and replaces itself when it finds one.

> Auto-update ships *in* 0.8.0, so it cannot bring you 0.8.0 — an older copy does not know how. Run `npx obsidian-mcp-rs@latest install` once and it takes over from there. `npx obsidian-mcp-rs list` says so if your copy predates it. Nothing is asked of you, and nothing is interrupted — the new version is what your client starts **next time it launches**.

Four things it deliberately will not do:

- **It waits 48 hours after a release ships.** If a release turns out to be broken, that window is when it gets withdrawn — so the bad one never reaches you.
- **It only ever replaces the copy `install` placed.** A binary from `cargo install`, from a distribution package, or the one `npx` caches belongs to whatever put it there. Those never update themselves.
- **It asks the network at most once a day per machine**, no matter how many times your client restarts the server — and a failed check is silent, so an offline or firewalled machine is never slowed down or broken by it.
- **It verifies what it downloads** against the checksum published with the release, and runs the new binary once before installing it. Anything that does not answer with the version it claims is thrown away.

Turning it off goes through an install, because that is where the answer is recorded:

```bash
npx obsidian-mcp-rs@latest install claude ~/vault --no-auto-update
```

Name the client and the vault. The bare `install --no-auto-update` starts the interactive wizard, and the wizard only records your answer once you have picked a client — so backing out of it changes nothing. The wizard asks instead of assuming and offers your current choice as the default. The answer is remembered; the same command with `--auto-update` takes it back. An install that says nothing about auto-update changes nothing about it, so configuring a second client will not switch it back on.

To take a release now rather than wait for the check, run **the installed copy** — `update` acts on the binary that is running it, and the copy in npm's cache is not the one in your configs. `npx obsidian-mcp-rs list` prints the path:

```bash
# macOS; see the table above for Linux and Windows
"$HOME/Library/Application Support/obsidian-mcp-rs/bin/obsidian-mcp-rs" update           # take the latest release
"$HOME/Library/Application Support/obsidian-mcp-rs/bin/obsidian-mcp-rs" update --check   # just say what is available
"$HOME/Library/Application Support/obsidian-mcp-rs/bin/obsidian-mcp-rs" update --force   # take it even inside the 48-hour hold
```

**What the check sends.** One `GET` to `api.github.com` per day, carrying what any HTTP request carries — your IP address — and a `User-Agent` of `obsidian-mcp-rs/<version>`. An actual update then downloads the binary from `github.com`, which redirects to `githubusercontent.com`. Nothing about your vaults, your notes, your client or your machine is sent, and nothing identifies you across days beyond what GitHub can infer from an address. It honours `HTTP_PROXY` / `HTTPS_PROXY` / `NO_PROXY`. If that is one request too many, `--no-auto-update` stops it entirely.

To go back to an older release, install it with auto-update off — `npx obsidian-mcp-rs@0.8.0 install claude ~/vault --no-auto-update`. Without that flag the daily check follows `releases/latest` in both directions and puts the newer one back within a day, which is deliberate: withdrawing a bad release means dropping it out of `releases/latest`, and everyone who already took it has to be able to come back down. The path in your configs never changes, so nothing needs re-pointing either way. (One exception, and it catches everyone who installed early: a config written before 0.7.0 does not hold that path yet, so none of this reaches it — see [below](#upgrading-from-a-config-written-before-070).)

`npx obsidian-mcp-rs list` prints where the server is, which version it is, and whether auto-update is on.

**On Windows, quit your AI clients before running `install`.** Windows refuses to overwrite a running executable; if a client still has the server open the installer will say so and ask you to close it. (`update` is not affected — it renames rather than overwrites.)

`uninstall` removes the binary too, once no client config still points at it.

### Upgrading from a config written before 0.7.0

Everything above describes a config `install` wrote for you. If yours predates 0.7.0 it holds `npx -y obsidian-mcp-rs`, or a resolved path into npm's `_npx` cache, and **re-running `install` will not replace it.**

The installer refuses to overwrite an entry that differs from the one it would write — that entry may be something you tuned by hand, and silently discarding it is worse than doing nothing. So it reports the entry and moves on:

```
! Claude Code – global (~/.claude.json)  already installed in ~/.claude.json, but with different settings than you asked for
    nothing was changed — re-run with --force to replace that entry
```

That line sits between the clients that *were* written and the "restart your client" note at the end, which is an easy place to miss it. If your client still starts `npx` after an update, this is why — and none of the process-lifecycle fixes above are in effect for it.

To migrate, add `--force`:

```bash
npx obsidian-mcp-rs@latest install --force
```

Every backend copies the previous file to `<config>.bak` before writing, so this is reversible.

`list` names the ones that need it:

```
! Claude Desktop                          outdated   …/claude_desktop_config.json — runs `obsidian-vault-mcp`, not the installed server; re-run `install --force`
! Claude Code – global (~/.claude.json)   outdated   ~/.claude.json — runs `npx`, not the installed server; re-run `install --force`
✓ Codex CLI – global                      installed  ~/.codex/config.toml
```

`outdated` means the entry is there and does not launch this server. Earlier versions reported every one of these as `installed`, because the check asked only whether an `obsidian` key existed — which is also why a config naming a binary that had since been deleted went unnoticed.

### Known issues that are not ours

- **Duplicate server processes on Claude Desktop.** Claude Desktop can spawn more than one copy of the same MCP server per launch ([claude-code#36616](https://github.com/anthropics/claude-code/issues/36616)). Nothing this server does causes it, and nothing it does can prevent it. It is safe: concurrent servers on one vault are serialised so they cannot lose each other's edits, and any that outlive their client exit on their own.
- **Orphaned MCP processes generally.** Several clients fail to terminate stdio MCP servers on unclean exit ([#22612](https://github.com/anthropics/claude-code/issues/22612), [#1935](https://github.com/anthropics/claude-code/issues/1935), [#40667](https://github.com/anthropics/claude-code/issues/40667)). This server watches the process that started it and exits when it goes, on all three platforms: a `getppid` poll on macOS and Linux, a wait on the parent's process handle on Windows.
- **Two devices, one synced vault.** Writes are serialised per machine. Two computers editing the same cloud-synced vault at once is a sync conflict, and it belongs to iCloud / Obsidian Sync rather than to this server.

## Features

- **25 MCP tools** for notes, search, links, properties, tasks, Canvas, and Obsidian Markdown guidance
- **Ranked search** — BM25 relevance with field boosts (a term in the title outranks the same term buried in a paragraph), returned best-first and capped so a common word can't flood the model's context
- **Link-aware moves** update inbound Markdown links and Canvas cards. Batch previews list affected files and blockers before you confirm the move.
- **Link graph** — `vault_links` answers backlinks, outgoing, broken links and orphans
- **Section-scoped edits** — point `edit_note` at one heading or one `^block-id` and only those bytes are rewritten; the rest of the note is passed through untouched
- **Frontmatter access** — `note_properties` reads and writes any YAML key, not just `tags`, and touches only the key you named
- **Multi-vault** support — pass multiple vault paths as arguments
- **Recoverable deletes** — `delete_note` moves the note to the vault's `.trash/` (as Obsidian does) rather than erasing it; a trashed note disappears from search and the link graph, but the user can still get it back
- **Daily notes** — `periodic_notes` reads/creates daily…yearly notes using the vault's *own* Obsidian settings (name format, folder, template), so it writes to the note you actually keep
- **Vault orientation** — `vault_info` answers what tags exist, what changed recently, and how big the vault is
- **Write controls** with read-only mode, folder allowlists, and revision checks for note edits
- **Markdown tasks** stay in their source notes. Find, add, finish, or reschedule tasks and preview changes across files.
- **JSON Canvas** lets agents read or edit nodes and edges while preserving untouched fields.
- **Zero runtime dependencies** — the server is a single static binary, and it is what your client runs: `install` places it and points the config straight at it, so no Node process sits in between. Node.js 22+ is needed only to *run the installer*; grab a binary from [Releases](https://github.com/MrRefactoring/obsidian-mcp-rs/releases) or `cargo install` to skip it entirely.
- **Cross-platform** — macOS (ARM64 + x64), Linux (x64 + ARM64 + musl), Windows (x64 + ARM64)
- **Tag search** via `tag:` prefix in queries
- **YAML frontmatter** tag management
- **Streamable HTTP** (optional) — `cargo install obsidian-mcp-rs --features http`, then `--http` serves several clients from one long-lived server. Validates the `Origin` header, as the MCP spec requires of local servers. stdio remains the default.
- **`npx` compatible** — try it or install it in one command, with nothing to download first

### Search

`search_notes` ranks hits with **BM25**, the same scoring family a full-text engine uses — but computed straight from the parallel vault walk, so there is no index to build, no watcher to keep in sync, and nothing to go stale when you edit a note in Obsidian.

Terms are weighted by where they occur: filename ×5, tags ×4, headings ×3, frontmatter ×2, body ×1. Rare terms count for more than common ones, so a query like `the kafka` ranks the note *about* Kafka above the note that merely says "the" a lot.

Results are paged (`limit`, default 20; `offset`) and each file quotes at most `maxMatchesPerFile` lines (default 3). Every response carries `total` and `truncated`, so the model can see that more matches exist without you paying for them in context.

Ranking answers "which notes are *about* this". Two questions it can't answer have their own arguments:

- **`regex: true`** — match a *shape* rather than words: a phone number, a `TODO(name)`, a URL. Hits are then ranked by how many lines matched, since relevance means nothing for a pattern.
- **`frontmatter: {"status": "active"}`** — keep only notes carrying those fields. A **list** field matches when it *contains* the value, so `{"tags": "work"}` finds a note with `tags: [work, urgent]`. Combine it with a query, or use it alone with an empty query as a pure metadata lookup ("every active note in this vault").

Both are computed inside the walk that already reads every note, so neither costs an extra pass.

## Performance

Vault-wide operations (`search_notes`, `rename_vault_tag`) walk the vault with the [`ignore`](https://crates.io/crates/ignore) crate and process files in parallel via [`rayon`](https://crates.io/crates/rayon). Measured with the criterion suite in [`benches/`](benches/vault_bench.rs) on a synthetic vault, Apple Silicon (10 logical cores); "serial" is the same code pinned to one thread (`RAYON_NUM_THREADS=1`):

| Operation                  | Serial (1 thread) | Parallel  | Speedup |
| -------------------------- | ----------------- | --------- | ------- |
| Ranked search (2000 notes) | 52.8 ms           | 26.2 ms   | ~2.0×   |
| Tag search (2000 notes)    | 45.6 ms           | 24.4 ms   | ~1.9×   |
| Tag rename (500 notes)     | 84.3 ms           | 60.0 ms   | ~1.4×   |

Single-note operations (`read_note`, `create_note`, `edit_note`, …) touch one file and are unaffected. Numbers vary with core count and disk; reproduce locally with `cargo bench`.

## Configuration

> **Tip:** `npx obsidian-mcp-rs install` writes these configs automatically, including the absolute path below — you do not have to work it out yourself. The sections below are for manual setup or reference.
>
> `command` is the path `install` reports, e.g. `~/Library/Application Support/obsidian-mcp-rs/bin/obsidian-mcp-rs` on macOS (spelled out in full — configs do not expand `~`). Do **not** put `npx` here: it starts three processes where one is needed, leaves two of them behind when a client shuts the server down, and hides your client's exit from the server so it cannot clean up after itself.

### Claude Desktop (`claude_desktop_config.json`)

```json
{
  "mcpServers": {
    "obsidian": {
      "command": "/Users/you/Library/Application Support/obsidian-mcp-rs/bin/obsidian-mcp-rs",
      "args": ["/path/to/your/vault"]
    }
  }
}
```

### Multiple vaults

```json
{
  "mcpServers": {
    "obsidian": {
      "command": "/Users/you/Library/Application Support/obsidian-mcp-rs/bin/obsidian-mcp-rs",
      "args": [
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
      "command": "/Users/you/Library/Application Support/obsidian-mcp-rs/bin/obsidian-mcp-rs",
      "args": ["~/Documents/Obsidian/MyVault"]
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
      "command": "/Users/you/Library/Application Support/obsidian-mcp-rs/bin/obsidian-mcp-rs",
      "args": ["/path/to/your/vault"]
    }
  }
}
```

Once added, Cursor's AI will have access to all 25 MCP tools in the default configuration. You can verify with the MCP panel in Settings.

### OpenClaw (`~/.openclaw/openclaw.json`)

```json
{
  "mcp": {
    "servers": {
      "obsidian": {
        "command": "/Users/you/Library/Application Support/obsidian-mcp-rs/bin/obsidian-mcp-rs",
        "args": ["/path/to/your/vault"],
        "transport": "stdio"
      }
    }
  }
}
```

## Read-only mode (`--no-edit`)

Pass `--no-edit` to start the server in read-only mode. The thirteen write-only tools are removed from `tools/list` and cannot be called.

**Removed under `--no-edit`** (the write-only tools):
`create_note`, `edit_note`, `delete_note`, `move_note`, `create_folder`, `add_note_tags`, `remove_note_tags`, `rename_vault_tag`, `create_task`, `update_task`, `apply_task_batch`, `create_canvas`, `edit_canvas`

**Still listed, because they read as well as write** — these are gated *per action*, so the reads work and the writes are refused:
- `note_properties` — `get` works; `set` and `remove` are refused
- `periodic_notes` — `get` and `list` work; `create` is refused
- `batch_move_notes` — preview works; applying a preview is refused

**Pure reads, always available:**
`read_note`, `search_notes`, `vault_links`, `vault_info`, `list_vaults`, `get_writing_guide`, `list_tasks`, `preview_task_batch`, `read_canvas`

### Limit writes to selected folders

Pass `--write-folder VAULT=FOLDER` when starting the server or running `install`. Use a vault name from `list_vaults` and a vault-relative folder path. Repeat the flag for multiple folders. Use `VAULT=.` to allow writes in all folders of one vault. If a vault name is ambiguous, use its canonical path.

```bash
obsidian-mcp-rs install claude --write-folder "MyVault=Inbox" ~/Obsidian/MyVault
```

With at least one rule, all configured vaults remain readable and only listed folders allow writes. A move or tag rename fails before it changes files if it needs to update a protected note. A normal delete from an allowed folder can move the note to the vault trash. Without rules, the server uses the default full write access. `--no-edit` disables writes regardless of these rules.

### Manual config with `--no-edit`

```json
{
  "mcpServers": {
    "obsidian": {
      "command": "/Users/you/Library/Application Support/obsidian-mcp-rs/bin/obsidian-mcp-rs",
      "args": ["--no-edit", "/path/to/your/vault"]
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

## Import notes

Use `import-markdown` for a local folder of Markdown notes and attachments. The command reads an existing MCP connection to select the vault and enforce its write rules. Omit `--apply` to preview the files, link changes, and blockers. Check the plan with the person who owns the vault before applying it with the same arguments and returned `plan_id`.

```bash
obsidian-mcp-rs import-markdown --client claude-code --vault MyVault --source ./export --folder Inbox
obsidian-mcp-rs import-markdown --client claude-code --vault MyVault --source ./export --folder Inbox --apply "$PLAN_ID"
```

Use `--global` for a global connection. Use one `--exclude PATH` per source-relative path to skip. Use `--allow-missing-attachments` only when the user accepts the unresolved attachments listed in the preview. The offline import preserves source files and rejects occupied destinations.

For other formats supported by the official Obsidian Importer, ask the user before opening it. `obsidian-mcp-rs open-importer --vault MyVault` opens the Importer for the named vault when Obsidian CLI and the official Importer plugin are available. The user chooses the source and confirms the import in Obsidian. [Import guide](docs/import.md)

## Tool Reference

**Migration:** Tool names changed to the snake_case names below. Update clients that call the old names; aliases are not provided.

### `read_note`
Read the content of an existing note.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `vault` | string | ✓ | Vault name |
| `filename` | string | ✓ | Note filename (`.md` optional) |
| `folder` | string | | Subfolder path within vault |
| `view` | string | | `content` (default) or `outline` — headings, block refs and frontmatter keys |
| `offset` | number | | First line to return, 1-based (default 1) |
| `limit` | number | | Most lines to return (default 400) |

Reads are capped so that one long note cannot consume the model's whole context. Past the limit the note is cut off with a marker saying which lines you got and what `offset` to pass for the rest; a note that fits comes back untouched. `offset` speaks the same line numbers `view: "outline"` prints, so one can be pasted straight into the other.

The second text block contains the revision of the complete file, even for an outline or partial response.

### `create_note`
Create a new note with Markdown content.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `vault` | string | ✓ | Vault name |
| `filename` | string | ✓ | Note filename |
| `content` | string | ✓ | Markdown content |
| `folder` | string | | Subfolder path (created automatically) |

### `edit_note`
Edit an existing note.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `vault` | string | ✓ | Vault name |
| `filename` | string | ✓ | Note filename |
| `operation` | string | ✓ | `append`, `prepend`, `replace`, `find_and_replace` |
| `content` | string | ✓ | Content to apply |
| `folder` | string | | Subfolder path |
| `search` | string | | Search text (required for `find_and_replace`) |
| `targetType`, `target` | string | | Select a heading or block. Pass both fields together. |
| `expectedRevision` | string | | Revision returned by the read. If the file changed, read it again before applying the edit. |

### `delete_note`
Delete a note from the vault.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `vault` | string | ✓ | Vault name |
| `filename` | string | ✓ | Note filename |
| `folder` | string | | Subfolder path |

### `move_note`
Move or rename a note within the vault.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `vault` | string | ✓ | Vault name |
| `filename` | string | ✓ | Source filename |
| `folder` | string | | Source folder |
| `newFolder` | string | | Destination folder. **Omit to keep the note where it is** — that is how you rename in place. Pass `""` to move it to the vault root. |
| `newFilename` | string | | New filename (same if omitted) |

Pass at least one of `newFolder` or `newFilename`. The move rejects an occupied destination. It updates inbound Markdown links and Canvas file cards, and reports any link updates that could not finish after the move.

### `create_folder`
Create a new directory in the vault.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `vault` | string | ✓ | Vault name |
| `path` | string | ✓ | Directory path relative to vault root |
| `recursive` | boolean | | Create parent dirs (default: `true`) |

### `search_notes`
Search notes by content, filename, or tag. Results are BM25-ranked, best-first, and paged.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `vault` | string | ✓ | Vault name |
| `query` | string | ✓ | Search term. `tag:name` searches a tag. May be empty when filtering by `frontmatter` alone |
| `path` | string | | Limit search to subfolder |
| `caseSensitive` | boolean | | Default: `false` |
| `searchType` | string | | `content` (default), `filename`, `both` |
| `regex` | boolean | | Read `query` as a regular expression (default `false`) |
| `frontmatter` | object | | Only notes carrying these fields, e.g. `{"status": "active"}`. A list field matches when it *contains* the value |
| `limit` | number | | Files to return (default 20) |
| `offset` | number | | Skip this many files (default 0) |
| `maxMatchesPerFile` | number | | Lines quoted per file (default 3) |

Each hit carries a `path` — pass it straight back as any note tool's `filename`.

### `vault_links`
The vault's link graph, in one parallel pass.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `vault` | string | ✓ | Vault name |
| `query` | string | ✓ | `backlinks`, `outgoing`, `broken`, or `orphans` |
| `filename` | string | | The note to ask about — required by `backlinks` and `outgoing` |
| `folder` | string | | Subfolder containing the note |
| `limit` | number | | Default 50 — `broken` and `orphans` run to thousands on a neglected vault |
| `offset` | number | | Skip this many (default 0) |

Links inside code fences are ignored: a `[[link]]` in a code sample is documentation, not a reference.

### `note_properties`
Read or write any YAML frontmatter key — not just `tags`. Writes are line surgery on the one key named, so the rest of the block (comments, key order, quoting) survives byte for byte.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `vault` | string | ✓ | Vault name |
| `filename` | string | ✓ | Note path |
| `action` | string | ✓ | `get`, `set`, or `remove` |
| `key` | string | | Which field. Omit with `get` to return the whole block |
| `value` | any | | What to write — string, number, boolean, list or object (`set` only) |
| `folder` | string | | Subfolder containing the note |

Under `--no-edit` this is gated per action: `get` works, `set`/`remove` are refused.

### `vault_info`
What's actually in this vault — the questions you ask *before* you know what to search for.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `vault` | string | ✓ | Vault name |
| `query` | string | ✓ | `tags` (every tag + how many notes carry it, commonest first), `recent` (newest first), or `stats` |
| `limit` | number | | Cap the list (default 20) |

### `periodic_notes`
Today's daily note, and its weekly/monthly/quarterly/yearly siblings — read from Obsidian's *own* settings (the Periodic Notes plugin's `data.json`, then core's `daily-notes.json`, then Obsidian's defaults), so it lands where Obsidian would rather than creating a stray note.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `vault` | string | ✓ | Vault name |
| `period` | string | ✓ | `daily`, `weekly`, `monthly`, `quarterly`, `yearly` |
| `action` | string | ✓ | `get`, `create` (idempotent), or `list` |
| `date` | string | | `YYYY-MM-DD` — defaults to today |
| `content` | string | | Text for a note `create` brings into existence; without it, the configured template is used |
| `limit` | number | | How many notes `list` walks back (default 10) |

### `add_note_tags`
Add tags to notes in frontmatter and/or content.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `vault` | string | ✓ | Vault name |
| `files` | string[] | ✓ | Vault-relative note paths. **All must exist** — if any doesn't, nothing is changed |
| `tags` | string[] | ✓ | Tags to add |
| `location` | string | | `frontmatter`, `content`, or `both` (default). Note that `both` puts the tag in the note **twice** |
| `normalize` | boolean | | Normalize tag format (default: `true`) |
| `position` | string | | `start` or `end` (default) — where an inline tag goes |

### `remove_note_tags`
Remove tags from notes.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `vault` | string | ✓ | Vault name |
| `files` | string[] | ✓ | Note filenames |
| `tags` | string[] | ✓ | Tags to remove |

### `rename_vault_tag`
Rename a tag across all notes in the vault.
The tool is available when the server has write access.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `vault` | string | ✓ | Vault name |
| `oldTag` | string | ✓ | Current tag name |
| `newTag` | string | ✓ | New tag name |
| `dryRun` | boolean | | Set to `true` to list the notes that would change without writing to them. The default is `false`. |

### `list_vaults`
List all vaults configured for this server. Takes no parameters.

### `get_writing_guide`

Read [Obsidian Markdown rules, examples, and note recipes](docs/writing-guide.md). This tool works offline and is available in read-only mode.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `topics` | string[] | | Up to four different topic names. Omit to list the topics. |

For example, request `{"topics":["links","callouts","meeting"]}`. Use the official `obsidian-markdown` skill when it is installed. Use the `sources` topic to see the pinned upstream revision and license.

### Task tools

| Tool | Use |
|------|-----|
| `list_tasks` | Find Markdown checkboxes in a vault, recursive folder, note, or today's daily note. Filter by status, dates, priority, tags, or blockers. Use `next_offset` to read more. |
| `create_task` | Add a task to a note or daily note. Select a section with `destination.heading`. |
| `update_task` | Pass the task's path, line, and revision from `list_tasks`. Change its text, status, due or scheduled date, or priority. |
| `preview_task_batch` | Preview up to 50 moves or date changes. Review warnings and affected files with the user. |
| `apply_task_batch` | After confirmation, submit the same operations and `plan_id`. Inspect per-file results if the batch stops before completion. |

For example, `{"vault":"MyVault","status":"todo","date":{"field":"due","match":"overdue"}}` lists overdue tasks. Use `scope: {"kind":"today"}` to find tasks written in today's daily note. The tools keep tasks in their source notes. Supported Obsidian Tasks fields, recurrence, and ready-to-use prompts are listed in the workflow guide. [Task workflows](docs/task-workflows.md)

### Canvas tools

| Tool | Use |
|------|-----|
| `read_canvas` | Read a board's nodes, edges, and revision by vault-relative path. |
| `create_canvas` | Create a board at an unused path. Pass JSON Canvas `nodes` and `edges`, or create an empty board. |
| `edit_canvas` | Pass `expectedRevision` and add, update, or remove nodes and edges. Existing nodes keep their positions unless you change them. |

Omit both `x` and `y` to place a new node automatically. Use node IDs in edge endpoints and edit patches. [JSON Canvas format](https://jsoncanvas.org/spec/1.0/)

### `batch_move_notes`

Submit `moves`, each with `sourceVault`, `sourcePath`, `destinationVault`, and `destinationPath`. Use exact vault-relative note paths. The response lists changes and blockers and returns `preview_id`. Use `offset` and `limit` to page through the details.

Show the preview to the user before applying it. Use `allowAttachmentCopy` if the user agrees to copy referenced files while keeping the originals. Use `allowEmbedConversion` and `allowCanvasConversion` only if the user agrees to convert cross-vault embeds and Canvas file cards to clickable links. If you change an option, get a new preview. To apply, send the same moves and options with `previewId`. If a file changed since the preview, review and confirm a new plan.

## Development

### Prerequisites

- [Rust](https://rustup.rs/) (stable; MSRV 1.89)
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

# If you started the server with --log-file, point `logs` at the same path,
# or it will show you the default log while yours fills up elsewhere.
npx obsidian-mcp-rs logs --log-file /tmp/mcp-debug.log
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
          ├── ObsidianHandler → 25 MCP tool implementations
          └── rmcp → JSON-RPC / MCP protocol
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, checks, and pull requests.
Community rules are in [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
Report vulnerabilities through the private contact in [SECURITY.md](SECURITY.md).

## License

MIT — see [LICENSE](LICENSE).
