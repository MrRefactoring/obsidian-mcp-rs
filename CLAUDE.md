# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Build / run
cargo build                              # debug
cargo build --release                    # release (LTO, opt-level=3, stripped)
cargo run -- /path/to/vault              # run server against a vault
cargo run -- --no-edit /path/to/vault    # read-only mode

# Tests
cargo test                               # all tests (the unit tests live in the lib target)
cargo test --lib <name>                  # single test — tests compile under the lib crate

# Benchmarks (criterion, harness = false; see benches/vault_bench.rs)
cargo bench                              # run search / rename_tag benchmarks
RAYON_NUM_THREADS=1 cargo bench          # single-threaded baseline for comparison
cargo bench --no-run                     # compile-only — this is what CI gates on

# Lint gates enforced by CI (.github/workflows/ci.yml)
cargo fmt --check
cargo clippy -- -D warnings

# Coverage (matches CI)
cargo llvm-cov --lcov --output-path lcov-rust.info

# Cross-platform check (CI matrix: aarch64/x86_64 darwin, x86_64 linux gnu+musl, x86_64 windows-msvc)
cargo check --target <triple>

# npm wrapper (in npm/obsidian-mcp-rs/)
npm install && npm run build && npm test   # TS build + vitest
```

Toolchain is pinned to **stable** (`rust-toolchain.toml`); MSRV is **1.94**. `rustfmt`: 100-col, 4-space, edition 2021. `clippy.toml` sets `msrv = "1.94.0"`.

## Architecture

This is a Rust **library** (`src/lib.rs`, crate `obsidian_mcp_rs`) holding all domain logic, with a **thin binary** (`src/main.rs`) that wires up the CLI/logging and speaks MCP over stdio. Splitting lib/bin lets `benches/` and integration tests link against the code. There is also an **npm wrapper** that distributes prebuilt binaries via the optional-dependencies pattern.

### Transport invariant (do not break)

The server uses `(stdin, stdout)` for the MCP JSON-RPC stream (`main.rs::run_server`). **Anything that writes to stdout will corrupt the protocol.** All diagnostics go to stderr or to a rotating file log (`~/Library/Logs/obsidian-mcp-rs/obsidian-mcp-rs.log` on macOS, `~/.local/share/...` on Linux, `%LOCALAPPDATA%\...` on Windows). `tracing_subscriber` is configured in `main::setup_logging`: stderr layer = WARN by default (DEBUG with `--verbose`), file layer = always DEBUG.

### Module layout

| Module       | Role                                                                                                                 |
|--------------|----------------------------------------------------------------------------------------------------------------------|
| `lib.rs`     | crate root — re-exports `error`/`handler`/`install`/`tools`/`vault` as the public library surface                    |
| `main.rs`    | thin bin over the lib: clap CLI, log setup, dispatches to `install`/`uninstall`/`list`/`logs` subcommands or starts the MCP server |
| `handler.rs` | `ObsidianHandler` with `#[tool_router]` macro — 11 MCP tools, thin wrappers over `vault`                             |
| `vault/`     | `VaultManager` (`mod.rs`) + submodules: `path` (**`safe_join` sandbox**), `frontmatter` (serde_yml tag parse), `tags`, `search`, `walk` (`md_files` via `ignore`). Vault walks run in parallel with `rayon` |
| `tools/*.rs` | `serde` + `schemars::JsonSchema` param structs only — one per tool                                                   |
| `install/`   | Writes/removes MCP-server entries in 14 AI-client configs (JSON / TOML for Codex / YAML for Goose)                   |
| `error.rs`   | `VaultError` + `From<VaultError> for rmcp::ErrorData`                                                                |

Tools are wired via the `#[tool_router]` / `#[tool_handler]` rmcp macros — adding a new tool means: new `tools/foo.rs` with a `Params` struct, plus a method on `ObsidianHandler` annotated `#[tool(name = "foo")]`.

### Security model (load-bearing — don't regress)

All paths that touch the filesystem **must** route through `vault::safe_join(root, folder, filename)`. It canonicalises the deepest existing ancestor and rejects anything that escapes the canonicalised vault root. This covers `..` traversal, absolute paths, and symlink escapes. There is a dedicated test block in `vault.rs` (`rejects_parent_traversal_*`, `rejects_symlink_escape`, `*_blocks_traversal`) — when adding any tool that accepts a user-supplied path component, add a matching block-traversal test.

The `--no-edit` flag is a gate enforced in `ObsidianHandler::check_write()`, called by every mutating tool. Read tools (`read-note`, `search-vault`, `list-available-vaults`) skip this gate. Adding a new write tool means calling `check_write()?` first.

### Multi-vault model

`VaultManager` keys each vault by its directory basename. Basename collisions are disambiguated as `<name>-2`, `<name>-3`, ... with a `tracing::warn!`. The MCP client refers to vaults by these names via the `vault` parameter on every tool.

### npm wrapper

`npm/obsidian-mcp-rs/` is a TypeScript launcher that resolves the correct platform-specific binary subpackage (`@obsidian-mcp-rs/darwin-arm64`, etc.) at install time via npm's `optionalDependencies`. The seven `npm/<triple>/` directories are the per-platform packages that get published alongside the main wrapper. **Versions across all eight packages must stay in lockstep**; `scripts/prepare-release.sh` handles the bump.

### CI gates that block merge

`cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, `cargo check` across the 5-target matrix, and the npm wrapper's `build` + `vitest`. Coverage from both Rust (`llvm-cov`) and TypeScript (`vitest --coverage`) is uploaded to Codecov as separate flags.

## Conventions worth knowing

- Use `git mv` to rename/move files — preserves history.
- Frontmatter tags are parsed with `serde_yml` (`frontmatter::extract_tags`) — only `tags:` matters. Parsing is strict: malformed YAML in the frontmatter body yields no tags (no line-by-line scraping). The boundary detection is still separate (`find_closing_fm`), since serde doesn't know about `---` markers.
- The closing-frontmatter marker is detected by `find_closing_fm`, which requires `---` to stand alone on a line. Use this helper anywhere you previously would have written `s.find("\n---")`.
- Inline-tag rewrites must go through `replace_inline_tag` (right-boundary check), so `#foo` does not match inside `#foobar`/`#foo-extra`.
- Vault-wide walks (`search`, `rename_tag`) go through `walk::md_files` (the `ignore` crate, so `.gitignore` and hidden files are respected) and process files in parallel via `rayon`. `follow_links(false)` keeps the walk inside the vault.

## Engineering Principles

Apply these to **everything** written in this repo — production code, tests, scripts, config.

- **KISS** — keep it simple. Prefer the most straightforward solution that works. No clever code where plain code does the job.
- **YAGNI** — build only what the current task requires. No speculative features, options, abstractions, or "future-proofing" for requirements that don't exist yet.
- **DRY** — no duplicated knowledge. Extract a shared helper when the *same* logic appears in multiple places — but don't over-DRY: two superficially similar lines that may diverge are not duplication. KISS/YAGNI win ties.
- **SOLID**
  - **S** — Single responsibility: each module, component, hook, or function does one thing.
  - **O** — Open/closed: extend behavior via new code (new ports, props, variants), not by editing stable internals.
  - **L** — Liskov substitution: implementations of a port/interface must be interchangeable without surprising callers.
  - **I** — Interface segregation: keep props and trait surfaces narrow; don't force consumers to depend on what they don't use.
  - **D** — Dependency inversion: keep the domain layer (`vault/`) free of transport/CLI concerns; concretes are wired together in `main::run_server` (`VaultManager` → `Arc` → `ObsidianHandler`). This repo has no `*Provider`/port abstraction layer and doesn't need one — a full DI/hexagonal setup for a single-process CLI MCP server would be over-engineering (YAGNI/KISS win here).

Principle conflicts resolve toward simplicity: KISS and YAGNI take precedence over premature SOLID/DRY structure.
