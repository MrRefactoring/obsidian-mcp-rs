# Changelog

## [0.6.0] - 2026-07-13

A correctness release. Two things shipped in 0.5.0 were not what they said they
were: `--no-edit` advertised every write tool it then refused, and the installer
rewrote whole config files it was only meant to add one key to. Both are fixed,
along with a batch of tool-surface bugs that made the server harder for a model
to use than it needed to be.

### Changed

- **The server now names its vaults in its own instructions.** It used to tell the model *"call `list-available-vaults` to discover vault names"* — a guaranteed round-trip at the start of every single conversation, to learn something the server already knew when it started. It can now open with the work.

### Fixed
- **The five tools that answer with structured content could not report an error the model would see.** `frontmatter`, `wikilinks`, `search-vault`, `periodic` and `vault-info` returned rmcp's `Json<T>`, which has a success shape and nothing else — so "that note does not exist" left the server as a **JSON-RPC protocol error**, the shape the spec reserves for a request the server could not process at all, and one a client is entitled to handle itself rather than hand to the model. The model got nothing back to correct, on precisely the errors it is best placed to correct. These tools now build their own `CallToolResult`, which carries `structuredContent` *and* `isError`: a successful call is byte for byte what it was, and `output_schema` on the `#[tool]` attribute keeps the `outputSchema` the return type used to derive. Proven over the wire, not against a struct field — `the_structured_tools_keep_their_output_schema_and_can_report_is_error`.
- **`periodic` could still pull an unbounded note into the model's context.** `read-note` was capped and `periodic` was not, so "open today's note" was left as a way around the cap — and while a daily note is small, a yearly one is a year of appends. It is now under the same cap, and since it hands back the note's `path`, the marker naming `read-note` and an offset is all the model needs to page on. Test: `periodic_is_under_the_same_cap_as_read_note`.
- **The README had two install sections.** `## Setup` (the recommended `npx` path) and a leftover `## Installation` that sat between *Performance* and *Configuration*, said less, and led with `npm install -g` — the one path the page otherwise never recommends. In the Russian README the two were literally both titled `## Установка`. Removed; `## Setup` was always the better of the two.

- **`search-vault` returned an identifier no note tool would accept.** Each hit carried a `filename` field holding the bare stem — `deep`, with the folder and the extension thrown away — alongside the usable `path`. The note tools' schema then said *"do not include path separators"*, so the model reached for the one field that could not work: `read-note` looked for `deep.md` in the vault root and reported the note missing. **Find-then-read is the most common two-step there is, and it broke on every note in a folder.** The constraint was also false — `safe_join` accepts a relative path perfectly well. `filename` is gone from the results, `path` is the only identifier, and the schema now says so. Test: `what_search_returns_is_what_read_note_accepts`.
- **A patch target copied out of the outline did not resolve.** `read-note view="outline"` exists to tell the patcher what it can aim at, and both `edit-note` and the `TargetNotFound` error point the model at it. But it printed `## Log (line 9)`, and a model that copied that line faithfully got `TargetNotFound` — the ` (line 9)` came with it. Targets are now quoted, with nothing but the target inside the quotes. The old test passed throughout because it checked the *parsed* headings rather than the bytes we actually emit; the new one asserts on the rendered outline. Test: `a_target_copied_verbatim_out_of_the_rendered_outline_resolves`.
- **`add-tags` and `remove-tags` reported success for notes that don't exist.** Files they couldn't find were silently skipped, and the tool then answered `Added tags ["x"] to 0 file(s):` with `isError: false` — which a model reads as success and moves on from. Nothing signalled the work hadn't happened. Every file in a batch is now resolved *before* any of them is written, so a batch either applies or fails naming what it couldn't find, and never half-applies. Tests: `add_tags_names_the_files_it_could_not_find`, `a_batch_with_one_missing_file_changes_nothing_at_all`.
- **`add-tags` took `location` and `position` as untyped strings, so a typo silently did something else.** `location: "Frontmatter"` — one capital letter — fell through the catch-all arm and wrote the tag to **both** places; `position: "begining"` quietly became `end`. This is the exact bug the project already fixed for `searchType` (*"an unrecognised value silently degraded to `content`, so a typo returned the wrong kind of results with no indication of why"*) and then typed every other vocabulary — `add-tags` was the one that got missed. Both are now enums: an unknown value is rejected as `INVALID_PARAMS` naming the offending input, and the legal values reach the model in the schema.
- **`wikilinks` had no limit at all**, and `broken`/`orphans` on a neglected vault run to thousands of rows — the same context-flooding hole `search-vault` was built to avoid. It now pages (`limit`, default 50; `offset`) and reports `total`/`truncated`. `vault-info query="tags"` honours `limit` too; it previously documented it as "ignored", which was honest but left no way to cap a 500-tag list.
- **Errors that left the model with nothing to correct.** `SearchTextNotFound` never said *what* had been searched for — it now echoes the needle and explains that `search` must match byte for byte. And missing-argument errors were reported as `InvalidPath`, so the model was told *"Invalid path: the 'set' action needs a 'value'"* and went off to fix the `filename`, which was the one thing that had been right; they are now a `MissingArgument` of their own.
- **The npm wrapper could orphan the server.** It used `spawnSync`, which blocks Node's event loop for the entire life of the server — so Node could never run a signal handler, and killing the wrapper left the Rust process alive, still holding the vault. Usually masked (with stdio inherited, the child exits on EOF when the client goes away), but a client that dies while that pipe stays open leaves the process behind. It now uses `spawn`, forwards `SIGINT`/`SIGTERM`/`SIGHUP`, and reports the child's exit as its own. Verified both ways: reverting to `spawnSync` reproduces the orphan.
- **Ranked search and `rename-tag` named notes differently from every other tool on Windows.** Both built their relative path with `Path::display()` instead of the `rel_path` helper the link graph and the other search paths use, so on Windows they returned `sub\deep.md` while everything else returned `sub/deep.md` — the same note with two names, depending on which tool you asked. Every outgoing path now goes through the one helper. Found by the CI Windows leg, not by reading the code. Test: `every_tool_names_a_note_the_same_way`.
- **The README contradicted itself and the code.** It gave three different tool counts on one page (15, 12, 11) while documenting only 11 of the 15 — `wikilinks`, `frontmatter`, `periodic` and `vault-info` had no reference entry at all, despite being headline features. It claimed Rust 1.94+ (the MSRV is 1.88). It said *"no Node.js required"* while every setup path it offered ran `npx`. And its read-only section listed 8 blocked tools **without mentioning `periodic`, which creates notes, or `frontmatter`, which writes YAML** — both are correctly gated per-action in the code, but a reader reasoning about safety from that list got a materially wrong picture of what the AI could write.
- **A mistyped vault path produced a server that worked, and was permanently empty.** Nothing checked that the configured directory existed. The assistant would search it, find nothing, and report — with `isError: false` — that there was nothing there; the user concluded the tool was broken, or that their notes were gone. The one warning we printed went to stderr, which every MCP client swallows. Now: `install` **refuses** a path that doesn't exist (that is where the typo is made, and the only moment the user is still looking at a terminal) and says so if the directory has no `.obsidian/`; the interactive wizard asks again instead of writing the bad path; every tool reports `VaultUnavailable` naming the path rather than answering as though the vault were empty; and `list-available-vaults` marks it `(MISSING — this directory does not exist)`. Tests: `a_vault_whose_directory_is_gone_says_so_instead_of_looking_empty`.
- **The installer rewrote the whole of a config file it was only supposed to add one key to.** `serde_json`'s `Value` is a `BTreeMap` and the entry was written with `to_string_pretty`, so every key in the user's file was re-sorted alphabetically and every compact line exploded. Other servers survived, but the file was no longer the user's file. The project already forbids exactly this for note frontmatter — *"a full YAML round-trip reformats the user's whole block"* — and already uses `toml_edit` for the Codex config for exactly this reason; the JSON path was the one that hadn't caught up. Configs are now edited through a JSONC CST (`jsonc-parser`): **only the `obsidian` key is touched**, and key order, formatting, and other servers come through byte for byte.
- **A config with comments in it could not be installed at all.** VS Code's `mcp.json` officially permits them, and a single `// comment` made `install` fail with `key must be a string at line 2 column 3` — while `list` had just reported the same file as "not set", having swallowed the parse error. A dead end. Comments are now parsed, **preserved on write**, and an unreadable config is reported as `unreadable` rather than misfiled as "not installed".
- **Backups were made and never mentioned; uninstall left them behind.** `mcp.json.bak` was written on every install, in silence — an installer that edits files you did not write earns its trust by saying what it did. It now prints where the previous config went. The backup name no longer hunts for a free `.bak.1`, `.bak.2`, … (in `.cursor/` and `.vscode/` those pile up inside the user's git repo), and a successful `uninstall` deletes the backup, so nothing of ours is left behind.
- **Re-installing with `--no-edit` was a silent no-op that still said "restart your client".** The realistic panic path — give the AI write access, think better of it, re-run with the safe flag — left the config untouched, with full write access, and told the user to restart. They did, and believed they were read-only. The installer now compares the installed entry against the one you asked for and says so when they differ; and it no longer tells you to restart when it changed nothing.
- **`obsidian-mcp-rs logs` could not read a custom log.** The README documents `--log-file` and, on the same page, tells you to run `logs` when reporting a bug. Together they didn't work: `logs` always read the default path, so the person already debugging — that is, the person filing the issue — was shown a stale log while the real one sat elsewhere. `logs --log-file <path>` now exists.
- **Colliding vault names were disambiguated by argument order.** Two vaults both called `notes` became `notes` and `notes-2` — assigned positionally, so reordering the paths in a config silently swapped which was which, and "save this to notes" wrote to the other vault. They are now named for their parent folder (`work/notes`, `personal/notes`), which is stable under reordering and actually tells the model which is which. Tests: `a_name_collision_is_broken_by_the_parent_folder`, `reordering_the_vault_paths_does_not_swap_which_is_which`.

## [0.5.1] - 2026-07-13

### Fixed

- **`move-note` turned a rename into a relocation, and deleted the folder the note came from.** Renaming `projects/old.md` to `new.md` — naming the note, its folder, and a new filename, but no `newFolder`, because the folder wasn't changing — moved the note to the **vault root** and then pruned the now-empty `projects/`. An omitted `newFolder` was read as "the vault root" rather than "leave it where it is". It now means the latter; pass `newFolder: ""` to ask for the root explicitly. A `move-note` that names neither a new folder nor a new name is refused outright rather than guessed at, the way `edit-note` already refuses half a target. Tests: `renaming_a_note_leaves_it_in_its_folder`, `an_empty_new_folder_moves_the_note_to_the_vault_root`, `a_move_that_moves_nothing_is_rejected`.
- **`read-note` had no limit and could consume the model's entire context in one call.** `search-vault` has been capped since it was written — *"defaults chosen so a careless query can't flood the model's context"* — but the tool that returns note *bodies* was not: a 440 KB note came back as ~112,000 tokens, and the session was over. Reads now return the first 400 lines by default, with `offset`/`limit` to page (the offsets are the same line numbers `view: "outline"` prints, so one can be pasted straight into the other), and a marker saying which lines you got and how to ask for the rest. A byte ceiling applies on top, because a line cap is no guarantee on its own — a note can be a single 400 KB line. A note that fits comes back byte for byte, marker-free. Tests: `a_note_that_fits_comes_back_byte_for_byte`, `a_long_note_is_cut_and_says_how_to_get_the_rest`, `one_enormous_line_cannot_flood_the_context`, `a_multibyte_character_is_never_split_in_half`.
- **`--no-edit` did not actually hide the write tools, though 0.5.0 said it did.** `with_options` pruned them out of the router, but `#[tool_handler]` defaults to `Self::tool_router()` — a *fresh* router built from the `#[tool]` attributes — so the pruned one was never consulted and `tools/list` advertised all eight write tools, `delete-note` included. Vaults were never at risk: `check_write` still refused every call. But a read-only server was describing itself as a read-write one, which is the whole thing the change was for. The unit test passed throughout because it asserted on the pruned *field* instead of the protocol; the guard is now an integration test that asks a real server over a real stdio transport (`no_edit_does_not_advertise_the_write_tools_over_the_wire`), and it fails without the fix.

## [0.5.0] - 2026-07-13

### Added

- **`search-vault` gained `regex: true` and a `frontmatter` filter.** BM25 answers "which notes are *about* this", but not "which lines match this *shape*" (a phone number, a `TODO(name)`, a URL) or "which notes *are* this" (`status: active`). The two questions BM25 can't answer are now separate arguments rather than a second search tool. `regex: true` reads the query as a regular expression — results are then ranked by how many lines matched, since relevance has no meaning for a pattern — and an unparseable pattern comes back as `INVALID_PARAMS` quoting the pattern and pointing the way out, instead of a bare "search failed". `frontmatter: {"status": "active"}` keeps only notes carrying those fields; a *list* field matches when it **contains** the value, so `{"tags": "work"}` finds a note with `tags: [work, urgent]`. The filter composes with any query, and works on its own with an empty query as a pure metadata lookup. Both are computed inside the walk that already reads every note, so neither adds a pass. The regex engine is capped (`size_limit`), and the `regex` crate has no backtracking, so a pathological pattern cannot hang the server. Tests: `a_filter_alone_answers_which_notes_carry_a_field`, `a_list_field_matches_when_it_contains_the_value`, `regex_ranks_by_how_many_lines_matched`, `an_invalid_regex_is_reported_not_swallowed`.
- **Streamable HTTP transport (`--http`), behind an optional `http` cargo feature.** stdio gives one client one server, spawned as a child process; HTTP lets several clients — or a client that isn't allowed to spawn processes — share one long-lived server, and therefore one `VaultManager` and one write lock, so two HTTP clients editing the same note serialise exactly as two stdio calls do. `--host`/`--port` set the bind address; the endpoint is `http://host:port/mcp`. **stdio remains the default and is untouched.**
  - **Off by default**, because the HTTP stack costs **+1.2 MB (+23%)** of binary and the npm packages ship prebuilt binaries for seven platforms, where that is a download the user pays for. `cargo install obsidian-mcp-rs --features http` to get it; a build without it says so instead of silently falling back to stdio.
  - **The `Origin` header is validated, and this is load-bearing.** The server has full read/write access to the vault and *no authentication*. Bound to `127.0.0.1` that sounds safe, but any web page the user visits can make their browser POST to `http://127.0.0.1:<port>/mcp` — the request comes from their own machine and there is no password to fail. That's DNS rebinding, and the MCP spec requires local servers to defend against it. A request whose `Origin` names a non-localhost site is refused with 403; native MCP clients send no `Origin` at all, so it costs them nothing. Verified end to end: `https://evil.com` and the near-miss `http://localhost.evil.com` are both refused, while `http://localhost:5173` is allowed. Tests: `the_users_own_machine_is_allowed`, `a_web_page_is_not`.
  - Binding to a non-loopback address warns loudly, on stderr and in the log.
  - CI compiles and tests the feature, or it would rot unnoticed.

- **`periodic` tool — today's daily note, and its weekly/monthly/quarterly/yearly siblings.** "Add this to today's note" is one of the things people most want an agent to do with a vault, and exactly the thing it cannot do by guessing: the note's name and folder are whatever the user configured in Obsidian, and getting either wrong creates a stray note instead of appending to the real one. So we don't guess — we read Obsidian's own settings (the Periodic Notes plugin's `data.json` first, then core's `daily-notes.json`, then Obsidian's documented defaults) and land where Obsidian would, including its moment.js name formats (`YYYY-MM-DD`, `gggg-[W]ww`, `[Week] ww`, …) and its template with `{{date}}`/`{{title}}` filled in. `action: "create"` is idempotent — an agent asking for today's note wants the note, not an error because it already exists. `list` walks back one period at a time and asks whether each note exists, so it never has to parse a filename back into a date. Tests: `renders_obsidians_default_formats`, `iso_week_year_is_not_the_calendar_year`, `the_periodic_notes_plugin_wins_over_core`, `periodic_create_is_idempotent`, `a_template_outside_the_vault_is_ignored_not_read`.
- **`vault-info` tool — what's actually in this vault?** A model pointed at an unfamiliar vault could search it, but only for words it already knew; it had no way to ask what tags exist, what was worked on recently, or how big the place is — the questions you ask *before* you know what to search for. `query: "tags"` lists every tag with how many notes carry it (frontmatter and inline `#tags` both count, deduplicated per note, commonest first); `"recent"` lists notes by last modified, newest first; `"stats"` gives notes, folders, bytes, distinct tags, links and broken links. Computed from the same parallel walk as everything else, so nothing is cached and nothing can go stale. A `#tag` inside a code fence is not counted — it's a comment. Tests: `tags_counts_notes_not_occurrences`, `a_tag_inside_a_code_block_is_not_a_tag`, `stats_describe_the_vault`, `recent_lists_newest_first`.

### Changed

- **`delete-note` now moves the note to the vault's `.trash/` instead of erasing it.** An agent deleting the wrong note is a plausible mistake and, until now, an unrecoverable one. `.trash` is hidden and `md_files` skips hidden directories, so a trashed note vanishes from search, the link graph and `rename-tag` exactly as if it were gone — but the user can still get it back. This is what Obsidian itself does. Pass `permanent: true` to erase; deleting a note that is *already* in the trash erases it, so emptying the trash still works. The note's folder is mirrored inside the trash (`a/note.md` and `b/note.md` don't collide) and a repeat delete of the same path becomes `note-2.md` rather than overwriting the first. Tests: `delete_moves_the_note_to_the_trash_by_default`, `a_trashed_note_is_invisible_to_search`, `trashing_the_same_path_twice_does_not_overwrite_the_first`, `deleting_from_the_trash_erases_it`.
- **`--no-edit` now hides the write tools from `tools/list`** instead of advertising them and rejecting the call. A tool the model can see is a tool it will try, and a rejection it then has to spend a turn recovering from; a read-only server should describe itself as one. The eight write-only tools are removed from the router, so they are absent from `tools/list` *and* unreachable via `tools/call`. `frontmatter` stays listed because `get` is a read — it is gated per action. Tests: `no_edit_hides_write_tools_from_the_tool_list`, `every_tool_is_listed_when_writes_are_allowed`.
- **`NoteNotFound` now tells the model how to recover** ("Check the folder, or run search-vault with searchType=\"filename\" to locate it"), matching the hint already on `TargetNotFound`.

## [0.4.0] - 2026-07-13

### Added

- **`frontmatter` tool — read and write any YAML key, not just `tags`.** `action: get` returns the whole frontmatter as structured JSON (or one `key`); `set` writes `key` = `value` (string, number, boolean, list or object); `remove` deletes a key. **Writes are line surgery on the one key named**: every other line of the block — comments, key order, quoting, nested mappings — survives byte for byte. A `serde_yml` round-trip would have been a fraction of the code and would have reformatted the user's whole frontmatter on every write. Under `--no-edit` the gate is per-action: `get` still works, `set`/`remove` are refused. Tests: `frontmatter_set_writes_one_key_and_preserves_the_rest`, `set_field_leaves_comments_and_key_order_alone`, `no_edit_allows_frontmatter_get`, `frontmatter_blocks_traversal`.
- **`edit-note` can be aimed at one section or one block.** Pass `targetType: "heading"` + `target: "## Log"` (the `#` is optional, matching is case-insensitive) to edit a heading's section — the heading line and everything under it, including nested headings, up to the next heading of the same or a higher level. Or `targetType: "block"` + `target: "^n1"` for an Obsidian block reference. Only those bytes are rewritten; the rest of the note is passed through untouched. Previously the only way to change one section was to `replace` the whole note, which silently loses whatever the model failed to reproduce. `replace` on a heading keeps the heading line, and `find_and_replace` is confined to the region rather than hitting the first match anywhere in the file. A target that doesn't exist is an `isError` result naming the outline — never a whole-note overwrite. Tests: `patching_a_section_leaves_the_rest_of_the_note_byte_for_byte`, `a_missing_target_is_an_error_not_a_whole_note_overwrite`, `find_and_replace_is_confined_to_the_region`.
- **`read-note` gained `view: "outline"`** — returns just the note's headings (with levels and line numbers), its `^block-id` references, and its frontmatter keys, instead of the whole text. This is the discovery step for the patch targets above: the outline and the patcher share one set of scanners, so every target the outline offers is one the patcher can find (test: `every_heading_the_outline_offers_can_actually_be_found`). Code fences, the frontmatter, `#tag` lines and `2^n` are all correctly excluded from being read as targets.
- **BM25-ranked search.** `search-vault` scores hits with BM25 and returns them best-first, weighting terms by where they occur (filename ×5, tags ×4, headings ×3, frontmatter ×2, body ×1). It is computed straight from the parallel vault walk — no index to build, no watcher, nothing to go stale. Results are now capped (`limit`/`offset`, and `maxMatchesPerFile`) with `total` and `truncated` reported, so a common word can no longer flood the model's context with thousands of lines.
- **`wikilinks` tool — the vault's link graph.** One tool with `query: backlinks | outgoing | broken | orphans`, built in a single parallel pass. Links inside code fences and inline code are excluded: a `[[link]]` in a code sample is documentation, not a reference.

### Fixed

- **Concurrent writes to the same note lost updates.** The MCP server answers requests concurrently, and every write tool is a read-modify-write — read the note, edit the text, write it back. `atomic_write` made each *write* atomic but not the read-modify-write *pair*, so two calls against one note both read the original and the second write silently discarded the first one's edit. Reproduced over MCP stdio: four writes to one note in a single batch, and only one survived. Mutations now take a server-wide write lock held across the whole read-modify-write; reads are deliberately not locked, since `atomic_write` renames into place and a reader therefore sees the old note or the new one, never a torn one. Tests: `concurrent_edits_to_one_note_do_not_lose_updates`, `concurrent_tag_and_frontmatter_writes_do_not_lose_each_other` (both fail without the lock).
- **`move-note` silently broke every inbound link.** It was a bare `fs::rename`, so every `[[wikilink]]` and markdown link pointing at the moved note was left dangling with no indication anything had happened. It now resolves the graph *before* the move and rewrites the links that pointed at the note, preserving alias/heading/embed syntax and the link flavour, and reports which notes it touched. Links inside code blocks are left alone. Tests: `move_note_rewrites_inbound_links_on_rename`, `move_note_leaves_links_in_code_blocks_alone`.
- **An unknown `edit-note` `operation` reached the domain as a string.** `operation` is now a typed enum, so an unrecognised value is rejected as `INVALID_PARAMS` naming the offending input, and the tool's `inputSchema` advertises the four legal operations. Test: `unknown_operation_is_rejected`.

- **`remove-tags` and `rename-tag` destroyed the frontmatter block.** Both rebuilt the note with the closing `---` glued onto the last frontmatter line (and a stray blank line after the opening marker), so `---\ntags:\n  - keep\n---` came back as `---\n\ntags:\n  - keep---`. Obsidian then stops recognising the frontmatter entirely and **every tag on the note reads back as absent** — and `rename-tag` did this to every matching note in the vault in a single call. The three hand-rolled split/reassemble routines are now one `edit_frontmatter` helper, so the marker can only be written in one place. Tests: `remove_tags_keeps_frontmatter_block_intact`, `rename_tag_keeps_frontmatter_block_intact`.
- **Adding a tag to inline frontmatter (`tags: [a, b]`) produced invalid YAML.** The new tag was appended as a block item *below* the inline line (`tags: [a, b]\n  - c`), which fails to parse — silently dropping every tag on the note. Inline lists now stay inline (`tags: [a, b, c]`). Test: `add_tag_to_inline_list_stays_inline_and_parses`.
- **`remove-tags` / `rename-tag` ignored inline and scalar frontmatter.** Only block-list items were rewritten, so `tags: [old]` and `tags: old` kept the old tag while the note was rewritten anyway. All three YAML shapes (block, inline, scalar) are now handled; a scalar gains a block list when a second tag is added, and removing the last tag leaves a valid empty list. Tests: `remove_tag_from_inline_list`, `rename_tag_in_inline_list`, `rename_scalar_tag`, `remove_scalar_tag`, `add_tag_to_scalar_promotes_it_to_a_block_list`.
- **`remove-tags` / `rename-tag` collaterally edited unrelated frontmatter lists.** The line filter matched `- value` anywhere in the frontmatter, so removing tag `x` also deleted `- x` from `aliases:`. Edits are now confined to the `tags:` field's own lines; every other key — including comments, quoting, and dates — survives byte for byte. Tests: `remove_tags_does_not_touch_a_matching_alias`, `rename_tag_does_not_touch_a_matching_alias`, `unrelated_frontmatter_survives_byte_for_byte`.
- **Tag search and tag rewrites disagreed on where a tag ends.** `search-vault`'s `tag:` query used a bare `contains("#tag")`, so `tag:foo` matched `#foobar`, while `rename-tag` (which does check the boundary) then declined to change the file. Both now share one boundary rule, which also gained a left boundary — `C#foo` and the fragment in `](http://x#foo)` no longer read as tags. Nested tags keep working as Obsidian defines them: searching `parent` finds `#parent/child`, but renaming `parent` does **not** rewrite `#parent/child`. Tests: `inline_tag_match_requires_a_right_boundary`, `inline_tag_match_requires_a_left_boundary`, `nested_tags_match_the_parent_when_searching_only`, `rename_leaves_nested_tags_alone`.
- **An unrecognised `searchType` silently degraded to `content`**, so a typo returned the wrong kind of results with no indication of why. `searchType` is now a typed enum: unknown values are rejected as `INVALID_PARAMS`, and the tool's `inputSchema` advertises the legal values (`content` / `filename` / `both`) instead of burying them in prose. Tests: `unknown_search_type_is_rejected`, `known_search_types_parse`.
- **Doc drift:** `add-tags`'s `normalize` parameter claimed `ProjectActive -> project-active`; `normalize_tag` does not split camelCase (it lowercases to `projectactive`). The description now states what the code does (`"My Tag" -> my-tag`).

## [0.3.0] - 2026-07-11

### Added

- **Atomic note writes.** `create-note`, `edit-note`, `add-tags`, `remove-tags`, and `rename-tag` now write to a sibling temp file and `rename` it over the target (`vault::write::atomic_write`), so a crash or concurrent write can never leave a half-written or truncated note — only the whole old or whole new content. `move-note` already used `fs::rename` and is unchanged. Tests: `writes_full_contents_and_leaves_no_temp`, `overwrites_existing_file`, `temp_path_is_sibling_of_target`.
- **`search-vault` now returns MCP `structuredContent` with a declared `outputSchema`.** The tool returns a typed `Json<SearchOutput>` (`{ results: [{ filename, path, matches }] }`); rmcp advertises the derived `outputSchema` in `tools/list` and fills both `structuredContent` and the text block (serialized JSON), so clients and the model consume hits without parsing prose. Tests: `search_vault_returns_structured_content`, `search_vault_empty_still_has_structured_content`.
- **Tool annotations and richer server identity.** Every tool now carries MCP hints — `readOnlyHint` on `read-note`/`search-vault`/`list-available-vaults`, `destructiveHint` on `delete-note`/`edit-note`/`move-note`/`remove-tags`/`rename-tag`, `openWorldHint = false` on all (a local vault is a closed world), plus a human-readable `title`. The `initialize` response now sets `instructions` and a proper `serverInfo` (see Changed). This lets clients such as Claude auto-approve read-only calls and warn before destructive ones.
- **Size-based log rotation.** At startup `main::rotate_if_large` rolls the log to `<path>.1` once it passes 5 MiB (keeping one backup), so the file no longer grows without bound. The current log path stays stable, so `logs` and the documented location are unchanged. Tests: `rotate_moves_oversized_file_to_backup`, `rotate_leaves_small_file_untouched`, `rotate_replaces_previous_backup`, `rotate_ignores_missing_file`.
- **End-to-end MCP stdio test** (`tests/mcp_stdio.rs`) — spawns the built binary and drives a full JSON-RPC handshake (`initialize` → `initialized` → `tools/list` → `tools/call`) over stdin/stdout, asserting all 11 tools are exposed and a note reads back over the live transport.
- **CI hardening.** `cargo test` now runs on a Linux/macOS/Windows matrix (was Linux-only); new jobs enforce the MSRV (`cargo check` on Rust 1.94, `--locked`) and run `cargo audit`; a `.github/dependabot.yml` keeps Cargo, npm, and GitHub-Actions dependencies current.
- **Prompt-based install is now the primary setup path in the README.** The `## Setup` section leads with a copy-paste prompt that has an agentic client (Claude Code, Cursor, Windsurf, …) run the installer itself, plus the native `claude mcp add obsidian -- npx -y obsidian-mcp-rs <vault>` one-liner; the interactive CLI wizard moves under a "Prefer a CLI?" subsection for non-agentic clients like Claude Desktop. Includes a heads-up that MCP config is read at session start, so a restart (and, for a project-scoped `.mcp.json`, `/mcp` approval) is needed before the tools appear.

### Changed

- **`serverInfo` now identifies this server** as `obsidian-mcp-rs` / its crate version (with a `title` of "Obsidian (Rust MCP)"). Previously the rmcp default surfaced the library's own identity (`rmcp` / the rmcp version) to clients.
- **Tool-execution errors are now reported as `isError: true` results instead of JSON-RPC protocol errors.** Per the MCP spec, business failures the model can recover from — note not found, note/directory already exists, `find_and_replace` search text not found — are returned inside the tool result (`isError: true`) so the model sees them and can self-correct. Genuinely malformed requests (unknown vault, path traversal / absolute path) map to `INVALID_PARAMS` (-32602) and server faults (IO/search) to `INTERNAL_ERROR` (-32603). **Behaviour change:** clients that previously received a JSON-RPC error for a missing note will now receive a successful response carrying `isError: true`. New `VaultError::SearchTextNotFound` and `VaultError::is_tool_execution_error()`; new tests cover the split.
- **Replaced the unmaintained, unsound `serde_yml`/`libyml` YAML stack** (RUSTSEC-2025-0067, RUSTSEC-2025-0068) with the maintained `serde_yaml_ng`, aliased back to `serde_yml` in code so call sites are unchanged. `cargo audit` is now clean. Goose `config.yaml` output is byte-for-byte covered by the existing `install`/`writer` tests.
- **Upgraded rmcp 1.8 → 2.2**, moving the server onto the MCP **2025-11-25** model. It now negotiates protocol version `2025-11-25` with capable clients (older clients still get the version they request). The upgrade aligned model types (internally `Content` → `ContentBlock`) and let `search-vault` adopt the `Json<T>` return idiom (see Added). No MSRV bump was required — the build still checks clean on Rust 1.94 (`cargo +1.94.0 check --all-targets --locked`).
- Refreshed the dependency lockfile (`cargo update`).
- `rustfmt` edition set to 2024 to match `Cargo.toml` (was 2021).

### Fixed

- **The file log was documented as "rotating" but grew without bound.** It now genuinely rotates (size-based, see Added), and the wording in `CLAUDE.md` matches the behaviour.
- **MCP error codes were flattened.** Every `VaultError` mapped to `INTERNAL_ERROR`; codes are now granular (`INVALID_PARAMS` vs `INTERNAL_ERROR`) via `From<VaultError> for rmcp::ErrorData`.
- **Doc drift:** `README.md`, `README.ru.md`, and `llms.txt` said "12 tools"; the server exposes 11.
- **Claude Code local config (`.mcp.json`) now writes `"type": "stdio"`.** The installer emitted the bare `{ command, args }` (`Standard`) form for `.mcp.json` while the global `~/.claude.json` writer already included `"type": "stdio"` — inconsistent, since Claude Code's `.mcp.json` schema uses the typed form. Both Claude Code targets now share the `ClaudeApp` entry shape. New test `write_entry_claude_app_format_has_type_stdio`.
- **Doc drift (Claude Code):** the README config heading "Claude Code / CLAUDE.md" was wrong — `CLAUDE.md` is a memory/instructions file, never an MCP config location. Renamed to "Claude Code (`.mcp.json` / `~/.claude.json`)" and the example now shows `"type": "stdio"`. `llms.txt` still said "rmcp 1.4"; updated to 2.2.
- Four handler tests bound the vault `TempDir` to `_`, dropping it before the call, so they exercised "missing vault root" (an IO error) rather than the intended "missing note"; they now keep the vault alive and assert the real business error.

### Security

- Documented a known, out-of-threat-model TOCTOU nuance in `vault::safe_join`: it returns a lexical (not canonicalized) path, so a symlink component swapped between the check and the caller's filesystem operation could escape. Winning that race requires write access to the vault directory, which already defeats the sandbox's purpose for a local single-user tool, so this is accepted as won't-fix and documented in the code.

## [0.2.1] - 2026-05-22

### Changed

- **`delete-note` now prunes an emptied source folder.** When deleting a note leaves its containing folder empty, that folder is removed too — mirroring the behaviour `move-note` gained in 0.2.0. The cleanup is best-effort (a failed `remove_dir` is logged via `tracing::warn!`, never propagated, so it can't fail the delete) and the vault root is never removed. The empty-folder pruning shared by `move-note` and `delete-note` is now a single `prune_empty_parent` helper. Tests: `delete_note_removes_emptied_source_folder`, `delete_note_keeps_nonempty_source_folder`, `delete_note_does_not_remove_vault_root`.

## [0.2.0] - 2026-05-22

### Changed

- **Internal refactor, no behavioural change** (same public MCP API, same config-file output). Split the 1700-line `src/vault.rs` into a `src/vault/` module — `mod.rs` (the `VaultManager` orchestrator), `path.rs` (`safe_join` sandbox), `frontmatter.rs` (parsing + `find_closing_fm`), `tags.rs` (tag operations + `replace_inline_tag`), `search.rs` (`SearchResult`/`SearchType` + the walk). Tests moved alongside the code they cover. All 190 tests stay green; `cargo clippy -- -D warnings` and `cargo fmt --check` are clean.
- `install/writer.rs` reworked around a `ConfigBackend` trait (`JsonBackend` parameterised by entry-path + builder, `TomlBackend`, `YamlBackend`), dispatched from a single `backend(format)` match. Adding a new JSON-shaped client is now one match arm instead of editing five `match`-on-`ConfigFormat` blocks. The dir/backup/write sequence is consolidated into one `write_with_backup` helper.
- `add_tags_to_frontmatter` flattened from four nested branches into early-return guard clauses; output is byte-for-byte identical.
- Frontmatter `tags` parsing moved from the hand-rolled line scanner to `serde_yml` (`frontmatter::extract_tags`), eliminating a custom YAML subset parser. Boundary detection still uses `find_closing_fm` (serde does not handle `---` markers). **Behaviour change:** parsing is now strict — a note whose frontmatter body is *invalid* YAML yields no tags instead of being scraped line-by-line, and non-string tag values (e.g. `tags: [2024]`) are ignored. Well-formed vaults are unaffected.
- Vault-wide walks (`search-vault`, `rename-tag`) replaced `walkdir` with the `ignore` crate via a shared `walk::md_files` helper (de-duplicating the two identical walk loops). **Behaviour change:** `.gitignore` rules and hidden files/folders are now respected, so gitignored or hidden notes are skipped — including by `rename-tag`.

### Performance

- Vault walks now process files in parallel with `rayon` (`search-vault`, `rename-tag`). Measured on a 2000-note synthetic vault (Apple Silicon, 10 logical cores) vs. the same code pinned to one thread: content search ~2.0×, tag search ~1.9×, tag rename (500 notes) ~1.4×.
- Case-insensitive content search lowercases each file once instead of once per line.
- Added a criterion benchmark suite (`benches/vault_bench.rs`) covering content/tag search and tag rename; CI compiles it (`cargo bench --no-run`) so it can't bitrot. This required splitting the crate into a library (`src/lib.rs`) plus a thin binary (`src/main.rs`) so benches and tests can link against the domain logic — `cargo test --lib` now works.

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

- `move-note` now prunes the source folder when the move leaves it empty. The immediate source directory is removed (best-effort — a failed cleanup never fails the move), and the vault root is never deleted. Tests: `move_note_removes_emptied_source_folder`, `move_note_keeps_nonempty_source_folder`, `move_note_does_not_remove_vault_root`.
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

[0.2.1]: https://github.com/MrRefactoring/obsidian-mcp-rs/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/MrRefactoring/obsidian-mcp-rs/compare/v0.1.6...v0.2.0
[0.1.6]: https://github.com/MrRefactoring/obsidian-mcp-rs/releases/tag/v0.1.6
[0.1.5]: https://github.com/MrRefactoring/obsidian-mcp-rs/releases/tag/v0.1.5
[0.1.4]: https://github.com/MrRefactoring/obsidian-mcp-rs/releases/tag/v0.1.4
[0.1.3]: https://github.com/MrRefactoring/obsidian-mcp-rs/releases/tag/v0.1.3
[0.1.2]: https://github.com/MrRefactoring/obsidian-mcp-rs/releases/tag/v0.1.2
[0.1.1]: https://github.com/MrRefactoring/obsidian-mcp-rs/releases/tag/v0.1.1
[0.1.0]: https://github.com/MrRefactoring/obsidian-mcp-rs/releases/tag/v0.1.0
