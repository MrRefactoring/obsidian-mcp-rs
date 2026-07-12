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
        frontmatter::FrontmatterParams, list_vaults::ListVaultsParams, move_note::MoveNoteParams,
        read_note::ReadNoteParams, remove_tags::RemoveTagsParams, rename_tag::RenameTagParams,
        search_vault::SearchVaultParams, vault_info::VaultInfoParams, wikilinks::WikilinksParams,
    },
    vault::{
        DEFAULT_RECENT, DeleteOutcome, Edit, FrontmatterAction, FrontmatterOutput, InfoOutput,
        LinkOutput, SearchOutput, Target, VaultManager,
    },
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

/// Tools that only ever write. Under `--no-edit` these are removed from the
/// router, so they are absent from `tools/list` *and* unreachable via
/// `tools/call` — `check_write` then stays as the second layer, and is what gates
/// `frontmatter`, the one tool that both reads and writes.
const WRITE_TOOLS: [&str; 8] = [
    "create-note",
    "edit-note",
    "delete-note",
    "move-note",
    "create-directory",
    "add-tags",
    "remove-tags",
    "rename-tag",
];

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
        let mut tool_router = Self::tool_router();
        if no_edit {
            // Don't advertise what we will only refuse. A tool the model can see
            // is a tool it will try, and a rejection it has to spend a turn
            // recovering from; removing the routes means `tools/list` describes a
            // read-only server honestly. `frontmatter` stays, because `get` is a
            // read — `check_write` gates it per action.
            for name in WRITE_TOOLS {
                tool_router.remove_route(name);
            }
        }
        Self {
            vault: Arc::new(vault),
            tool_router,
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

    /// Read a note. `view: "outline"` returns just its headings, block
    /// references and frontmatter keys — what `edit-note` can be aimed at —
    /// instead of the whole text.
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
            view,
        }): rmcp::handler::server::wrapper::Parameters<ReadNoteParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(tool = "read-note", %vault, %filename, ?view);
        match self.vault.read_note(
            &vault,
            &filename,
            folder.as_deref(),
            &view.unwrap_or_default(),
        ) {
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

    /// Edit a note: append, prepend, replace, or find_and_replace. Set
    /// `targetType`/`target` to edit one heading's section or one `^block-id`
    /// instead of the whole note — the rest of the file is then left untouched.
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
            target_type,
            target,
        }): rmcp::handler::server::wrapper::Parameters<EditNoteParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(tool = "edit-note", %vault, %filename, ?operation, ?target);
        self.check_write()?;

        // Half a target is a mistake we must not guess our way through: editing
        // the whole note when the model meant one section would clobber it.
        let target = match (&target_type, &target) {
            (Some(kind), Some(name)) => Some(Target { kind, name }),
            (None, None) => None,
            _ => {
                return Err(McpError::invalid_params(
                    "'targetType' and 'target' must be given together",
                    None,
                ));
            }
        };

        let edit = Edit {
            operation: operation.clone(),
            content: &content,
            search: search.as_deref(),
            target,
        };
        let (old, new) = match self
            .vault
            .edit_note(&vault, &filename, folder.as_deref(), &edit)
        {
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
            "Note '{}' updated with operation '{:?}'\n\n```diff\n{}```",
            filename, operation, unified
        ))
    }

    /// Read or write a note's YAML frontmatter. "get" returns the whole
    /// frontmatter (or one `key`); "set" writes `key` = `value`; "remove"
    /// deletes `key`. Writes touch only that key — every other line, comment and
    /// key order in the note is preserved.
    #[tool(
        name = "frontmatter",
        annotations(
            title = "Read or write frontmatter",
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn frontmatter(
        &self,
        rmcp::handler::server::wrapper::Parameters(FrontmatterParams {
            vault,
            filename,
            action,
            key,
            value,
            folder,
        }): rmcp::handler::server::wrapper::Parameters<FrontmatterParams>,
    ) -> Result<Json<FrontmatterOutput>, McpError> {
        tracing::debug!(tool = "frontmatter", %vault, %filename, ?action, ?key);
        if action != FrontmatterAction::Get {
            self.check_write()?;
        }
        let out = self
            .vault
            .frontmatter(
                &vault,
                &filename,
                folder.as_deref(),
                &action,
                key.as_deref(),
                value.as_ref(),
            )
            .map_err(err)?;
        Ok(Json(out))
    }

    /// Delete a note. By default it moves to the vault's `.trash/`, where the
    /// user can recover it; pass `permanent: true` to erase it instead. If this
    /// empties its containing folder, that folder is removed too (the vault root
    /// is never deleted).
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
            permanent,
        }): rmcp::handler::server::wrapper::Parameters<DeleteNoteParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(tool = "delete-note", %vault, %filename, ?permanent);
        self.check_write()?;
        match self.vault.delete_note(
            &vault,
            &filename,
            folder.as_deref(),
            permanent.unwrap_or(false),
        ) {
            Ok(DeleteOutcome {
                trashed_to: Some(dest),
            }) => ok(format!(
                "Moved note '{}' to '{}' — it can still be recovered from the vault's trash.",
                filename, dest
            )),
            Ok(DeleteOutcome { trashed_to: None }) => {
                ok(format!("Permanently deleted note '{}'", filename))
            }
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

    /// Describe a vault before searching it: `query: "tags"` lists every tag
    /// with how many notes carry it, `"recent"` lists the notes touched most
    /// recently, and `"stats"` gives its size and shape (notes, folders, links,
    /// broken links).
    #[tool(
        name = "vault-info",
        annotations(
            title = "Describe vault",
            read_only_hint = true,
            open_world_hint = false
        )
    )]
    fn vault_info(
        &self,
        rmcp::handler::server::wrapper::Parameters(VaultInfoParams {
            vault,
            query,
            limit,
        }): rmcp::handler::server::wrapper::Parameters<VaultInfoParams>,
    ) -> Result<Json<InfoOutput>, McpError> {
        tracing::debug!(tool = "vault-info", %vault, ?query);
        let out = self
            .vault
            .vault_info(&vault, &query, limit.unwrap_or(DEFAULT_RECENT))
            .map_err(err)?;
        Ok(Json(out))
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
        frontmatter::FrontmatterParams, list_vaults::ListVaultsParams, move_note::MoveNoteParams,
        read_note::ReadNoteParams, remove_tags::RemoveTagsParams, rename_tag::RenameTagParams,
        search_vault::SearchVaultParams,
    };
    use crate::vault::{EditOperation, NoteView, TargetKind};

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
            view: None,
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
            view: None,
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
            view: None,
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
            operation: EditOperation::Append,
            content: "b".into(),
            folder: None,
            search: None,
            target_type: None,
            target: None,
        }));
        assert!(r.is_ok());
    }

    #[test]
    fn edit_note_missing_is_tool_error() {
        let (_dir, h, vault) = setup();
        let r = h.edit_note(Parameters(EditNoteParams {
            vault,
            filename: "ghost".into(),
            operation: EditOperation::Append,
            content: "x".into(),
            folder: None,
            search: None,
            target_type: None,
            target: None,
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
            operation: EditOperation::FindAndReplace,
            content: "x".into(),
            folder: None,
            search: Some("missing".into()),
            target_type: None,
            target: None,
        }));
        assert_is_error(r);
    }

    // ── edit-note: patch targets ──────────────────────────────────────────────

    const SECTIONED: &str = "# Top\n\nintro\n\n## Log\n\nfirst\n";

    #[test]
    fn edit_note_can_target_one_section() {
        let (dir, h, vault) = setup();
        write(&dir, "e.md", SECTIONED);
        let r = h.edit_note(Parameters(EditNoteParams {
            vault,
            filename: "e.md".into(),
            operation: EditOperation::Append,
            content: "second".into(),
            folder: None,
            search: None,
            target_type: Some(TargetKind::Heading),
            target: Some("## Log".into()),
        }));
        assert!(r.is_ok());
        let out = fs::read_to_string(dir.path().join("e.md")).unwrap();
        assert_eq!(out, "# Top\n\nintro\n\n## Log\n\nfirst\nsecond\n");
    }

    #[test]
    fn edit_note_missing_target_is_tool_error() {
        let (dir, h, vault) = setup();
        write(&dir, "e.md", SECTIONED);
        let r = h.edit_note(Parameters(EditNoteParams {
            vault,
            filename: "e.md".into(),
            operation: EditOperation::Replace,
            content: "x".into(),
            folder: None,
            search: None,
            target_type: Some(TargetKind::Heading),
            target: Some("Ghost".into()),
        }));
        assert_is_error(r);
        assert_eq!(
            fs::read_to_string(dir.path().join("e.md")).unwrap(),
            SECTIONED,
            "a missed target must not have rewritten the note"
        );
    }

    #[test]
    fn edit_note_rejects_half_a_target() {
        // Silently falling back to a whole-note edit here would clobber the note.
        let (dir, h, vault) = setup();
        write(&dir, "e.md", SECTIONED);
        let r = h.edit_note(Parameters(EditNoteParams {
            vault,
            filename: "e.md".into(),
            operation: EditOperation::Replace,
            content: "x".into(),
            folder: None,
            search: None,
            target_type: Some(TargetKind::Heading),
            target: None,
        }));
        assert!(r.is_err());
        assert_eq!(
            fs::read_to_string(dir.path().join("e.md")).unwrap(),
            SECTIONED
        );
    }

    // ── read-note: outline ────────────────────────────────────────────────────

    #[test]
    fn read_note_outline_returns_targets_not_prose() {
        let (dir, h, vault) = setup();
        write(&dir, "e.md", SECTIONED);
        let r = h.read_note(Parameters(ReadNoteParams {
            vault,
            filename: "e.md".into(),
            folder: None,
            view: Some(NoteView::Outline),
        }));
        let text = r.unwrap().content[0].as_text().unwrap().text.clone();
        assert!(text.contains("## Log"), "{text}");
        assert!(!text.contains("intro"), "{text}");
    }

    // ── frontmatter ───────────────────────────────────────────────────────────

    fn fm(vault: String, filename: &str, action: FrontmatterAction) -> FrontmatterParams {
        FrontmatterParams {
            vault,
            filename: filename.into(),
            action,
            key: None,
            value: None,
            folder: None,
        }
    }

    #[test]
    fn frontmatter_get_returns_structured_content() {
        let (dir, h, vault) = setup();
        write(&dir, "n.md", "---\ntitle: T\n---\nbody\n");
        let r = h.frontmatter(Parameters(fm(vault, "n", FrontmatterAction::Get)));
        let out = r.unwrap().0;
        assert_eq!(out.frontmatter["title"], serde_json::json!("T"));
        assert!(!out.changed);
    }

    #[test]
    fn frontmatter_set_writes_the_key() {
        let (dir, h, vault) = setup();
        write(&dir, "n.md", "---\ntitle: T\n---\nbody\n");
        let r = h.frontmatter(Parameters(FrontmatterParams {
            key: Some("status".into()),
            value: Some(serde_json::json!("draft")),
            ..fm(vault, "n", FrontmatterAction::Set)
        }));
        assert!(r.unwrap().0.changed);
        assert_eq!(
            fs::read_to_string(dir.path().join("n.md")).unwrap(),
            "---\ntitle: T\nstatus: draft\n---\nbody\n"
        );
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
            permanent: None,
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
            permanent: None,
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
            operation: EditOperation::Append,
            content: "b".into(),
            folder: None,
            search: None,
            target_type: None,
            target: None,
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
            permanent: None,
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
    fn no_edit_hides_write_tools_from_the_tool_list() {
        // A tool the model can see is a tool it will try. In a read-only server
        // `tools/list` must describe a read-only server.
        let (_dir, h, _) = setup_readonly();
        let listed: Vec<String> = h
            .tool_router
            .list_all()
            .iter()
            .map(|t| t.name.to_string())
            .collect();

        for write_tool in WRITE_TOOLS {
            assert!(
                !listed.iter().any(|n| n == write_tool),
                "--no-edit must not advertise '{write_tool}': {listed:?}"
            );
        }
        for read_tool in [
            "read-note",
            "search-vault",
            "wikilinks",
            "list-available-vaults",
        ] {
            assert!(listed.iter().any(|n| n == read_tool), "missing {read_tool}");
        }
        assert!(
            listed.iter().any(|n| n == "frontmatter"),
            "frontmatter reads as well as writes, so it stays — gated per action"
        );
    }

    #[test]
    fn every_tool_is_listed_when_writes_are_allowed() {
        let (_dir, h, _) = setup();
        assert_eq!(h.tool_router.list_all().len(), 14);
    }

    #[test]
    fn no_edit_blocks_frontmatter_set() {
        let (dir, h, vault) = setup_readonly();
        write(&dir, "n.md", "---\ntitle: T\n---\n");
        let r = h.frontmatter(Parameters(FrontmatterParams {
            key: Some("title".into()),
            value: Some(serde_json::json!("hacked")),
            ..fm(vault, "n", FrontmatterAction::Set)
        }));
        let e = r.err().expect("a write in --no-edit mode must be refused");
        assert!(e.message.contains("--no-edit"));
        assert_eq!(
            fs::read_to_string(dir.path().join("n.md")).unwrap(),
            "---\ntitle: T\n---\n"
        );
    }

    #[test]
    fn no_edit_blocks_frontmatter_remove() {
        let (dir, h, vault) = setup_readonly();
        write(&dir, "n.md", "---\ntitle: T\n---\n");
        let r = h.frontmatter(Parameters(FrontmatterParams {
            key: Some("title".into()),
            ..fm(vault, "n", FrontmatterAction::Remove)
        }));
        let e = r.err().expect("a write in --no-edit mode must be refused");
        assert!(e.message.contains("--no-edit"));
    }

    #[test]
    fn no_edit_allows_frontmatter_get() {
        // `frontmatter` is the one tool that both reads and writes, so the gate
        // is per-action: reading must still work in a read-only server.
        let (dir, h, vault) = setup_readonly();
        write(&dir, "n.md", "---\ntitle: T\n---\n");
        let r = h.frontmatter(Parameters(fm(vault, "n", FrontmatterAction::Get)));
        assert_eq!(r.unwrap().0.frontmatter["title"], serde_json::json!("T"));
    }

    #[test]
    fn no_edit_allows_read_note() {
        let (dir, h, vault) = setup_readonly();
        write(&dir, "n.md", "body");
        let r = h.read_note(Parameters(ReadNoteParams {
            vault,
            filename: "n.md".into(),
            folder: None,
            view: None,
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
