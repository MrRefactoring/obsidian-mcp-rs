use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateNoteParams {
    /// Name of the vault.
    pub vault: String,
    /// Note path relative to the vault root. You can omit `.md`. Use the `path` from `search_notes`.
    pub filename: String,
    /// Content of the note in Markdown format
    pub content: String,
    /// Folder path relative to the vault root. Parent directories are created automatically.
    pub folder: Option<String>,
}
