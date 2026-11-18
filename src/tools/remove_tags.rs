use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RemoveTagsParams {
    /// Name of the vault.
    pub vault: String,
    /// Note paths relative to the vault root. Include the `.md` extension.
    pub files: Vec<String>,
    /// Tags to remove, for example "status/active".
    pub tags: Vec<String>,
}
