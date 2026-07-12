use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Json},
    model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};
use similar::TextDiff;

#[cfg(test)]
use crate::vault::SearchType;
use crate::{
    error::VaultError,
    tools::{
        add_tags::AddTagsParams, create_directory::CreateDirectoryParams,
        create_note::CreateNoteParams, delete_note::DeleteNoteParams, edit_note::EditNoteParams,
        list_vaults::ListVaultsParams, move_note::MoveNoteParams, read_note::ReadNoteParams,
        remove_tags::RemoveTagsParams, rename_tag::RenameTagParams,
        search_vault::SearchVaultParams, wikilinks::WikilinksParams,
    },
    vault::{LinkOutput, SearchOutput, VaultManager},
};

#[derive(Clone)]
pub struct ObsidianHandler {
    vault: Arc<VaultManager>,
    // Populated for the rmcp #[tool_router] macro to dispatch through;
    // dead-code analysis can't see the macro-generated reads.
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
    no_edit: bool,
}

fn ok(text: impl Into<String>) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![ContentBlock::text(
        text.into(),
    )]))
}

fn err(e: VaultError) -> McpError {
    tracing::error!("{}", e);
    // Preserve the granular MCP error code (INVALID_PARAMS vs INTERNAL_ERROR)
    // from the `From<VaultError>` impl instead of flattening everything.
    McpError::from(e)
}

/// Map a failed vault operation onto the correct MCP shape. Per the spec,
/// tool-execution errors (note missing / already exists / search text not found)
/// are returned as `isError: true` results so the model can see and self-correct;
/// malformed-request errors and server faults stay JSON-RPC protocol errors.
fn tool_error(e: VaultError) -> Result<CallToolResult, McpError> {
    if e.is_tool_execution_error() {
        tracing::debug!(error = %e, "tool execution error (isError result)");
        Ok(CallToolResult::error(vec![ContentBlock::text(
            e.to_string(),
        )]))
    } else {
        Err(err(e))
    }
}

#[tool_router]
impl ObsidianHandler {
    #[cfg(test)]
    pub fn new(vault: VaultManager) -> Self {
        Self::with_options(vault, false)
    }

    pub fn with_options(vault: VaultManager, no_edit: bool) -> Self {
        Self {
            vault: Arc::new(vault),
            tool_router: Self::tool_router(),
            no_edit,
        }
    }

    fn check_write(&self) -> Result<(), McpError> {
        if self.no_edit {
            Err(McpError::invalid_request(
                "write operations are disabled: server was started with --no-edit",
                None,
            ))
        } else {
            Ok(())
        }
    }

    /// Read the content of an existing note in the vault.
    #[tool(
        name = "read-note",
        annotations(title = "Read note", read_only_hint = true, open_world_hint = false)
    )]
    fn read_note(
        &self,
        rmcp::handler::server::wrapper::Parameters(ReadNoteParams {
            vault,
            filename,
            folder,
        }): rmcp::handler::server::wrapper::Parameters<ReadNoteParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(tool = "read-note", %vault, %filename);
        match self.vault.read_note(&vault, &filename, folder.as_deref()) {
            Ok(content) => ok(content),
            Err(e) => tool_error(e),
        }
    }

    /// Create a new note in the specified vault with Markdown content.
    #[tool(
        name = "create-note",
        annotations(
            title = "Create note",
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn create_note(
        &self,
        rmcp::handler::server::wrapper::Parameters(CreateNoteParams {
            vault,
            filename,
            content,
            folder,
        }): rmcp::handler::server::wrapper::Parameters<CreateNoteParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(tool = "create-note", %vault, %filename);
        self.check_write()?;
        match self
            .vault
            .create_note(&vault, &filename, &content, folder.as_deref())
        {
            Ok(path) => ok(format!("Created note at {}", path.display())),
            Err(e) => tool_error(e),
        }
    }

    /// Edit an existing note. Operations: append, prepend, replace, find_and_replace.
    #[tool(
        name = "edit-note",
        annotations(
            title = "Edit note",
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn edit_note(
        &self,
        rmcp::handler::server::wrapper::Parameters(EditNoteParams {
            vault,
            filename,
            operation,
            content,
            folder,
            search,
        }): rmcp::handler::server::wrapper::Parameters<EditNoteParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(tool = "edit-note", %vault, %filename, %operation);
        self.check_write()?;
        let (old, new) = match self.vault.edit_note(
            &vault,
            &filename,
            &operation,
            &content,
            folder.as_deref(),
            search.as_deref(),
        ) {
            Ok(v) => v,
            Err(e) => return tool_error(e),
        };
        let diff = TextDiff::from_lines(&old, &new);
        let unified = diff
            .unified_diff()
            .context_radius(3)
            .header(&filename, &filename)
            .to_string();
        ok(format!(
            "Note '{}' updated with operation '{}'\n\n```diff\n{}```",
            filename, operation, unified
        ))
    }

    /// Delete a note from the vault. If this empties its containing folder,
    /// that folder is removed too (the vault root is never deleted).
    #[tool(
        name = "delete-note",
        annotations(
            title = "Delete note",
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn delete_note(
        &self,
        rmcp::handler::server::wrapper::Parameters(DeleteNoteParams {
            vault,
            filename,
            folder,
        }): rmcp::handler::server::wrapper::Parameters<DeleteNoteParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(tool = "delete-note", %vault, %filename);
        self.check_write()?;
        match self.vault.delete_note(&vault, &filename, folder.as_deref()) {
            Ok(()) => ok(format!("Deleted note '{}'", filename)),
            Err(e) => tool_error(e),
        }
    }

    /// Move or rename a note within the vault.
    #[tool(
        name = "move-note",
        annotations(
            title = "Move or rename note",
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn move_note(
        &self,
        rmcp::handler::server::wrapper::Parameters(MoveNoteParams {
            vault,
            filename,
            folder,
            new_folder,
            new_filename,
        }): rmcp::handler::server::wrapper::Parameters<MoveNoteParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(tool = "move-note", %vault, %filename);
        self.check_write()?;
        match self.vault.move_note(
            &vault,
            &filename,
            folder.as_deref(),
            new_folder.as_deref(),
            new_filename.as_deref(),
        ) {
            Ok(outcome) => {
                let mut msg = format!("Moved note to {}", outcome.path.display());
                if !outcome.relinked.is_empty() {
                    msg.push_str(&format!(
                        "\n\nUpdated links in {} note(s): {}",
                        outcome.relinked.len(),
                        outcome.relinked.join(", ")
                    ));
                }
                ok(msg)
            }
            Err(e) => tool_error(e),
        }
    }

    /// Explore the vault's link graph: which notes link here (backlinks), what
    /// this note links to (outgoing), links pointing nowhere (broken), or notes
    /// nothing links to (orphans).
    #[tool(
        name = "wikilinks",
        annotations(
            title = "Explore links",
            read_only_hint = true,
            open_world_hint = false
        )
    )]
    fn wikilinks(
        &self,
        rmcp::handler::server::wrapper::Parameters(WikilinksParams {
            vault,
            query,
            filename,
            folder,
        }): rmcp::handler::server::wrapper::Parameters<WikilinksParams>,
    ) -> Result<Json<LinkOutput>, McpError> {
        tracing::debug!(tool = "wikilinks", %vault, ?query);
        let out = self
            .vault
            .wikilinks(&vault, &query, filename.as_deref(), folder.as_deref())
            .map_err(err)?;
        Ok(Json(out))
    }

    /// Create a new directory in the vault.
    #[tool(
        name = "create-directory",
        annotations(
            title = "Create directory",
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn create_directory(
        &self,
        rmcp::handler::server::wrapper::Parameters(CreateDirectoryParams {
            vault,
            path,
            recursive,
        }): rmcp::handler::server::wrapper::Parameters<CreateDirectoryParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(tool = "create-directory", %vault, %path);
        self.check_write()?;
        match self
            .vault
            .create_directory(&vault, &path, recursive.unwrap_or(true))
        {
            Ok(dir) => ok(format!("Created directory {}", dir.display())),
            Err(e) => tool_error(e),
        }
    }

    /// Search notes by content, filename, or tag ("tag:" prefix). Results are
    /// ranked best-first and capped — read `total` to see how many matched.
    #[tool(
        name = "search-vault",
        annotations(title = "Search vault", read_only_hint = true, open_world_hint = false)
    )]
    fn search_vault(
        &self,
        rmcp::handler::server::wrapper::Parameters(params): rmcp::handler::server::wrapper::Parameters<
            SearchVaultParams,
        >,
    ) -> Result<Json<SearchOutput>, McpError> {
        tracing::debug!(tool = "search-vault", vault = %params.vault, query = %params.query);
        let limits = params.limits();
        let st = params.search_type.unwrap_or_default();

        // Returning `Json<T>` lets rmcp derive the tool's `outputSchema` from
        // `SearchOutput` and emit both `structuredContent` and a JSON text block.
        let out = self
            .vault
            .search_vault(
                &params.vault,
                &params.query,
                params.path.as_deref(),
                params.case_sensitive.unwrap_or(false),
                &st,
                &limits,
            )
            .map_err(err)?;
        Ok(Json(out))
    }

    /// Add tags to notes in frontmatter and/or content.
    #[tool(
        name = "add-tags",
        annotations(
            title = "Add tags",
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn add_tags(
        &self,
        rmcp::handler::server::wrapper::Parameters(AddTagsParams {
            vault,
            files,
            tags,
            location,
            normalize,
            position,
        }): rmcp::handler::server::wrapper::Parameters<AddTagsParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(tool = "add-tags", %vault, ?tags);
        self.check_write()?;
        match self.vault.add_tags(
            &vault,
            &files,
            &tags,
            location.as_deref().unwrap_or("both"),
            normalize.unwrap_or(true),
            position.as_deref().unwrap_or("end"),
        ) {
            Ok(modified) => ok(format!(
                "Added tags {:?} to {} file(s): {}",
                tags,
                modified.len(),
                modified.join(", ")
            )),
            Err(e) => tool_error(e),
        }
    }

    /// Remove tags from notes in frontmatter and content.
    #[tool(
        name = "remove-tags",
        annotations(
            title = "Remove tags",
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn remove_tags(
        &self,
        rmcp::handler::server::wrapper::Parameters(RemoveTagsParams { vault, files, tags }): rmcp::handler::server::wrapper::Parameters<RemoveTagsParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(tool = "remove-tags", %vault, ?tags);
        self.check_write()?;
        match self.vault.remove_tags(&vault, &files, &tags) {
            Ok(modified) => ok(format!(
                "Removed tags {:?} from {} file(s): {}",
                tags,
                modified.len(),
                modified.join(", ")
            )),
            Err(e) => tool_error(e),
        }
    }

    /// Rename a tag across all notes in the vault.
    #[tool(
        name = "rename-tag",
        annotations(
            title = "Rename tag",
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn rename_tag(
        &self,
        rmcp::handler::server::wrapper::Parameters(RenameTagParams {
            vault,
            old_tag,
            new_tag,
        }): rmcp::handler::server::wrapper::Parameters<RenameTagParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(tool = "rename-tag", %vault, %old_tag, %new_tag);
        self.check_write()?;
        match self.vault.rename_tag(&vault, &old_tag, &new_tag) {
            Ok(modified) => ok(format!(
                "Renamed tag '{}' to '{}' in {} file(s): {}",
                old_tag,
                new_tag,
                modified.len(),
                modified.join(", ")
            )),
            Err(e) => tool_error(e),
        }
    }

    /// List all available vaults configured for this server.
    #[tool(
        name = "list-available-vaults",
        annotations(title = "List vaults", read_only_hint = true, open_world_hint = false)
    )]
    fn list_available_vaults(
        &self,
        rmcp::handler::server::wrapper::Parameters(ListVaultsParams {}): rmcp::handler::server::wrapper::Parameters<ListVaultsParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(tool = "list-available-vaults");
        let vaults = self.vault.list_vaults();
        if vaults.is_empty() {
            return ok("No vaults configured.");
        }
        let list = vaults
            .iter()
            .map(|(name, path)| format!("- {} → {}", name, path.display()))
            .collect::<Vec<_>>()
            .join("\n");
        ok(format!("Available vaults:\n{}", list))
    }
}

#[tool_handler]
impl ServerHandler for ObsidianHandler {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            // Identify *this* server, not the rmcp library (whose from_build_env
            // default reports "rmcp"/its own version).
            .with_server_info(
                Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
                    .with_title("Obsidian (Rust MCP)"),
            )
            .with_instructions(
                "Notes live in one or more named vaults. Call `list-available-vaults` to \
                 discover vault names, then pass a `vault` name to every tool. Filenames are \
                 relative to the vault root; the `.md` extension is optional. Tag search uses \
                 a `tag:` prefix in `search-vault`.",
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::handler::server::wrapper::Parameters;
    use std::fs;
    use tempfile::TempDir;

    use crate::tools::{
        add_tags::AddTagsParams, create_directory::CreateDirectoryParams,
        create_note::CreateNoteParams, delete_note::DeleteNoteParams, edit_note::EditNoteParams,
        list_vaults::ListVaultsParams, move_note::MoveNoteParams, read_note::ReadNoteParams,
        remove_tags::RemoveTagsParams, rename_tag::RenameTagParams,
        search_vault::SearchVaultParams,
    };

    fn setup() -> (TempDir, ObsidianHandler, String) {
        let dir = TempDir::new().unwrap();
        let vault_name = dir
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let manager = VaultManager::new(vec![dir.path().to_path_buf()]);
        let handler = ObsidianHandler::new(manager);
        (dir, handler, vault_name)
    }

    fn write(dir: &TempDir, name: &str, content: &str) {
        fs::write(dir.path().join(name), content).unwrap();
    }

    /// Assert a tool-execution error surfaced as an `isError: true` result
    /// (not a JSON-RPC protocol error), per the MCP spec.
    fn assert_is_error(r: Result<CallToolResult, McpError>) {
        let res = r.expect("expected an isError tool result, got a protocol error");
        assert_eq!(res.is_error, Some(true), "expected isError result");
    }

    // ── read-note ─────────────────────────────────────────────────────────────

    #[test]
    fn read_note_ok() {
        let (dir, h, vault) = setup();
        write(&dir, "n.md", "body");
        let r = h.read_note(Parameters(ReadNoteParams {
            vault,
            filename: "n.md".into(),
            folder: None,
        }));
        assert!(r.is_ok());
    }

    #[test]
    fn read_note_not_found_is_tool_error() {
        let (_dir, h, vault) = setup();
        let r = h.read_note(Parameters(ReadNoteParams {
            vault,
            filename: "ghost".into(),
            folder: None,
        }));
        assert_is_error(r);
    }

    #[test]
    fn unknown_vault_is_protocol_error() {
        let (_, h, _) = setup();
        let r = h.read_note(Parameters(ReadNoteParams {
            vault: "no-such-vault".into(),
            filename: "n".into(),
            folder: None,
        }));
        // A bad vault name is a malformed request → JSON-RPC protocol error, not isError.
        assert!(r.is_err());
    }

    // ── create-note ───────────────────────────────────────────────────────────

    #[test]
    fn create_note_ok() {
        let (dir, h, vault) = setup();
        let r = h.create_note(Parameters(CreateNoteParams {
            vault,
            filename: "new.md".into(),
            content: "hi".into(),
            folder: None,
        }));
        assert!(r.is_ok());
        assert!(dir.path().join("new.md").exists());
    }

    #[test]
    fn create_note_duplicate_is_tool_error() {
        let (dir, h, vault) = setup();
        write(&dir, "dup.md", "");
        let r = h.create_note(Parameters(CreateNoteParams {
            vault,
            filename: "dup".into(),
            content: "".into(),
            folder: None,
        }));
        assert_is_error(r);
    }

    // ── edit-note ─────────────────────────────────────────────────────────────

    #[test]
    fn edit_note_append_ok() {
        let (dir, h, vault) = setup();
        write(&dir, "e.md", "a");
        let r = h.edit_note(Parameters(EditNoteParams {
            vault,
            filename: "e.md".into(),
            operation: "append".into(),
            content: "b".into(),
            folder: None,
            search: None,
        }));
        assert!(r.is_ok());
    }

    #[test]
    fn edit_note_missing_is_tool_error() {
        let (_dir, h, vault) = setup();
        let r = h.edit_note(Parameters(EditNoteParams {
            vault,
            filename: "ghost".into(),
            operation: "append".into(),
            content: "x".into(),
            folder: None,
            search: None,
        }));
        assert_is_error(r);
    }

    #[test]
    fn edit_note_search_text_not_found_is_tool_error() {
        let (dir, h, vault) = setup();
        write(&dir, "e.md", "hello world");
        let r = h.edit_note(Parameters(EditNoteParams {
            vault,
            filename: "e.md".into(),
            operation: "find_and_replace".into(),
            content: "x".into(),
            folder: None,
            search: Some("missing".into()),
        }));
        assert_is_error(r);
    }

    // ── delete-note ───────────────────────────────────────────────────────────

    #[test]
    fn delete_note_ok() {
        let (dir, h, vault) = setup();
        write(&dir, "del.md", "");
        let r = h.delete_note(Parameters(DeleteNoteParams {
            vault,
            filename: "del".into(),
            folder: None,
        }));
        assert!(r.is_ok());
    }

    #[test]
    fn delete_note_missing_is_tool_error() {
        let (_dir, h, vault) = setup();
        let r = h.delete_note(Parameters(DeleteNoteParams {
            vault,
            filename: "ghost".into(),
            folder: None,
        }));
        assert_is_error(r);
    }

    // ── move-note ─────────────────────────────────────────────────────────────

    #[test]
    fn move_note_ok() {
        let (dir, h, vault) = setup();
        write(&dir, "src.md", "");
        let r = h.move_note(Parameters(MoveNoteParams {
            vault,
            filename: "src".into(),
            folder: None,
            new_folder: None,
            new_filename: Some("dst".into()),
        }));
        assert!(r.is_ok());
    }

    #[test]
    fn move_note_missing_is_tool_error() {
        let (_dir, h, vault) = setup();
        let r = h.move_note(Parameters(MoveNoteParams {
            vault,
            filename: "ghost".into(),
            folder: None,
            new_folder: None,
            new_filename: None,
        }));
        assert_is_error(r);
    }

    // ── create-directory ──────────────────────────────────────────────────────

    #[test]
    fn create_directory_ok() {
        let (dir, h, vault) = setup();
        let r = h.create_directory(Parameters(CreateDirectoryParams {
            vault,
            path: "newdir".into(),
            recursive: Some(true),
        }));
        assert!(r.is_ok());
        assert!(dir.path().join("newdir").is_dir());
    }

    #[test]
    fn create_directory_default_recursive() {
        let (dir, h, vault) = setup();
        let r = h.create_directory(Parameters(CreateDirectoryParams {
            vault,
            path: "a/b".into(),
            recursive: None,
        }));
        assert!(r.is_ok());
        assert!(dir.path().join("a/b").is_dir());
    }

    // ── search-vault ──────────────────────────────────────────────────────────

    #[test]
    fn search_vault_finds_result() {
        let (dir, h, vault) = setup();
        write(&dir, "s.md", "needle content");
        let r = h.search_vault(Parameters(SearchVaultParams {
            vault,
            query: "needle".into(),
            path: None,
            case_sensitive: None,
            search_type: None,
            limit: None,
            offset: None,
            max_matches_per_file: None,
        }));
        let out = r.unwrap().0;
        assert_eq!(out.results.len(), 1);
    }

    #[test]
    fn search_vault_no_results() {
        let (dir, h, vault) = setup();
        write(&dir, "s.md", "no match");
        let r = h.search_vault(Parameters(SearchVaultParams {
            vault,
            query: "zzz".into(),
            path: None,
            case_sensitive: None,
            search_type: None,
            limit: None,
            offset: None,
            max_matches_per_file: None,
        }));
        assert!(r.unwrap().0.results.is_empty());
    }

    #[test]
    fn search_vault_returns_structured_content() {
        let (dir, h, vault) = setup();
        write(&dir, "s.md", "needle content");
        let r = h.search_vault(Parameters(SearchVaultParams {
            vault,
            query: "needle".into(),
            path: None,
            case_sensitive: None,
            search_type: None,
            limit: None,
            offset: None,
            max_matches_per_file: None,
        }));
        let out = r.unwrap().0;
        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].path, "s.md");
        assert!(!out.results[0].snippets.is_empty());
    }

    #[test]
    fn search_vault_empty_still_has_structured_content() {
        let (dir, h, vault) = setup();
        write(&dir, "s.md", "no match here");
        let r = h.search_vault(Parameters(SearchVaultParams {
            vault,
            query: "zzz".into(),
            path: None,
            case_sensitive: None,
            search_type: None,
            limit: None,
            offset: None,
            max_matches_per_file: None,
        }));
        assert!(r.unwrap().0.results.is_empty());
    }

    #[test]
    fn search_vault_filename_type() {
        let (dir, h, vault) = setup();
        write(&dir, "matchme.md", "");
        let r = h.search_vault(Parameters(SearchVaultParams {
            vault,
            query: "matchme".into(),
            path: None,
            case_sensitive: None,
            search_type: Some(SearchType::Filename),
            limit: None,
            offset: None,
            max_matches_per_file: None,
        }));
        assert!(r.is_ok());
    }

    #[test]
    fn search_vault_both_type() {
        let (dir, h, vault) = setup();
        write(&dir, "note.md", "content");
        let r = h.search_vault(Parameters(SearchVaultParams {
            vault,
            query: "note".into(),
            path: None,
            case_sensitive: None,
            search_type: Some(SearchType::Both),
            limit: None,
            offset: None,
            max_matches_per_file: None,
        }));
        assert!(r.is_ok());
    }

    // ── add-tags ──────────────────────────────────────────────────────────────

    #[test]
    fn add_tags_ok() {
        let (dir, h, vault) = setup();
        write(&dir, "t.md", "content");
        let r = h.add_tags(Parameters(AddTagsParams {
            vault,
            files: vec!["t.md".into()],
            tags: vec!["mytag".into()],
            location: None,
            normalize: None,
            position: None,
        }));
        assert!(r.is_ok());
    }

    // ── remove-tags ───────────────────────────────────────────────────────────

    #[test]
    fn remove_tags_ok() {
        let (dir, h, vault) = setup();
        write(&dir, "t.md", "text #old");
        let r = h.remove_tags(Parameters(RemoveTagsParams {
            vault,
            files: vec!["t.md".into()],
            tags: vec!["old".into()],
        }));
        assert!(r.is_ok());
    }

    // ── rename-tag ────────────────────────────────────────────────────────────

    #[test]
    fn rename_tag_ok() {
        let (dir, h, vault) = setup();
        write(&dir, "t.md", "---\ntags:\n  - alpha\n---\n");
        let r = h.rename_tag(Parameters(RenameTagParams {
            vault,
            old_tag: "alpha".into(),
            new_tag: "beta".into(),
        }));
        assert!(r.is_ok());
    }

    // ── list-available-vaults ─────────────────────────────────────────────────

    #[test]
    fn list_available_vaults_returns_list() {
        let (_, h, _) = setup();
        let r = h.list_available_vaults(Parameters(ListVaultsParams {}));
        assert!(r.is_ok());
        let text = r.unwrap().content[0].as_text().unwrap().text.clone();
        assert!(text.contains("Available vaults") || text.contains("No vaults"));
    }

    #[test]
    fn list_available_vaults_empty_manager() {
        let manager = VaultManager::new(vec![]);
        let handler = ObsidianHandler::new(manager);
        let r = handler.list_available_vaults(Parameters(ListVaultsParams {}));
        assert!(r.is_ok());
        let text = r.unwrap().content[0].as_text().unwrap().text.clone();
        assert!(text.contains("No vaults"));
    }

    // ── --no-edit mode ────────────────────────────────────────────────────────

    fn setup_readonly() -> (TempDir, ObsidianHandler, String) {
        let dir = TempDir::new().unwrap();
        let vault_name = dir
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let manager = VaultManager::new(vec![dir.path().to_path_buf()]);
        let handler = ObsidianHandler::with_options(manager, true);
        (dir, handler, vault_name)
    }

    #[test]
    fn no_edit_blocks_create_note() {
        let (_, h, vault) = setup_readonly();
        let r = h.create_note(Parameters(CreateNoteParams {
            vault,
            filename: "x.md".into(),
            content: "".into(),
            folder: None,
        }));
        assert!(r.is_err());
        assert!(r.unwrap_err().message.contains("--no-edit"));
    }

    #[test]
    fn no_edit_blocks_edit_note() {
        let (dir, h, vault) = setup_readonly();
        write(&dir, "e.md", "a");
        let r = h.edit_note(Parameters(EditNoteParams {
            vault,
            filename: "e.md".into(),
            operation: "append".into(),
            content: "b".into(),
            folder: None,
            search: None,
        }));
        assert!(r.is_err());
        assert!(r.unwrap_err().message.contains("--no-edit"));
    }

    #[test]
    fn no_edit_blocks_delete_note() {
        let (dir, h, vault) = setup_readonly();
        write(&dir, "del.md", "");
        let r = h.delete_note(Parameters(DeleteNoteParams {
            vault,
            filename: "del.md".into(),
            folder: None,
        }));
        assert!(r.is_err());
        assert!(r.unwrap_err().message.contains("--no-edit"));
    }

    #[test]
    fn no_edit_blocks_move_note() {
        let (dir, h, vault) = setup_readonly();
        write(&dir, "src.md", "");
        let r = h.move_note(Parameters(MoveNoteParams {
            vault,
            filename: "src.md".into(),
            folder: None,
            new_folder: None,
            new_filename: Some("dst.md".into()),
        }));
        assert!(r.is_err());
        assert!(r.unwrap_err().message.contains("--no-edit"));
    }

    #[test]
    fn no_edit_blocks_create_directory() {
        let (_, h, vault) = setup_readonly();
        let r = h.create_directory(Parameters(CreateDirectoryParams {
            vault,
            path: "newdir".into(),
            recursive: None,
        }));
        assert!(r.is_err());
        assert!(r.unwrap_err().message.contains("--no-edit"));
    }

    #[test]
    fn no_edit_blocks_add_tags() {
        let (dir, h, vault) = setup_readonly();
        write(&dir, "t.md", "content");
        let r = h.add_tags(Parameters(AddTagsParams {
            vault,
            files: vec!["t.md".into()],
            tags: vec!["tag".into()],
            location: None,
            normalize: None,
            position: None,
        }));
        assert!(r.is_err());
        assert!(r.unwrap_err().message.contains("--no-edit"));
    }

    #[test]
    fn no_edit_blocks_remove_tags() {
        let (dir, h, vault) = setup_readonly();
        write(&dir, "t.md", "#old");
        let r = h.remove_tags(Parameters(RemoveTagsParams {
            vault,
            files: vec!["t.md".into()],
            tags: vec!["old".into()],
        }));
        assert!(r.is_err());
        assert!(r.unwrap_err().message.contains("--no-edit"));
    }

    #[test]
    fn no_edit_blocks_rename_tag() {
        let (dir, h, vault) = setup_readonly();
        write(&dir, "t.md", "---\ntags:\n  - alpha\n---\n");
        let r = h.rename_tag(Parameters(RenameTagParams {
            vault,
            old_tag: "alpha".into(),
            new_tag: "beta".into(),
        }));
        assert!(r.is_err());
        assert!(r.unwrap_err().message.contains("--no-edit"));
    }

    #[test]
    fn no_edit_allows_read_note() {
        let (dir, h, vault) = setup_readonly();
        write(&dir, "n.md", "body");
        let r = h.read_note(Parameters(ReadNoteParams {
            vault,
            filename: "n.md".into(),
            folder: None,
        }));
        assert!(r.is_ok());
    }

    #[test]
    fn no_edit_allows_search_vault() {
        let (dir, h, vault) = setup_readonly();
        write(&dir, "s.md", "needle");
        let r = h.search_vault(Parameters(SearchVaultParams {
            vault,
            query: "needle".into(),
            path: None,
            case_sensitive: None,
            search_type: None,
            limit: None,
            offset: None,
            max_matches_per_file: None,
        }));
        assert!(r.is_ok());
    }

    #[test]
    fn no_edit_allows_list_available_vaults() {
        let (_, h, _) = setup_readonly();
        let r = h.list_available_vaults(Parameters(ListVaultsParams {}));
        assert!(r.is_ok());
    }
}
