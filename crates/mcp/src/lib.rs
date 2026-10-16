//! MCP server library: local loopback JSON-RPC endpoint exposing
//! parse/download/task tools driven by the real parser + task stack.
//!
//! Layout mirrors the Python `src/util/mcp/` package: `protocol` (framing
//! and error codes), `server` (the axum app), `state` (shared world),
//! `tools` (the tool registry), `stdio` (the stdio bridge).

pub mod protocol;
pub mod server;
pub mod state;
pub mod stdio;
pub mod tools;

use std::sync::Arc;

use anyhow::Context;
use base64::Engine;
use serde_json::json;

use state::McpState;
use tasks::TaskManager;

/// Default `Mcp.mcp_port` (Python `MCP_PORT`).
pub const DEFAULT_PORT: u16 = 23330;

/// Auto-generated token size in bytes (32 → 43 URL-safe base64 chars).
const TOKEN_BYTES: usize = 32;

/// Running server; dropping stops it.
pub struct McpHandle {
    shutdown: tokio::sync::watch::Sender<bool>,
    port: u16,
    token: String,
}

impl McpHandle {
    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// Stop the server; the graceful-shutdown watcher closes the listener.
    pub fn stop(self) {
        let _ = self.shutdown.send(true);
    }
}

/// Start the MCP server on 127.0.0.1 per the config's `Mcp` group.
/// Generates + persists a token when `mcp_token` is empty; a disabled
/// config returns `Ok(None)`.
pub async fn start(
    config: Arc<parking_lot::Mutex<config::ConfigStore>>,
    manager: Arc<TaskManager>,
) -> anyhow::Result<Option<McpHandle>> {
    let (enabled, configured_port, configured_token) = {
        let store = config.lock();
        (
            store
                .get("MCP", "mcp_enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            store
                .get("MCP", "mcp_port")
                .and_then(|v| v.as_i64())
                .unwrap_or(DEFAULT_PORT as i64),
            store
                .get("MCP", "mcp_token")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned(),
        )
    };
    if !enabled {
        return Ok(None);
    }

    // Token: configured, or generated and persisted (restarts keep clients valid).
    let token = if configured_token.is_empty() {
        let token = generate_token();
        config.lock().set("MCP", "mcp_token", json!(token));
        config.lock().save().context("save mcp_token")?;
        token
    } else {
        configured_token
    };

    let state = Arc::new(McpState::new(config, manager, ""));
    let app = server::app(Arc::new(server::McpServer {
        dispatcher: Arc::new(protocol::Dispatcher::new(tools::register_all(
            state.clone(),
        ))),
        token: token.clone(),
    }));

    let port = u16::try_from(configured_port).unwrap_or(DEFAULT_PORT);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .with_context(|| format!("bind 127.0.0.1:{port}"))?;
    let bound_port = listener.local_addr()?.port();

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.changed().await;
            })
            .await;
    });

    Ok(Some(McpHandle {
        shutdown: shutdown_tx,
        port: bound_port,
        token,
    }))
}

fn generate_token() -> String {
    use rand::RngExt;
    let mut bytes = [0u8; TOKEN_BYTES];
    rand::rng().fill(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}
