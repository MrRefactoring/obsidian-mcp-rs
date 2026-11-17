# Security

## Supported versions

Security fixes are released for the latest stable version of `obsidian-mcp-rs`.
If you use an earlier release, first check whether the issue occurs on the latest version.

## Reporting a vulnerability

Contact project owner Vladislav Tupikin at
[mrrefactoring@yandex.ru](mailto:mrrefactoring@yandex.ru).
Do not publish exploitation details in a public issue before coordinating disclosure.

Include the server version, OS, installation method, transport, and minimal reproduction
steps. Describe the expected access boundary and the result of crossing it. If possible,
include an example using a separate test vault. Remove tokens, private note contents,
search queries, and private paths from the report.

After reviewing the report, we will coordinate next steps and a disclosure timeline.
Report ordinary server bugs through the
[bug report template](https://github.com/MrRefactoring/obsidian-mcp-rs/issues/new/choose).

## Security boundaries

- The server performs actions requested by the MCP client with the permissions of the
  user who launched it. By default, notes can be read, created, edited, moved, and
  deleted without separate confirmation of each call.
- `vault::safe_join` checks paths against the vault root and blocks escape through
  `..`, absolute paths, and symbolic links. It does not isolate the vault from another
  process already allowed to modify its directory, or protect against a malicious MCP
  client performing permitted operations inside the vault.
- `--no-edit` removes write-only tools from `tools/list` and blocks mutating actions
  of mixed-purpose tools. Reading remains available.
- The optional HTTP transport checks `Origin` but does not authenticate clients. It is
  intended for a trusted local environment.
- Auto-update of the installed copy uses GitHub Releases after a 48-hour hold and
  verifies the downloaded binary's checksum and version. The checksum verifies download
  integrity and does not replace a publisher signature. Update controls are described
  in [README.md](README.md#updating).
- The log file contains diagnostic information, paths, and search queries. The current
  version does not log responses containing note contents; inspect logs from older
  versions before sharing them.
