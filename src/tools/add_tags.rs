use serde::Deserialize;

use crate::vault::{TagLocation, TagPosition};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AddTagsParams {
    /// Name of the vault containing the notes
    pub vault: String,
    /// Notes to tag, as paths relative to the vault root (e.g. "projects/apollo.md").
    /// All of them must exist — if any does not, nothing is changed.
    pub files: Vec<String>,
    /// Array of tags to add (e.g. "status/active", "project/docs")
    pub tags: Vec<String>,
    /// Where to write each tag (default: "both"). Note that "both" puts the tag in
    /// the note **twice** — once in the frontmatter, and once inline in the body.
    pub location: Option<TagLocation>,
    /// Normalize tag format — lowercase, spaces to hyphens (e.g. "My Tag" -> my-tag). Default: true
    pub normalize: Option<bool>,
    /// Where an inline tag goes in the body (default: "end"). Ignored when
    /// `location` is "frontmatter".
    pub position: Option<TagPosition>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(extra: serde_json::Value) -> Result<AddTagsParams, serde_json::Error> {
        let mut base = serde_json::json!({"vault": "v", "files": ["a.md"], "tags": ["t"]});
        for (k, val) in extra.as_object().unwrap() {
            base[k.as_str()] = val.clone();
        }
        serde_json::from_value(base)
    }

    #[test]
    fn the_defaults_are_both_and_end() {
        let p = parse(serde_json::json!({})).unwrap();
        assert_eq!(p.location.unwrap_or_default(), TagLocation::Both);
        assert_eq!(p.position.unwrap_or_default(), TagPosition::End);
    }

    #[test]
    fn a_mistyped_location_is_rejected_instead_of_silently_meaning_both() {
        // `location: "Frontmatter"` — one capital letter — used to fall through a
        // catch-all arm and write the tag to *both* places, with nothing to say
        // anything had gone wrong. The same class of bug `search-vault` already
        // fixed by typing its vocabulary.
        let err = parse(serde_json::json!({"location": "Frontmatter"})).unwrap_err();
        assert!(err.to_string().contains("Frontmatter"), "{err}");
    }

    #[test]
    fn a_mistyped_position_is_rejected_instead_of_silently_meaning_end() {
        let err = parse(serde_json::json!({"position": "begining"})).unwrap_err();
        assert!(err.to_string().contains("begining"), "{err}");
    }

    #[test]
    fn the_legal_values_parse() {
        for (loc, want) in [
            ("frontmatter", TagLocation::Frontmatter),
            ("content", TagLocation::Content),
            ("both", TagLocation::Both),
        ] {
            let p = parse(serde_json::json!({ "location": loc })).unwrap();
            assert_eq!(p.location.unwrap(), want);
        }
        for (pos, want) in [("start", TagPosition::Start), ("end", TagPosition::End)] {
            let p = parse(serde_json::json!({ "position": pos })).unwrap();
            assert_eq!(p.position.unwrap(), want);
        }
    }
}
