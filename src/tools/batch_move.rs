use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchNoteMove {
    #[doc = "Source vault name."]
    pub source_vault: String,
    #[doc = "Source note path from the vault root."]
    pub source_path: String,
    #[doc = "Destination vault name."]
    pub destination_vault: String,
    #[doc = "Destination note path from the vault root."]
    pub destination_path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchMoveParams {
    #[doc = "Moves to plan or apply. Use exact vault-relative paths."]
    pub moves: Vec<BatchNoteMove>,
    #[doc = "Omit to preview. To apply, pass the ID of the plan the user confirmed."]
    pub preview_id: Option<String>,
    #[doc = "Allow copying referenced files to the destination vault. Keep originals."]
    #[serde(default)]
    pub allow_attachment_copy: bool,
    #[doc = "Allow converting cross-vault note embeds to clickable links."]
    #[serde(default)]
    pub allow_embed_conversion: bool,
    #[doc = "Allow converting cross-vault Canvas file cards to text cards with links."]
    #[serde(default)]
    pub allow_canvas_conversion: bool,
    #[doc = "Skip this many preview details. The response always includes all blockers."]
    #[serde(default)]
    pub offset: usize,
    #[doc = "Maximum preview details to return. Defaults to 50."]
    pub limit: Option<usize>,
}
