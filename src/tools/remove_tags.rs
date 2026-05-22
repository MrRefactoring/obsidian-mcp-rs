use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RemoveTagsParams {
    /// Name of the vault containing the notes
    pub vault: String,
    /// Array of note filenames to remove tags from (must include .md extension)
    pub files: Vec<String>,
    /// Array of tags to remove (e.g. "status/active", "project/docs")
    pub tags: Vec<String>,
}
