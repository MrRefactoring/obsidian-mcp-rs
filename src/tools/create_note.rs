use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateNoteParams {
    /// Name of the vault to create the note in
    pub vault: String,
    /// Note to act on: a vault-relative path (`projects/apollo.md`) or a bare
    /// filename. `.md` optional. `search-vault`'s `path` works as-is.
    pub filename: String,
    /// Content of the note in Markdown format
    pub content: String,
    /// Optional subfolder path relative to vault root. Parent directories are created automatically.
    pub folder: Option<String>,
}
