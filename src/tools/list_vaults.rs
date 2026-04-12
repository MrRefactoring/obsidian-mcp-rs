use rmcp::handler::server::wrapper::Parameters;
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub struct ListVaultsParams {}

pub type ListVaults = Parameters<ListVaultsParams>;
