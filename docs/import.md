# Import notes into Obsidian

Ask the user which source and vault to use. Prefer the official Importer for an export from another application, and for HTML or CSV. Use the offline CLI for a folder of Markdown notes and their local attachments.

## Import from an application

Follow the [source-specific export guide](https://obsidian.md/help/import). With the official Importer enabled in the selected Obsidian vault and the desktop CLI available, open the Importer only at the user's request:

```sh
obsidian-mcp-rs open-importer --vault 'Work'
```

Use the exact vault name from the desktop application. If the CLI or plugin is unavailable, ask the user to enable [Importer](https://obsidian.md/help/plugins/importer) under Community plugins and choose **Import notes** in the command palette. The user selects the format, source, destination and options, then starts the import in Obsidian. The application's operation uses Obsidian's permissions.

After the user reports completion, check the destination through the MCP connection. Use `search_notes` to find the imported notes, `note_properties` to inspect properties and `vault_links` to find unresolved internal links. Report only verified results.

An isolated desktop experiment with Obsidian 1.15.1 and Importer 3.2.0 ran the plugin's `runImport` method against Markdown and HTML fixtures and found their output files. CSV required its separate configuration screen and wrote no files in that experiment. The production CLI dispatches the plugin's command to open the UI.

## Import a local Markdown folder without opening Obsidian

A local agent needs a terminal and access to the folder the user selected. Run from the client project's directory for a project connection. Add `--global` to select the global connection where the client offers one. Use the vault name from `list_vaults`.

```sh
obsidian-mcp-rs import-markdown --client claude-code --vault 'Work' --source ~/Downloads/Export --folder 'Inbox/Import'
```

Show the user the planned paths, any link updates, any blockers and any warnings. The report is JSON and includes a `plan_id`. The import preserves the source layout, copies referenced local attachments and keeps the source files in place. It reports an existing destination as a blocker. Choose another destination or use repeatable `--exclude SOURCE_RELATIVE_PATH` options after the user chooses files or subfolders to skip. Missing local embeds block the import unless the user agrees to continue with `--allow-missing-attachments`. Preview again after changing options.

After the user confirms a plan with no blockers, repeat the original command with `--apply PLAN_ID`:

```sh
obsidian-mcp-rs import-markdown --client claude-code --vault 'Work' --source ~/Downloads/Export --folder 'Inbox/Import' --apply 'PLAN_ID'
```

The command reads its write permissions from the selected client configuration and checks the source and destination again. A `--no-edit` connection can preview. A changed plan requires a new preview and confirmation. If a write fails, the report identifies the files already written, the failure and the files still pending. Verify the resulting notes using the MCP connection.
