use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RenameTagParams {
    /// Name of the vault.
    pub vault: String,
    /// Tag to rename, for example "old-tag".
    #[serde(rename = "oldTag")]
    pub old_tag: String,
    /// New tag name, for example "new-tag".
    #[serde(rename = "newTag")]
    pub new_tag: String,
    #[doc = "Set to true to list the notes that would change without writing to them."]
    #[serde(rename = "dryRun", default)]
    pub dry_run: bool,
}
