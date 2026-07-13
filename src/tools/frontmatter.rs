use serde::Deserialize;

use crate::vault::FrontmatterAction;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FrontmatterParams {
    /// Name of the vault containing the note
    pub vault: String,
    /// Note to act on: a vault-relative path (`projects/apollo.md`) or a bare
    /// filename. `.md` optional. `search-vault`'s `path` works as-is.
    pub filename: String,
    /// What to do: "get" reads, "set" writes `key` = `value`, "remove" deletes `key`
    pub action: FrontmatterAction,
    /// The frontmatter key. Required for "set" and "remove". For "get", omit it
    /// to read the whole frontmatter.
    pub key: Option<String>,
    /// The value to write, as JSON — a string, number, boolean, list or object.
    /// Required for "set". Lists are written as YAML block lists.
    pub value: Option<serde_json::Value>,
    /// Optional subfolder path relative to vault root
    pub folder: Option<String>,
}
