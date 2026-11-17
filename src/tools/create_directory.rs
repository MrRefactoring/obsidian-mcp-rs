use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateDirectoryParams {
    /// Name of the vault.
    pub vault: String,
    /// Folder path relative to the vault root, for example "journal/2024".
    pub path: String,
    /// Create missing parent folders. Default: true.
    pub recursive: Option<bool>,
}
