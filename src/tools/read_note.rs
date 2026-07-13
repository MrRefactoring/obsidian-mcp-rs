use serde::Deserialize;

use crate::vault::{NoteView, ReadWindow};

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
    /// First line to return, 1-based (default: 1). These are the same line
    /// numbers `view: "outline"` reports, so one can be pasted straight in.
    pub offset: Option<usize>,
    /// Most lines to return (default: 400). A longer note is cut off with a
    /// marker saying which lines you got and what `offset` to pass for the rest.
    /// Ignored by `view: "outline"`, which is short by construction.
    pub limit: Option<usize>,
}

impl ReadNoteParams {
    /// The slice of the note to read, with the defaults applied. These exist for
    /// the same reason `search-vault`'s limits do: without them one read of one
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
