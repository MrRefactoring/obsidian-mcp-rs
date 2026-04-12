use rmcp::handler::server::wrapper::Parameters;
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RenameTagParams {
    /// Name of the vault to rename the tag in
    pub vault: String,
    /// The tag to rename (e.g. "old-tag")
    #[serde(rename = "oldTag")]
    pub old_tag: String,
    /// The new tag name (e.g. "new-tag")
    #[serde(rename = "newTag")]
    pub new_tag: String,
}

pub type RenameTag = Parameters<RenameTagParams>;
