use serde::Deserialize;

use crate::vault::{SearchLimits, SearchType};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchVaultParams {
    /// Name of the vault to search in
    pub vault: String,
    /// Search query. For text search use the term directly; for tag search use "tag:" prefix (e.g. "tag:status/active")
    pub query: String,
    /// Optional subfolder path within the vault to limit the search scope
    pub path: Option<String>,
    /// Whether to perform case-sensitive search (default: false)
    #[serde(rename = "caseSensitive")]
    pub case_sensitive: Option<bool>,
    /// What to match the query against (default: "content")
    #[serde(rename = "searchType")]
    pub search_type: Option<SearchType>,
    /// Maximum number of files to return, best-matching first (default: 20)
    pub limit: Option<usize>,
    /// Skip this many files — use with `limit` to page through matches (default: 0)
    pub offset: Option<usize>,
    /// Maximum matching lines to quote per file (default: 3)
    #[serde(rename = "maxMatchesPerFile")]
    pub max_matches_per_file: Option<usize>,
}

impl SearchVaultParams {
    /// Result limits, with the defaults applied. These exist to keep a careless
    /// query from flooding the model's context: without them a common word on a
    /// large vault returns every matching line of every matching file.
    pub fn limits(&self) -> SearchLimits {
        let d = SearchLimits::default();
        SearchLimits {
            limit: self.limit.unwrap_or(d.limit),
            offset: self.offset.unwrap_or(d.offset),
            max_matches_per_file: self.max_matches_per_file.unwrap_or(d.max_matches_per_file),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(search_type: serde_json::Value) -> Result<SearchVaultParams, serde_json::Error> {
        serde_json::from_value(serde_json::json!({
            "vault": "v",
            "query": "q",
            "searchType": search_type,
        }))
    }

    #[test]
    fn known_search_types_parse() {
        assert_eq!(
            parse(serde_json::json!("filename")).unwrap().search_type,
            Some(SearchType::Filename)
        );
        assert_eq!(
            parse(serde_json::json!("both")).unwrap().search_type,
            Some(SearchType::Both)
        );
    }

    #[test]
    fn unknown_search_type_is_rejected() {
        // Previously an unrecognised value silently degraded to `content`, so a
        // typo returned the wrong kind of results with no indication of why.
        let err = parse(serde_json::json!("flename")).unwrap_err();
        assert!(
            err.to_string().contains("flename"),
            "the error must name the offending value, got: {err}"
        );
    }

    #[test]
    fn omitted_search_type_defaults_to_content() {
        let params = parse(serde_json::Value::Null).unwrap();
        assert_eq!(params.search_type.unwrap_or_default(), SearchType::Content);
    }
}
