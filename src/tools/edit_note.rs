use rmcp::handler::server::wrapper::Parameters;
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EditNoteParams {
    /// Name of the vault containing the note
    pub vault: String,
    /// Note filename (with or without .md extension). Do not include path separators.
    pub filename: String,
    /// Edit operation: "append" adds to end, "prepend" adds to start, "replace" overwrites entirely, "find_and_replace" replaces first occurrence of `search` with `content`
    pub operation: String,
    /// Content to apply according to the chosen operation
    pub content: String,
    /// Optional subfolder path relative to vault root
    pub folder: Option<String>,
    /// Text to find when using "find_and_replace" operation
    pub search: Option<String>,
}

pub type EditNote = Parameters<EditNoteParams>;
