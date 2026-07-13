use thiserror::Error;

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("Vault '{0}' not found. Available vaults: {1}")]
    VaultNotFound(String, String),

    #[error(
        "Vault '{0}' is configured to point at '{1}', but that directory does not exist. \
         The path was most likely mistyped when this server was set up — tell the user, \
         and do not report the vault as empty."
    )]
    VaultUnavailable(String, String),

    #[error(
        "Note '{0}' not found in vault '{1}'. Check the folder, or run search-vault with searchType=\"filename\" to locate it."
    )]
    NoteNotFound(String, String),

    #[error("Note '{0}' already exists in vault '{1}'")]
    NoteAlreadyExists(String, String),

    #[error("Directory '{0}' already exists")]
    DirectoryAlreadyExists(String),

    #[error("Invalid path: {0}")]
    InvalidPath(String),

    #[error("Search text not found in note '{0}'")]
    SearchTextNotFound(String),

    #[error(
        "Target '{0}' not found in note '{1}'. Read the note with view=\"outline\" to list its headings and block references."
    )]
    TargetNotFound(String, String),

    #[error("Invalid frontmatter in '{0}': {1}")]
    InvalidFrontmatter(String, String),

    #[error("IO error for '{0}': {1}")]
    Io(String, #[source] std::io::Error),

    #[error("Search error: {0}")]
    Search(String),

    #[error("Invalid regex '{0}': {1}. Fix the pattern, or drop regex=true to search for words.")]
    InvalidRegex(String, String),
}

impl VaultError {
    pub fn io(path: impl Into<String>, err: std::io::Error) -> Self {
        Self::Io(path.into(), err)
    }

    /// Tool *execution* errors: the request was well-formed but the operation
    /// can't complete given the vault's current state. Per the MCP spec these
    /// are reported to the model as `isError: true` tool results (so it can
    /// self-correct), not as JSON-RPC protocol errors. Malformed-request errors
    /// (bad vault/path/args) and server faults (IO/search) are not in this set.
    pub fn is_tool_execution_error(&self) -> bool {
        matches!(
            self,
            VaultError::VaultUnavailable(..)
                | VaultError::NoteNotFound(..)
                | VaultError::NoteAlreadyExists(..)
                | VaultError::DirectoryAlreadyExists(..)
                | VaultError::SearchTextNotFound(..)
                | VaultError::TargetNotFound(..)
        )
    }
}

impl From<VaultError> for rmcp::ErrorData {
    fn from(err: VaultError) -> Self {
        use rmcp::model::ErrorCode;
        // Client mistakes (bad vault/note/path) map to INVALID_PARAMS so the MCP
        // client can tell them apart from genuine server faults (IO/search).
        let code = match &err {
            VaultError::VaultNotFound(..)
            | VaultError::VaultUnavailable(..)
            | VaultError::NoteNotFound(..)
            | VaultError::NoteAlreadyExists(..)
            | VaultError::DirectoryAlreadyExists(..)
            | VaultError::InvalidPath(..)
            | VaultError::SearchTextNotFound(..)
            | VaultError::TargetNotFound(..)
            | VaultError::InvalidFrontmatter(..)
            | VaultError::InvalidRegex(..) => ErrorCode::INVALID_PARAMS,
            VaultError::Io(..) | VaultError::Search(..) => ErrorCode::INTERNAL_ERROR,
        };
        rmcp::ErrorData::new(code, err.to_string(), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_not_found_message() {
        let e = VaultError::VaultNotFound("myv".into(), "a, b".into());
        assert!(e.to_string().contains("myv"));
        assert!(e.to_string().contains("a, b"));
    }

    #[test]
    fn note_not_found_message() {
        let e = VaultError::NoteNotFound("/path/note.md".into(), "vault".into());
        assert!(e.to_string().contains("/path/note.md"));
    }

    #[test]
    fn note_already_exists_message() {
        let e = VaultError::NoteAlreadyExists("/p".into(), "v".into());
        assert!(e.to_string().contains("already exists"));
    }

    #[test]
    fn directory_already_exists_message() {
        let e = VaultError::DirectoryAlreadyExists("/dir".into());
        assert!(e.to_string().contains("/dir"));
    }

    #[test]
    fn invalid_path_message() {
        let e = VaultError::InvalidPath("bad path".into());
        assert!(e.to_string().contains("bad path"));
    }

    #[test]
    fn invalid_frontmatter_message() {
        let e = VaultError::InvalidFrontmatter("note.md".into(), "bad yaml".into());
        assert!(e.to_string().contains("note.md"));
    }

    #[test]
    fn search_error_message() {
        let e = VaultError::Search("regex fail".into());
        assert!(e.to_string().contains("regex fail"));
    }

    #[test]
    fn io_constructor() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "oops");
        let e = VaultError::io("/some/path", io_err);
        assert!(e.to_string().contains("/some/path"));
    }

    #[test]
    fn from_vault_error_into_error_data() {
        let e = VaultError::NoteNotFound("n".into(), "v".into());
        let data: rmcp::ErrorData = e.into();
        assert!(data.message.contains("not found"));
    }

    #[test]
    fn client_errors_map_to_invalid_params() {
        use rmcp::model::ErrorCode;
        for e in [
            VaultError::VaultNotFound("v".into(), "".into()),
            VaultError::NoteNotFound("n".into(), "v".into()),
            VaultError::NoteAlreadyExists("n".into(), "v".into()),
            VaultError::DirectoryAlreadyExists("d".into()),
            VaultError::InvalidPath("bad".into()),
            VaultError::InvalidFrontmatter("n".into(), "bad".into()),
            VaultError::InvalidRegex("[0-9".into(), "unclosed class".into()),
        ] {
            let data: rmcp::ErrorData = e.into();
            assert_eq!(data.code, ErrorCode::INVALID_PARAMS);
        }
    }

    #[test]
    fn tool_execution_errors_are_classified() {
        // Business errors the model can self-correct on → isError result.
        assert!(VaultError::NoteNotFound("n".into(), "v".into()).is_tool_execution_error());
        assert!(VaultError::NoteAlreadyExists("n".into(), "v".into()).is_tool_execution_error());
        assert!(VaultError::DirectoryAlreadyExists("d".into()).is_tool_execution_error());
        assert!(VaultError::SearchTextNotFound("n".into()).is_tool_execution_error());
        // A missing patch target is self-correctable: the message tells the model
        // to read the outline and pick a real one.
        let target = VaultError::TargetNotFound("## Log".into(), "n.md".into());
        assert!(target.is_tool_execution_error());
        assert!(target.to_string().contains("outline"));
        // An unparseable regex is a malformed *argument*, so it stays a protocol
        // error (INVALID_PARAMS) rather than an isError result — but the message
        // hands the model back its own pattern and a way out.
        let regex = VaultError::InvalidRegex("555-[0-9".into(), "unclosed class".into());
        assert!(!regex.is_tool_execution_error());
        assert!(regex.to_string().contains("555-[0-9"));
        assert!(regex.to_string().contains("regex=true"));
        // Malformed-request / server faults → protocol errors, not isError.
        assert!(!VaultError::VaultNotFound("v".into(), "".into()).is_tool_execution_error());
        assert!(!VaultError::InvalidPath("bad".into()).is_tool_execution_error());
        assert!(!VaultError::Search("regex".into()).is_tool_execution_error());
        assert!(!VaultError::io("/p", std::io::Error::other("x")).is_tool_execution_error());
    }

    #[test]
    fn server_errors_map_to_internal_error() {
        use rmcp::model::ErrorCode;
        let io = VaultError::io("/p", std::io::Error::other("boom"));
        let search = VaultError::Search("regex".into());
        for e in [io, search] {
            let data: rmcp::ErrorData = e.into();
            assert_eq!(data.code, ErrorCode::INTERNAL_ERROR);
        }
    }
}
