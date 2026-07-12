use serde::Deserialize;

use crate::vault::{Period, PeriodicAction};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PeriodicParams {
    /// Name of the vault
    pub vault: String,
    /// Which periodic note: "daily", "weekly", "monthly", "quarterly", "yearly"
    pub period: Period,
    /// What to do: "get" reads it (and fails if it doesn't exist), "create"
    /// reads it and creates it first if needed, "list" lists the ones that exist
    pub action: PeriodicAction,
    /// The date the note is for, as YYYY-MM-DD. Defaults to today. Ignored by "list".
    pub date: Option<String>,
    /// Text for a note that "create" brings into existence. Without it, the note
    /// is seeded from whatever template Obsidian is configured to use.
    pub content: Option<String>,
    /// How many notes "list" returns (default: 10)
    pub limit: Option<usize>,
}
