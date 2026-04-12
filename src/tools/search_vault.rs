use rmcp::handler::server::wrapper::Parameters;
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchVaultParams {
    /// Name of the vault to search in
    pub vault: String,
    /// Search query. For text search use the term directly; for tag search use "tag:" prefix (e.g. "tag:status/active")
    pub query: String,
    /// Optional subfolder path within the vault to limit the search scope
    pub path: Option<String>,
    /// Whether to perform case-sensitive search (default: false)
    #[serde(rename = "caseSensitive")]
    pub case_sensitive: Option<bool>,
    /// Type of search: "content" (default), "filename", or "both"
    #[serde(rename = "searchType")]
    pub search_type: Option<String>,
}

pub type SearchVault = Parameters<SearchVaultParams>;
