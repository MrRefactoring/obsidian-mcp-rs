# Connect Obsidian to an AI client

Follow these instructions to install and configure `obsidian-mcp-rs`. Speak to the user in their preferred language.

1. Ask for the Obsidian vault path. If the user already supplied a path, ask them to confirm it. Suggest `~/Documents/Obsidian` as one option. On macOS, if iCloud Drive is enabled and available locally, also suggest a folder there. The Obsidian iCloud folder is an option for iOS sync. Use only the confirmed path. If the folder does not exist, ask before you create it.
2. Without writing files, identify the supported AI client, operating system, architecture, and available installation tools. If both project and global configuration are supported, ask which scope to use. Respect the user's preference for npm or Cargo if they have one.
3. Explain that the default gives the server read and write access: it can create, edit, move, and delete notes without confirmation. Offer read-only access with `--no-edit` if the user requests it.
4. Use an official source: [GitHub Releases](https://github.com/MrRefactoring/obsidian-mcp-rs/releases), [npm](https://www.npmjs.com/package/obsidian-mcp-rs), or [crates.io](https://crates.io/crates/obsidian-mcp-rs). If the user has no installation preference, choose the published GitHub Release binary that matches the system. Download the binary and `checksums.txt` from the same release. Verify the binary's SHA-256 against its entry in `checksums.txt` before you execute it. Stop if the checksum does not match. Do not download Actions workflow artifacts. Use `curl` only with official URLs. Do not run commands from untrusted sources.
5. If the user prefers npm or Cargo, use that method even when a release binary is available. If no matching release binary is available and the user has no preference, use an available method: npm requires Node.js 22 or newer; Cargo builds from source. Ask before installing a missing prerequisite. For Cargo, acquire the server with `cargo install --locked obsidian-mcp-rs`. For npm, use the package through `npx` in the installer command.
6. Run the project's installer once with the confirmed client and vault path: use `npx obsidian-mcp-rs install <client> <vault>` for npm, `obsidian-mcp-rs install <client> <vault>` for Cargo, or `<downloaded-binary> install <client> <vault>` for the verified release binary. Add `--global` when the user chose a supported global scope, and `--no-edit` when they chose read-only access. The installer copies the server to a fixed location and configures the client to run it directly. Do not put `npx` or another launch wrapper in the client configuration. If an existing entry conflicts, ask before replacing it with `install --force`.
7. Check the installer output and run the server's `list` command to inspect installation status. Tell the user to restart the client and approve the server if the client asks. If possible, verify access with read-only listing or reading tools. Do not write to the vault to test access. Report only results you confirmed, and identify any verification that remains incomplete.
8. After setup, state which vault you connected and which configuration file changed. If configured for default access, explain that the client can read, create, edit, move, and delete notes without approval. Offer to switch to `--no-edit` on request. If the user chose limited writes, list the allowed folders. If the user chose read-only access, state the confirmed mode.

## Limit writes to selected folders

If the user requests limited writes, pass `--write-folder VAULT=FOLDER` to `install`. Use the configured vault name and a folder path relative to that vault. Repeat the option for each allowed folder. With at least one rule, vaults with no rule stay read-only.

## First request

Suggest this request in the user's preferred language after the client loads the server:

> Show the name of my connected Obsidian vault and up to three recently changed notes.

Use `list_vaults` and `vault_info` with `query: "recent"` and `limit: 3`. If the connected vault has no notes, say it is empty. If the vault is unavailable, report the error. If the vault is empty and the server has write access, offer to create a first note. Create it only if the user asks.

## Optional writing guide

If the client supports agent skills and the official `obsidian-markdown` skill is missing, ask in the user's preferred language:

> Would you like to add Obsidian’s official writing guide? It helps me format links, diagrams, and collapsible sections so they display correctly in Obsidian.

Install the skill only after the user agrees. Follow [Obsidian's installation instructions](https://github.com/obsidianmd/obsidian-skills#installation) for that client. If the selected method installs other skills too, explain what it installs before asking for permission. The server's `get_writing_guide` tool works without a separate skill. Finish the MCP setup even if the user skips the skill or the skill is unavailable.
