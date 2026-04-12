use std::sync::Arc;

use similar::TextDiff;
use rmcp::{
    ErrorData as McpError,
    ServerHandler,
    handler::server::router::tool::ToolRouter,
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};

use crate::{
    tools::{
        add_tags::AddTagsParams,
        create_directory::CreateDirectoryParams,
        create_note::CreateNoteParams,
        delete_note::DeleteNoteParams,
        edit_note::EditNoteParams,
        list_vaults::ListVaultsParams,
        move_note::MoveNoteParams,
        read_note::ReadNoteParams,
        remove_tags::RemoveTagsParams,
        rename_tag::RenameTagParams,
        search_vault::SearchVaultParams,
    },
    vault::{SearchType, VaultManager},
};

#[derive(Clone)]
pub struct ObsidianHandler {
    vault: Arc<VaultManager>,
    tool_router: ToolRouter<Self>,
}

fn ok(text: impl Into<String>) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![Content::text(text.into())]))
}

fn err(e: impl std::fmt::Display) -> McpError {
    McpError::internal_error(e.to_string(), None)
}

#[tool_router]
impl ObsidianHandler {
    pub fn new(vault: VaultManager) -> Self {
        Self {
            vault: Arc::new(vault),
            tool_router: Self::tool_router(),
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
        let (old, new) = self
            .vault
            .edit_note(&vault, &filename, &operation, &content, folder.as_deref(), search.as_deref())
            .map_err(err)?;
        let diff = TextDiff::from_lines(&old, &new);
        let unified = diff
            .unified_diff()
            .context_radius(3)
            .header(&filename, &filename)
            .to_string();
        ok(format!("Note '{}' updated with operation '{}'\n\n```diff\n{}```", filename, operation, unified))
    }

    /// Delete a note from the vault.
    #[tool(name = "delete-note")]
    fn delete_note(
        &self,
        rmcp::handler::server::wrapper::Parameters(DeleteNoteParams {
            vault,
            filename,
            folder,
        }): rmcp::handler::server::wrapper::Parameters<DeleteNoteParams>,
    ) -> Result<CallToolResult, McpError> {
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
        let modified = self
            .vault
            .remove_tags(&vault, &files, &tags)
            .map_err(err)?;
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
        ServerInfo::new(
            ServerCapabilities::builder().enable_tools().build(),
        )
    }
}
