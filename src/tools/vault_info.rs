use serde::Deserialize;

use crate::vault::InfoQuery;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct VaultInfoParams {
    /// Name of the vault to describe
    pub vault: String,
    /// What to ask: "tags" (every tag, with how many notes carry it),
    /// "recent" (notes by last modified, newest first), or "stats" (notes,
    /// folders, size, tags, links, broken links).
    pub query: InfoQuery,
    /// How many notes "recent" returns (default: 20). Ignored by the other queries.
    pub limit: Option<usize>,
}
