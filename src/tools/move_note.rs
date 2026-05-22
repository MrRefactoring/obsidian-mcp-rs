use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MoveNoteParams {
    /// Name of the vault containing the note
    pub vault: String,
    /// Note filename (with or without .md extension). Do not include path separators.
    pub filename: String,
    /// Current subfolder path (leave empty for vault root)
    pub folder: Option<String>,
    /// Destination folder path (leave empty for vault root)
    #[serde(rename = "newFolder")]
    pub new_folder: Option<String>,
    /// New filename (leave empty to keep the same name)
    #[serde(rename = "newFilename")]
    pub new_filename: Option<String>,
}
