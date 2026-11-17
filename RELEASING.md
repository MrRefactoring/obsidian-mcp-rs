# Preparing a release

## npm access through OIDC

Publishing the eight packages uses GitHub Actions Trusted Publishing. The workflow
does not need a persistent `NPM_TOKEN`. Configure the trusted relationship for each
package as follows:

| npm field | Value |
|---|---|
| Publisher | GitHub Actions |
| Organization or user | `MrRefactoring` |
| Repository | `obsidian-mcp-rs` |
| Workflow filename | `release.yml` |
| Environment name | empty |
| Allow npm publish | enabled |
| Allow npm dist-tag | disabled |

Configure the relationship under Settings → Trusted Publisher for `obsidian-mcp-rs`
and the seven `@obsidian-mcp-rs/` packages: `darwin-arm64`, `darwin-x64`,
`linux-arm64`, `linux-x64`, `linux-x64-musl`, `win32-arm64`, and `win32-x64`.

The workflow grants the npm job `id-token: write` and installs npm CLI 11 under
Node.js 22. See the [npm documentation](https://docs.npmjs.com/trusted-publishers/)
for setup and CLI requirements.

A new relationship expires unless the first successful publication happens within two
days. Save the settings when a verified release is ready to publish. `npm publish --dry-run`, `npm whoami`, and a rerun that skips already published versions do not
verify OIDC publishing.

When migrating from a token, first verify actual OIDC publication of all eight
packages. Then remove the GitHub `NPM_TOKEN` secret, revoke the unneeded npm token,
and disable publishing through bypass-2FA tokens in the package settings. Do not remove
`CARGO_REGISTRY_TOKEN`: the separate crates.io job needs it.

## Versions and checks

Prepare the release on a separate branch:

```bash
./scripts/prepare-release.sh 0.8.1
```

The script aligns Cargo, the eight npm packages, `optionalDependencies`, and the
lockfile. Fill in the generated section of `CHANGELOG.md` without adding `[Unreleased]`.
Review the diff and run the checks in [CONTRIBUTING.md](CONTRIBUTING.md). Wait for the
full CI matrix on the final PR commit.

## Publishing and verification

After merging the PR, the tag must point to a `master` commit whose version matches
Cargo and all npm packages. The workflow verifies this before building.

The GitHub Release must contain seven binaries with their existing names,
`checksums.txt`, and provenance attestations. Check the crate version on crates.io,
the versions and `latest` tags of all eight npm packages, tarball SRI, and that the
SHA-256 of the packaged binaries matches `checksums.txt`.

Run the published package under Node.js 22 with a separate npm cache and test vault.
Check `--version`, MCP initialize, tools/list, and read_note, and confirm stdout has
no diagnostics. Do not run a test `install` against personal configurations.

After fixing a failure, rerun the failed jobs in the existing workflow. Already
published versions are skipped. Actions artifacts are retained for one day; if they
have expired, a rebuild is required.

Auto-update starts distributing a release 48 hours after the GitHub Release is
published. To withdraw a release, mark it as a prerelease: installed servers follow
`releases/latest` in both directions.
