use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateNoteParams {
    /// Name of the vault.
    pub vault: String,
    /// Note path relative to the vault root. You can omit `.md`. Use the `path` from `search_notes`.
    pub filename: String,
    /// Markdown for the note. For Obsidian syntax, use `get_writing_guide`.
    pub content: String,
    /// Folder path relative to the vault root. Parent directories are created automatically.
    pub folder: Option<String>,
}
