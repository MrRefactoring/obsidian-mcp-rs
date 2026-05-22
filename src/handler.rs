use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::router::tool::ToolRouter,
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};
use similar::TextDiff;

use crate::{
    tools::{
        add_tags::AddTagsParams, create_directory::CreateDirectoryParams,
        create_note::CreateNoteParams, delete_note::DeleteNoteParams, edit_note::EditNoteParams,
        list_vaults::ListVaultsParams, move_note::MoveNoteParams, read_note::ReadNoteParams,
        remove_tags::RemoveTagsParams, rename_tag::RenameTagParams,
        search_vault::SearchVaultParams,
    },
    vault::{SearchType, VaultManager},
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
    Ok(CallToolResult::success(vec![Content::text(text.into())]))
}

fn err(e: impl std::fmt::Display) -> McpError {
    let msg = e.to_string();
    tracing::error!("{}", msg);
    McpError::internal_error(msg, None)
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
    #[tool(name = "read-note")]
    fn read_note(
        &self,
        rmcp::handler::server::wrapper::Parameters(ReadNoteParams {
            vault,
            filename,
            folder,
        }): rmcp::handler::server::wrapper::Parameters<ReadNoteParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(tool = "read-note", %vault, %filename);
        let content = self
            .vault
            .read_note(&vault, &filename, folder.as_deref())
            .map_err(err)?;
        ok(content)
    }

    /// Create a new note in the specified vault with Markdown content.
    #[tool(name = "create-note")]
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
        let path = self
            .vault
            .create_note(&vault, &filename, &content, folder.as_deref())
            .map_err(err)?;
        ok(format!("Created note at {}", path.display()))
    }

    /// Edit an existing note. Operations: append, prepend, replace, find_and_replace.
    #[tool(name = "edit-note")]
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
        let (old, new) = self
            .vault
            .edit_note(
                &vault,
                &filename,
                &operation,
                &content,
                folder.as_deref(),
                search.as_deref(),
            )
            .map_err(err)?;
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
    #[tool(name = "delete-note")]
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
        self.vault
            .delete_note(&vault, &filename, folder.as_deref())
            .map_err(err)?;
        ok(format!("Deleted note '{}'", filename))
    }

    /// Move or rename a note within the vault.
    #[tool(name = "move-note")]
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
        let dest = self
            .vault
            .move_note(
                &vault,
                &filename,
                folder.as_deref(),
                new_folder.as_deref(),
                new_filename.as_deref(),
            )
            .map_err(err)?;
        ok(format!("Moved note to {}", dest.display()))
    }

    /// Create a new directory in the vault.
    #[tool(name = "create-directory")]
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
        let dir = self
            .vault
            .create_directory(&vault, &path, recursive.unwrap_or(true))
            .map_err(err)?;
        ok(format!("Created directory {}", dir.display()))
    }

    /// Search for specific content within vault notes. Supports content, filename, and tag search.
    #[tool(name = "search-vault")]
    fn search_vault(
        &self,
        rmcp::handler::server::wrapper::Parameters(SearchVaultParams {
            vault,
            query,
            path,
            case_sensitive,
            search_type,
        }): rmcp::handler::server::wrapper::Parameters<SearchVaultParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(tool = "search-vault", %vault, %query);
        let st = match search_type.as_deref() {
            Some("filename") => SearchType::Filename,
            Some("both") => SearchType::Both,
            _ => SearchType::Content,
        };

        let results = self
            .vault
            .search_vault(
                &vault,
                &query,
                path.as_deref(),
                case_sensitive.unwrap_or(false),
                &st,
            )
            .map_err(err)?;

        if results.is_empty() {
            return ok("No results found.");
        }

        let output = results
            .iter()
            .map(|r| {
                let matches = r.matches.join("\n    ");
                format!("## {}\nPath: {}\n    {}", r.filename, r.path, matches)
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        ok(format!("Found {} result(s):\n\n{}", results.len(), output))
    }

    /// Add tags to notes in frontmatter and/or content.
    #[tool(name = "add-tags")]
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
        let modified = self
            .vault
            .add_tags(
                &vault,
                &files,
                &tags,
                location.as_deref().unwrap_or("both"),
                normalize.unwrap_or(true),
                position.as_deref().unwrap_or("end"),
            )
            .map_err(err)?;
        ok(format!(
            "Added tags {:?} to {} file(s): {}",
            tags,
            modified.len(),
            modified.join(", ")
        ))
    }

    /// Remove tags from notes in frontmatter and content.
    #[tool(name = "remove-tags")]
    fn remove_tags(
        &self,
        rmcp::handler::server::wrapper::Parameters(RemoveTagsParams { vault, files, tags }): rmcp::handler::server::wrapper::Parameters<RemoveTagsParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(tool = "remove-tags", %vault, ?tags);
        self.check_write()?;
        let modified = self.vault.remove_tags(&vault, &files, &tags).map_err(err)?;
        ok(format!(
            "Removed tags {:?} from {} file(s): {}",
            tags,
            modified.len(),
            modified.join(", ")
        ))
    }

    /// Rename a tag across all notes in the vault.
    #[tool(name = "rename-tag")]
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
        let modified = self
            .vault
            .rename_tag(&vault, &old_tag, &new_tag)
            .map_err(err)?;
        ok(format!(
            "Renamed tag '{}' to '{}' in {} file(s): {}",
            old_tag,
            new_tag,
            modified.len(),
            modified.join(", ")
        ))
    }

    /// List all available vaults configured for this server.
    #[tool(name = "list-available-vaults")]
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
    fn read_note_not_found_is_err() {
        let (_, h, vault) = setup();
        let r = h.read_note(Parameters(ReadNoteParams {
            vault,
            filename: "ghost".into(),
            folder: None,
        }));
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
    fn create_note_duplicate_is_err() {
        let (dir, h, vault) = setup();
        write(&dir, "dup.md", "");
        let r = h.create_note(Parameters(CreateNoteParams {
            vault,
            filename: "dup".into(),
            content: "".into(),
            folder: None,
        }));
        assert!(r.is_err());
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
    fn edit_note_error_on_missing() {
        let (_, h, vault) = setup();
        let r = h.edit_note(Parameters(EditNoteParams {
            vault,
            filename: "ghost".into(),
            operation: "append".into(),
            content: "x".into(),
            folder: None,
            search: None,
        }));
        assert!(r.is_err());
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
    fn delete_note_missing_is_err() {
        let (_, h, vault) = setup();
        let r = h.delete_note(Parameters(DeleteNoteParams {
            vault,
            filename: "ghost".into(),
            folder: None,
        }));
        assert!(r.is_err());
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
    fn move_note_missing_is_err() {
        let (_, h, vault) = setup();
        let r = h.move_note(Parameters(MoveNoteParams {
            vault,
            filename: "ghost".into(),
            folder: None,
            new_folder: None,
            new_filename: None,
        }));
        assert!(r.is_err());
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
        }));
        assert!(r.is_ok());
        let text = r.unwrap().content[0].as_text().unwrap().text.clone();
        assert!(text.contains("result"));
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
        }));
        assert!(r.is_ok());
        let text = r.unwrap().content[0].as_text().unwrap().text.clone();
        assert!(text.contains("No results"));
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
            search_type: Some("filename".into()),
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
            search_type: Some("both".into()),
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
