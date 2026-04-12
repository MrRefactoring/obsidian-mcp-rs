use rmcp::handler::server::wrapper::Parameters;
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateDirectoryParams {
    /// Name of the vault where the directory should be created
    pub vault: String,
    /// Path of the directory to create, relative to the vault root (e.g. "journal/2024")
    pub path: String,
    /// Create parent directories if they do not exist (default: true)
    pub recursive: Option<bool>,
}

pub type CreateDirectory = Parameters<CreateDirectoryParams>;
