use rmcp::handler::server::wrapper::Parameters;
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AddTagsParams {
    /// Name of the vault containing the notes
    pub vault: String,
    /// Array of note filenames to add tags to (must include .md extension, may include relative path)
    pub files: Vec<String>,
    /// Array of tags to add (e.g. "status/active", "project/docs")
    pub tags: Vec<String>,
    /// Where to add tags: "frontmatter", "content", or "both" (default: "both")
    pub location: Option<String>,
    /// Normalize tag format (e.g. ProjectActive -> project-active). Default: true
    pub normalize: Option<bool>,
    /// Where to add inline tags in content: "start" or "end" (default: "end")
    pub position: Option<String>,
}

pub type AddTags = Parameters<AddTagsParams>;
