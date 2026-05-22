//! Library surface of `obsidian-mcp-rs`.
//!
//! The binary (`src/main.rs`) is a thin CLI/transport wrapper over these
//! modules; exposing them as a library also lets `benches/` and integration
//! tests link against the domain logic.

pub mod error;
pub mod handler;
pub mod install;
pub mod tools;
pub mod vault;
