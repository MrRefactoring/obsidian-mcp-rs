use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DeleteNoteParams {
    /// Name of the vault containing the note
    pub vault: String,
    /// Note to act on: a vault-relative path (`projects/apollo.md`) or a bare
    /// filename. `.md` optional. `search-vault`'s `path` works as-is.
    pub filename: String,
    /// Optional subfolder path relative to vault root
    pub folder: Option<String>,
    /// Erase the note instead of moving it to the vault's `.trash/`, where the
    /// user can still recover it. There is no undo. (default: false)
    pub permanent: Option<bool>,
}
