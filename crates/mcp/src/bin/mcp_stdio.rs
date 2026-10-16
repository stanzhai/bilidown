//! `mcp-stdio`: stdio ↔ TCP bridge binary (Python `mcp_stdio.py`).
//!
//! Reads line-delimited JSON-RPC on stdin, forwards each to the local
//! HTTP MCP endpoint, prints one ASCII-escaped JSON line per response on
//! stdout. Auto-launch of the GUI app is not ported: run Bili23
//! Downloader separately.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut port = mcp::DEFAULT_PORT;
    let mut token = String::new();
    let mut config_path: Option<std::path::PathBuf> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--port" => port = iter.next().and_then(|v| v.parse().ok()).unwrap_or(port),
            "--token" => token = iter.next().cloned().unwrap_or_default(),
            "--config" => config_path = iter.next().map(std::path::PathBuf::from),
            "--help" | "-h" => {
                println!("mcp-stdio [--port PORT] [--token TOKEN] [--config PATH]");
                return ExitCode::SUCCESS;
            }
            _ => {}
        }
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(err) => {
            eprintln!("[bili23-mcp] failed to start runtime: {err}");
            return ExitCode::from(2);
        }
    };

    runtime.block_on(async move {
        // Token from --token, else dig it out of the config file.
        let token = if !token.is_empty() {
            token
        } else {
            match mcp::stdio::load_config_token(config_path.as_deref()) {
                Ok(Some(t)) => t,
                Ok(None) | Err(_) => {
                    eprintln!("[bili23-mcp] 未找到 MCP 令牌。");
                    eprintln!("[bili23-mcp] 请先在 Bili23 Downloader 中启用 MCP 服务器。");
                    return ExitCode::from(2);
                }
            }
        };

        match mcp::stdio::run(port, token).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("[bili23-mcp] {err}");
                ExitCode::from(2)
            }
        }
    })
}
