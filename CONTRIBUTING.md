# Contributing

Small fixes can be submitted directly as pull requests. For a new feature, first
open an issue to agree on the expected behavior and scope. See
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) for community standards and
[SECURITY.md](SECURITY.md) for vulnerability reporting.

## Development setup

Install stable Rust with `rustfmt` and `clippy`, Git, and Node.js 22 or later.
The minimum supported Rust version is 1.89; CI checks this version.

```bash
git clone https://github.com/MrRefactoring/obsidian-mcp-rs.git
cd obsidian-mcp-rs
git switch -c fix/my-change
cargo build
cargo test
```

For the npm wrapper:

```bash
cd npm/obsidian-mcp-rs
npm ci
npm run build
npm test
```

Develop against a separate test vault. Start the server with
`cargo run -- /path/to/test-vault`; add `--no-edit` to test read-only access.

## Checks before a pull request

From the repository root, run:

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo clippy --features http --all-targets -- -D warnings
cargo test
cargo test --features http
cargo bench --no-run
cargo +1.89.0 check --all-targets --locked
cargo audit
```

`cargo audit` requires `cargo-audit`. If the npm wrapper changed, also build and test
it under Node.js 22. CI checks Linux, macOS, Windows, and all seven release targets;
locally, check the available platform and wait for the full CI matrix.

## Invariants to preserve

- In stdio mode, stdout contains only MCP JSON-RPC. Diagnostics go to stderr or the
  log; responses containing note contents must not be logged.
- User-supplied paths go through `vault::safe_join`. A new path parameter needs a
  test for escaping the vault.
- Mutating tools call `check_write()`, and mutating `VaultManager` methods hold
  `write_guard()` throughout reading and writing. Note edits read from disk, not the
  search cache.
- Changing one frontmatter field or config entry preserves other comments, key order,
  and formatting. If the CST cannot support an edit, return an error without writing;
  full serialization of the user file is not acceptable.
- `JsonSchema` parameter descriptions are included in the MCP schema. When changing
  them, check the resulting `tools/list` and description cost to the client.
- Preserve Rust 1.89 and Node.js 22. Do not change package versions or create a tag
  in a regular PR: release preparation runs separately through
  `scripts/prepare-release.sh`. Release and npm OIDC setup instructions are in
  [RELEASING.md](RELEASING.md).

## Submitting changes

Move tracked files with `git mv`. For a behavior fix, add a test that reproduces the
original problem. Do not add tests that only check for documentation text.

Describe the problem, resulting behavior, and checks in the PR. User-visible changes
belong in `CHANGELOG.md`; coordinate the section for a future release with a
maintainer. Update both README versions when changing shared user documentation.

Write commit messages in English, for example
`fix: preserve frontmatter when updating tags`. Do not add sign-offs or generator
attribution to commits, PRs, or source files.
