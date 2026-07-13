use serde::Deserialize;

use crate::vault::{LinkLimits, LinkQuery};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WikilinksParams {
    /// Name of the vault to inspect
    pub vault: String,
    /// Which slice of the link graph to return. "backlinks" and "outgoing" are
    /// about one note and need `filename`; "broken" and "orphans" cover the
    /// whole vault.
    pub query: LinkQuery,
    /// Note to query — required for "backlinks" and "outgoing" (.md optional)
    pub filename: Option<String>,
    /// Optional subfolder containing the note
    pub folder: Option<String>,
    /// Most links (or orphan notes) to return (default: 50). "broken" and
    /// "orphans" on a neglected vault run to thousands.
    pub limit: Option<usize>,
    /// Skip this many — use with `limit` to page through them (default: 0)
    pub offset: Option<usize>,
}

impl WikilinksParams {
    pub fn limits(&self) -> LinkLimits {
        let d = LinkLimits::default();
        LinkLimits {
            limit: self.limit.unwrap_or(d.limit).max(1),
            offset: self.offset.unwrap_or(d.offset),
        }
    }
}
