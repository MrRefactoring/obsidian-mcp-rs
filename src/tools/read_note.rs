use serde::Deserialize;

use crate::vault::NoteView;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReadNoteParams {
    /// Name of the vault containing the note
    pub vault: String,
    /// Note filename (with or without .md extension). Do not include path separators.
    pub filename: String,
    /// Optional subfolder path relative to vault root (e.g. "journal/2024")
    pub folder: Option<String>,
    /// How much to return: "content" for the note's text (default), or
    /// "outline" for just its headings, block references and frontmatter keys —
    /// the targets `edit-note` and `frontmatter` can aim at.
    pub view: Option<NoteView>,
}
