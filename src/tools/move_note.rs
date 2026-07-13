use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MoveNoteParams {
    /// Name of the vault containing the note
    pub vault: String,
    /// Note filename (with or without .md extension). Do not include path separators.
    pub filename: String,
    /// Current subfolder path (omit for vault root)
    pub folder: Option<String>,
    /// Destination folder. Omit to keep the note in its current folder — that is
    /// how you rename in place. Pass "" to move it to the vault root.
    #[serde(rename = "newFolder")]
    pub new_folder: Option<String>,
    /// New filename (omit to keep the same name)
    #[serde(rename = "newFilename")]
    pub new_filename: Option<String>,
}
