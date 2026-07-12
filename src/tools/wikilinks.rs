use serde::Deserialize;

use crate::vault::LinkQuery;

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
}
