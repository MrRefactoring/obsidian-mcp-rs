use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReadNoteParams {
    /// Name of the vault containing the note
    pub vault: String,
    /// Note filename (with or without .md extension). Do not include path separators.
    pub filename: String,
    /// Optional subfolder path relative to vault root (e.g. "journal/2024")
    pub folder: Option<String>,
}
