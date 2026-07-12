use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DeleteNoteParams {
    /// Name of the vault containing the note
    pub vault: String,
    /// Note filename (with or without .md extension). Do not include path separators.
    pub filename: String,
    /// Optional subfolder path relative to vault root
    pub folder: Option<String>,
    /// Erase the note instead of moving it to the vault's `.trash/`, where the
    /// user can still recover it. There is no undo. (default: false)
    pub permanent: Option<bool>,
}
