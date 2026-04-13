# Changelog

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

[0.1.3]: https://github.com/MrRefactoring/obsidian-mcp-rs/releases/tag/v0.1.3
[0.1.2]: https://github.com/MrRefactoring/obsidian-mcp-rs/releases/tag/v0.1.2
[0.1.1]: https://github.com/MrRefactoring/obsidian-mcp-rs/releases/tag/v0.1.1
[0.1.0]: https://github.com/MrRefactoring/obsidian-mcp-rs/releases/tag/v0.1.0
