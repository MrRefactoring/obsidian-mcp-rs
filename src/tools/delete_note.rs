use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DeleteNoteParams {
    /// Name of the vault.
    pub vault: String,
    /// Note path relative to the vault root. You can omit `.md`. Use the `path` from `search_notes`.
    pub filename: String,
    /// Folder path relative to the vault root
    pub folder: Option<String>,
    /// Erase the note instead of moving it to the vault's `.trash/`, where the
    /// user can still recover it. There is no undo. (default: false)
    pub permanent: Option<bool>,
}
