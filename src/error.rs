use thiserror::Error;

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("Vault '{0}' not found. Available vaults: {1}")]
    VaultNotFound(String, String),

    #[error("Note '{0}' not found in vault '{1}'")]
    NoteNotFound(String, String),

    #[error("Note '{0}' already exists in vault '{1}'")]
    NoteAlreadyExists(String, String),

    #[error("Directory '{0}' already exists")]
    DirectoryAlreadyExists(String),

    #[error("Invalid path: {0}")]
    InvalidPath(String),

    #[error("Invalid frontmatter in '{0}': {1}")]
    InvalidFrontmatter(String, String),

    #[error("IO error for '{0}': {1}")]
    Io(String, #[source] std::io::Error),

    #[error("Search error: {0}")]
    Search(String),
}

impl VaultError {
    pub fn io(path: impl Into<String>, err: std::io::Error) -> Self {
        Self::Io(path.into(), err)
    }
}

impl From<VaultError> for rmcp::ErrorData {
    fn from(err: VaultError) -> Self {
        rmcp::ErrorData::new(
            rmcp::model::ErrorCode::INTERNAL_ERROR,
            err.to_string(),
            None,
        )
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
}
