# Changelog

## [Unreleased]

### Changed

- **Internal refactor, no behavioural change** (same public MCP API, same config-file output). Split the 1700-line `src/vault.rs` into a `src/vault/` module — `mod.rs` (the `VaultManager` orchestrator), `path.rs` (`safe_join` sandbox), `frontmatter.rs` (parsing + `find_closing_fm`), `tags.rs` (tag operations + `replace_inline_tag`), `search.rs` (`SearchResult`/`SearchType` + the walk). Tests moved alongside the code they cover. All 190 tests stay green; `cargo clippy -- -D warnings` and `cargo fmt --check` are clean.
- `install/writer.rs` reworked around a `ConfigBackend` trait (`JsonBackend` parameterised by entry-path + builder, `TomlBackend`, `YamlBackend`), dispatched from a single `backend(format)` match. Adding a new JSON-shaped client is now one match arm instead of editing five `match`-on-`ConfigFormat` blocks. The dir/backup/write sequence is consolidated into one `write_with_backup` helper.
- `add_tags_to_frontmatter` flattened from four nested branches into early-return guard clauses; output is byte-for-byte identical.
- Frontmatter `tags` parsing moved from the hand-rolled line scanner to `serde_yml` (`frontmatter::extract_tags`), eliminating a custom YAML subset parser. Boundary detection still uses `find_closing_fm` (serde does not handle `---` markers). **Behaviour change:** parsing is now strict — a note whose frontmatter body is *invalid* YAML yields no tags instead of being scraped line-by-line, and non-string tag values (e.g. `tags: [2024]`) are ignored. Well-formed vaults are unaffected.
- Vault-wide walks (`search-vault`, `rename-tag`) replaced `walkdir` with the `ignore` crate via a shared `walk::md_files` helper (de-duplicating the two identical walk loops). **Behaviour change:** `.gitignore` rules and hidden files/folders are now respected, so gitignored or hidden notes are skipped — including by `rename-tag`.

### Performance

- Vault walks now process files in parallel with `rayon` (`search-vault`, `rename-tag`), giving near-linear speedup on large vaults.
- Case-insensitive content search lowercases each file once instead of once per line.

### Security

- **Path traversal in `add-tags` / `remove-tags`** — the v0.1.6 sandboxing fix routed every other path-bearing tool through `safe_join`, but the two tag tools still used a bare `root.join(file)` for each entry in their `files` array. A crafted `files: ["../../../etc/hosts"]` (or any absolute path) would let an MCP client read and overwrite files anywhere the server process could reach. Both tools now resolve every entry through `safe_join`, so traversal attempts return an `InvalidPath` error before any I/O. New regression tests: `add_tags_blocks_traversal`, `add_tags_blocks_absolute_path`, `remove_tags_blocks_traversal`.

### Fixed

- **Frontmatter terminator false-positives** — the closing-`---` marker was located with `find("\n---")`, which also matched `\n----`, `\n---foo`, and similar non-delimiters, splitting the frontmatter at the wrong byte and corrupting the body on subsequent writes. A new `find_closing_fm` helper requires `---` to stand alone on a line (followed by `\n`, `\r`, or end-of-input) and is now used by `extract_frontmatter`, `add_tags_to_frontmatter`, `add_tags_to_content`, `remove_tags_from_note`, and `rename_tag_in_note`.
- **Inline-tag rewrites corrupted overlapping tags** — `rename-tag` and `remove-tags` used `String::replace` on `#tag`, so renaming `foo` to `bar` also clobbered `#foobar` → `#barbar` and `#foo-extra` → `#bar-extra`. A new `replace_inline_tag` helper enforces a right-boundary check (tag-continuation characters: alphanumerics, `-`, `_`, `/`). Tests: `rename_tag_does_not_corrupt_overlapping_inline_tags`, `remove_tags_does_not_corrupt_overlapping_inline_tags`.
- **Vault basename collisions silently shadowed earlier paths** — `VaultManager::new` keyed every vault by `path.file_name()`, so passing `~/work/notes` and `~/personal/notes` would register only the second one. Colliding names are now disambiguated as `<name>-2`, `<name>-3`, … with a `tracing::warn!`. Test: `vault_basename_collisions_are_disambiguated`.

### Removed

- Crate-wide `#![allow(dead_code)]` in `main.rs`. The build is now warning-clean.
- Unused `pub type Xxx = Parameters<XxxParams>;` aliases from all 11 files under `src/tools/` (no consumer referenced them).
- Unused `SearchResult.vault` and `Frontmatter.raw` fields (populated but never read).
- Unused `regex` crate dependency (`normalize_tag` was constructing a `Regex` it never applied).

### Added

- `CLAUDE.md` — onboarding notes for Claude Code: commands (incl. the `--bin obsidian-mcp-rs` workaround for `cargo test --lib`), the stdout-is-MCP transport invariant, the `safe_join` / `check_write` security model, the multi-vault basename rule, and engineering principles.


## [0.1.6] - 2026-05-21

### Security

- **Path traversal in vault tools** — `filename` and `folder` arguments accepted by `read-note`, `create-note`, `edit-note`, `delete-note`, `move-note`, `create-directory`, `add-tags`, `remove-tags`, `rename-tag`, and `search-vault` were not validated, so a crafted `../` (or an absolute path) could read, write, or delete files outside the configured vault root. Symlinks inside the vault that pointed outside it were also followed. All path inputs now go through a `safe_join` helper that canonicalizes the deepest existing ancestor and rejects anything that does not live under the canonicalized vault root; absolute paths in `filename`/`folder` are rejected outright. Reported by Luca; tests cover `..` traversal, absolute paths, and symlink-based escapes.

### Added

- `install`/`uninstall`/`list` support for 12 additional MCP clients: Windsurf, VS Code (Copilot), Gemini CLI, Antigravity, Cline, Kiro, LM Studio, Factory, Amp, opencode, Codex CLI, Goose
- TOML and YAML config-format writers (Codex `config.toml`, Goose `config.yaml`)

### Changed

- `logs` subcommand output is now colorized (ERROR red, WARN yellow, DEBUG/TRACE dimmed) with styled headers and separators


## [0.1.5] - 2026-04-14

### Fixed

- `install`, `uninstall`, `list`, and `logs` subcommands not recognized by the published binary — the platform packages (`@obsidian-mcp-rs/*`) were pinned to `0.1.2` in `optionalDependencies` instead of the current version, so npx resolved an old binary without these subcommands
- `prepare-release.sh` now updates `optionalDependencies` unconditionally (previously only matched entries at `CURRENT_VERSION`, silently skipping them when platform packages lagged behind)


## [0.1.4] - 2026-04-13

### Changed

- Dependencies updated to latest versions: `dirs` 5 → 6, `dialoguer` 0.11 → 0.12, `console` 0.15 → 0.16
- `rust-version` set to `1.94` in `Cargo.toml`; MSRV in `clippy.toml` updated to match
- Code modernised for Rust 1.94: nested `if let` chains collapsed using stabilised `let_chains`; `manual_strip` and `if_same_then_else` lints resolved in `vault.rs`


## [0.1.3] - 2026-04-13

### Added

- `install` subcommand — interactive wizard and direct CLI to write MCP config into AI client config files
  - Clients: Claude Desktop, Claude Code (local `.mcp.json` + global `~/.claude.json`), Cursor (local `.cursor/mcp.json` + global `~/.cursor/mcp.json`), OpenClaw
  - `--global` flag selects global config for `claude-code` and `cursor` (local is default)
  - `--dry-run`, `--force` flags; auto-backup before any write (`.json.bak`)
  - Cross-platform config path resolution (macOS / Windows / Linux)
- `uninstall` subcommand — interactive or direct removal of MCP config entry
- `list` subcommand — show installation status across all detected AI clients
- `logs` subcommand — print log file path, last 100 log entries, and a GitHub issue link for bug reports
- `--no-edit` flag — starts the server in read-only mode; all write tools (`create-note`, `edit-note`, `delete-note`, `move-note`, `create-directory`, `add-tags`, `remove-tags`, `rename-tag`) return an error immediately
- `--verbose` / `-v` flag — enables DEBUG-level logging to stderr without needing `RUST_LOG`
- `--log-file <FILE>` flag — override the automatic log file path; pass `-` to disable file logging entirely
- Automatic DEBUG log file written on every server start:
  - macOS: `~/Library/Logs/obsidian-mcp-rs/obsidian-mcp-rs.log`
  - Linux: `~/.local/share/obsidian-mcp-rs/obsidian-mcp-rs.log`
  - Windows: `%LOCALAPPDATA%\obsidian-mcp-rs\obsidian-mcp-rs.log`
- Structured startup log: version, PID, no_edit state, and each vault path logged at INFO on start
- `tracing::debug!` on every MCP tool invocation with key parameters; `tracing::error!` on every tool failure
- `scripts/prepare-release.sh` — automates version bump across all 9 package files and updates `CHANGELOG.md`
- `codecov.yml` — Codecov flag configuration for separate Rust and TypeScript coverage reporting
- Code coverage badge in README (Codecov)
- `platform.ts` — platform detection logic extracted from `bin.ts` into a separate, testable module with named exports
- `platform.test.ts` — 16 vitest unit tests covering `detectPlatform`, `detectMusl`, and `resolveBinaryPath`
- `vitest.config.ts` — vitest configuration with `@vitest/coverage-v8` lcov reporter
- Russian README (`README.ru.md`) with language switcher on both README files

### Changed

- README: added **Quick setup** section near the top with wizard and direct install examples
- README: added **Troubleshooting** section with log file locations, `--verbose`, `--log-file` usage, and bug-report instructions
- README: added language switcher (`English | Русский`) below the header
- CI: workflow branch target changed from `main` to `master`; all action versions updated to latest
- CI: added `coverage` job — `cargo llvm-cov --lcov` for Rust and `vitest --coverage` for TypeScript, both uploaded to Codecov with separate flags
- `bin.ts` refactored into a thin launcher (`spawnSync`); all detection logic moved to `platform.ts`
- `tsconfig.json`: test and config files excluded from the build output

### Fixed

- CI was not running on `master` branch (was targeting non-existent `main`)
- `bin.ts` platform logic was untestable due to inline `require()` calls; fixed by moving to static imports in `platform.ts`

## [0.1.2] - 2026-04-13

### Added

- Project logo (`assets/logo.svg`) — Obsidian crystal with MCP connection nodes
- Write-access warning in README — users are informed the server has full read/write access to vaults

### Changed

- Rust edition updated from `2021` to `2024`
- `similar` dependency updated to v3.1.0
- README header redesigned: centered layout, logo, promo badges (Claude Ready, Cursor Ready, MCP Native, Rust Powered, npx Compatible), flat-square style throughout
- Development prerequisites updated to Node.js 22+

### Fixed

- README was not included in the published npm package — added `cp README.md npm/obsidian-mcp-rs/README.md` step to release workflow
- Logo and badge URLs use absolute `raw.githubusercontent.com` paths so they render correctly on npmjs.com

## [0.1.1] - 2026-04-13

### Changed

- TypeScript dev dependency updated to v6; added explicit `types: ["node"]` to `tsconfig.json` (required by TypeScript v6)
- GitHub Actions updated: `actions/checkout` → v6, `actions/setup-node` → v6, `actions/upload-artifact` → v7, `actions/download-artifact` → v8, `softprops/action-gh-release` → v3

### Fixed

- `repository.url` casing corrected to `MrRefactoring` in all platform `package.json` files (sigstore provenance validates case-sensitively)

### Removed

- Unused direct dependencies `serde_json` and `serde_yaml_neo` from `Cargo.toml`

## [0.1.0] - 2026-04-13

### Added

- 12 MCP tools: `read-note`, `create-note`, `edit-note`, `delete-note`, `move-note`, `create-directory`, `search-vault`, `list-available-vaults`, `add-tags`, `remove-tags`, `rename-tag`
- Multi-vault support — pass multiple vault paths as CLI arguments
- `edit-note` operations: `append`, `prepend`, `replace`, `find_and_replace`
- Content, filename, and tag search (`tag:` prefix) in `search-vault`
- YAML frontmatter tag management with inline and block list support
- Tag normalization (lowercase, hyphenated)
- Cross-platform binary distribution via npm optional dependencies
- Platform packages: `darwin-arm64`, `darwin-x64`, `linux-arm64`, `linux-x64`, `linux-x64-musl`, `win32-arm64`, `win32-x64`
- TypeScript npm wrapper with automatic platform detection and musl detection for Linux
- GitHub Actions CI: lint, test, cross-target `cargo check`
- GitHub Actions release pipeline: builds all 7 targets, creates GitHub Release with SHA256 checksums, publishes npm packages with provenance

[Unreleased]: https://github.com/MrRefactoring/obsidian-mcp-rs/compare/v0.1.6...HEAD
[0.1.6]: https://github.com/MrRefactoring/obsidian-mcp-rs/releases/tag/v0.1.6
[0.1.5]: https://github.com/MrRefactoring/obsidian-mcp-rs/releases/tag/v0.1.5
[0.1.4]: https://github.com/MrRefactoring/obsidian-mcp-rs/releases/tag/v0.1.4
[0.1.3]: https://github.com/MrRefactoring/obsidian-mcp-rs/releases/tag/v0.1.3
[0.1.2]: https://github.com/MrRefactoring/obsidian-mcp-rs/releases/tag/v0.1.2
[0.1.1]: https://github.com/MrRefactoring/obsidian-mcp-rs/releases/tag/v0.1.1
[0.1.0]: https://github.com/MrRefactoring/obsidian-mcp-rs/releases/tag/v0.1.0
