use serde::Deserialize;

use crate::vault::{NoteView, ReadWindow};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReadNoteParams {
    /// Name of the vault.
    pub vault: String,
    /// Note path relative to the vault root. You can omit `.md`. Use the `path` from `search_notes`.
    pub filename: String,
    /// Folder path relative to the vault root (e.g. "journal/2024")
    pub folder: Option<String>,
    /// How much to return: "content" for the note's text (default), or
    /// "outline" for just its headings, block references and frontmatter keys —
    /// the targets `edit_note` and `note_properties` can aim at.
    pub view: Option<NoteView>,
    /// First line to return, 1-based (default: 1). These are the same line
    /// numbers `view: "outline"` reports, so one can be pasted straight in.
    pub offset: Option<usize>,
    /// Maximum lines to return (default: 400). A marker identifies the returned
    /// lines and the `offset` for the rest. Ignored by `view: "outline"`.
    pub limit: Option<usize>,
}

impl ReadNoteParams {
    /// The slice of the note to read, with the defaults applied. These exist for
    /// the same reason `search_notes`'s limits do: without them one read of one
    /// long note can consume the model's whole context window.
    pub fn window(&self) -> ReadWindow {
        let d = ReadWindow::default();
        ReadWindow {
            // Zero would ask for nothing at all, which is never what was meant.
            offset: self.offset.unwrap_or(d.offset).max(1),
            limit: self.limit.unwrap_or(d.limit).max(1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::DEFAULT_READ_LINES;

    fn parse(v: serde_json::Value) -> ReadNoteParams {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn defaults_protect_the_context_window() {
        let p = parse(serde_json::json!({"vault": "v", "filename": "n"}));
        assert_eq!(
            p.window(),
            ReadWindow {
                offset: 1,
                limit: DEFAULT_READ_LINES,
            }
        );
    }

    #[test]
    fn a_zero_is_read_as_the_smallest_real_request() {
        // Asking for line zero, or for zero lines, is a mistake — answering it
        // literally would return nothing and read as "the note is empty".
        let p = parse(serde_json::json!({"vault": "v", "filename": "n", "offset": 0, "limit": 0}));
        assert_eq!(
            p.window(),
            ReadWindow {
                offset: 1,
                limit: 1
            }
        );
    }
}
