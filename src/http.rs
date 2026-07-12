//! Serving MCP over Streamable HTTP, as well as stdio.
//!
//! stdio gives one client one server, spawned as a child process. HTTP lets
//! several clients — or a client that isn't allowed to spawn processes — talk to
//! one long-lived server that shares a single `VaultManager`, and therefore a
//! single write lock.
//!
//! # Why the Origin check is not optional
//!
//! This server has full read/write access to the user's notes and **no
//! authentication**. Bound to `127.0.0.1`, that sounds safe — it isn't, on its
//! own. Any web page the user visits can make their browser POST to
//! `http://127.0.0.1:<port>/mcp`; the request comes from *their* machine, and
//! there is no password to fail. That's the DNS-rebinding attack, and it's why
//! the MCP specification requires local HTTP servers to validate `Origin`.
//!
//! So we do: a request whose `Origin` names a site that isn't localhost is
//! refused. Native MCP clients don't send `Origin` at all, so it costs them
//! nothing, and browsers can't forge it.

use std::net::SocketAddr;

use axum::{
    Router,
    extract::Request,
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::Response,
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};

use crate::{handler::ObsidianHandler, vault::VaultManager};

/// Where the MCP endpoint lives. Clients are configured with `http://host:port/mcp`.
pub const MCP_PATH: &str = "/mcp";

/// Is this `Origin` the user's own machine?
///
/// Only the scheme and host matter. `null` (a sandboxed iframe or a `file://`
/// page) is *not* trusted — an attacker can produce it.
pub(crate) fn origin_is_local(origin: &str) -> bool {
    let Some(rest) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    else {
        return false;
    };
    // Strip the port; an IPv6 host is bracketed, so find the port after the `]`.
    let host = match rest.rsplit_once(']') {
        Some((v6, _)) => &v6[1..],
        None => rest.split(':').next().unwrap_or(rest),
    };

    host == "localhost" || host == "127.0.0.1" || host == "::1"
}

/// Refuse cross-origin requests. See the module docs — this is the only thing
/// standing between a malicious web page and the user's vault.
async fn guard_origin(request: Request, next: Next) -> Result<Response, StatusCode> {
    match request.headers().get(header::ORIGIN) {
        // Native MCP clients send no Origin. Browsers always do, cross-origin.
        None => Ok(next.run(request).await),
        Some(origin) if HeaderValue::to_str(origin).is_ok_and(origin_is_local) => {
            Ok(next.run(request).await)
        }
        Some(origin) => {
            tracing::warn!(
                origin = ?origin,
                "refused a cross-origin request — a web page may be trying to reach your vault"
            );
            Err(StatusCode::FORBIDDEN)
        }
    }
}

/// Serve until the process is killed.
///
/// Every session shares one `ObsidianHandler`, and therefore one `VaultManager`
/// and one write lock — so two HTTP clients editing the same note serialise
/// exactly as two stdio calls would.
pub async fn serve(vaults: VaultManager, no_edit: bool, addr: SocketAddr) -> anyhow::Result<()> {
    if !addr.ip().is_loopback() {
        // Worth shouting about: there is no authentication. Anyone who can reach
        // this port can read and rewrite every note.
        tracing::warn!(%addr, "listening on a non-loopback address — this server has no authentication, so anyone who can reach this port can read and rewrite the vault");
        eprintln!(
            "WARNING: {addr} is not a loopback address. This server has no authentication —\n\
             anyone who can reach this port can read and rewrite your notes."
        );
    }

    let handler = ObsidianHandler::with_options(vaults, no_edit);
    let service = StreamableHttpService::new(
        move || Ok(handler.clone()),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default(),
    );

    let app = Router::new()
        .nest_service(MCP_PATH, service)
        .layer(middleware::from_fn(guard_origin));

    let listener = tokio::net::TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?;
    tracing::info!(%bound, "serving MCP over Streamable HTTP");
    // stderr, never stdout: in stdio mode stdout is the protocol stream, and the
    // two modes must not diverge in where they write.
    eprintln!("MCP endpoint: http://{bound}{MCP_PATH}");

    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_users_own_machine_is_allowed() {
        for origin in [
            "http://localhost",
            "http://localhost:3000",
            "http://127.0.0.1:8080",
            "https://localhost:443",
            "http://[::1]:3000",
        ] {
            assert!(origin_is_local(origin), "{origin} must be allowed");
        }
    }

    #[test]
    fn a_web_page_is_not() {
        for origin in [
            "https://evil.com",
            "http://evil.com:3000",
            // The classic near-miss: a hostname that merely *starts* with the
            // one we trust.
            "http://localhost.evil.com",
            "http://127.0.0.1.evil.com",
            // A sandboxed iframe or a file:// page. An attacker can produce it,
            // so it is not the user's machine as far as we're concerned.
            "null",
            "file://",
        ] {
            assert!(!origin_is_local(origin), "{origin} must be refused");
        }
    }
}
