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
