use serde::Deserialize;

use crate::vault::{EditOperation, TargetKind};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EditNoteParams {
    /// Name of the vault containing the note
    pub vault: String,
    /// Note filename (with or without .md extension). Do not include path separators.
    pub filename: String,
    /// What to do: "append" adds to the end, "prepend" to the start, "replace" overwrites, "find_and_replace" swaps the first occurrence of `search` for `content`
    pub operation: EditOperation,
    /// Content to apply according to the chosen operation
    pub content: String,
    /// Optional subfolder path relative to vault root
    pub folder: Option<String>,
    /// Text to find when using "find_and_replace"
    pub search: Option<String>,
    /// Narrow the edit to one part of the note instead of the whole file:
    /// "heading" for a section, "block" for an Obsidian `^block-id`. Needs
    /// `target`. Read the note with view="outline" to list what's available.
    #[serde(rename = "targetType")]
    pub target_type: Option<TargetKind>,
    /// Which heading ("## Log" or just "Log") or block id ("^n1" or "n1") to
    /// edit. Only used with `targetType`. The first match wins.
    pub target: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: serde_json::Value) -> Result<EditNoteParams, serde_json::Error> {
        serde_json::from_value(json)
    }

    #[test]
    fn unknown_operation_is_rejected() {
        // It used to fall through to the domain as a string. Typing it means the
        // model gets an INVALID_PARAMS naming the offending value instead.
        let err = parse(serde_json::json!({
            "vault": "v", "filename": "n", "operation": "destroy", "content": "x"
        }))
        .expect_err("an unknown operation must not deserialize");
        assert!(err.to_string().contains("destroy"), "{err}");
    }

    #[test]
    fn the_documented_operations_all_parse() {
        for op in ["append", "prepend", "replace", "find_and_replace"] {
            assert!(
                parse(serde_json::json!({
                    "vault": "v", "filename": "n", "operation": op, "content": "x"
                }))
                .is_ok(),
                "'{op}' must parse"
            );
        }
    }

    #[test]
    fn a_target_parses_into_its_typed_kind() {
        let p = parse(serde_json::json!({
            "vault": "v", "filename": "n", "operation": "append", "content": "x",
            "targetType": "heading", "target": "## Log"
        }))
        .unwrap();
        assert_eq!(p.target_type, Some(TargetKind::Heading));
        assert_eq!(p.target.as_deref(), Some("## Log"));
    }
}
