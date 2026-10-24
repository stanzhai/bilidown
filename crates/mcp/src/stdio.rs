//! stdio ↔ HTTP bridge (Python `src/util/mcp/stdio_bridge.py`).
//!
//! Line-delimited JSON-RPC on stdin → one POST per message to the local
//! MCP endpoint → one ASCII-escaped JSON line per response on stdout.
//! Diagnostics go to stderr with the `[bilidown-mcp]` prefix: stdout is the
//! message channel and one stray log line would break the client.

use std::io::{BufRead, Write};
use std::path::Path;

use anyhow::{Context, Result};
use base64::Engine;
use serde_json::{json, Value};

const PROTOCOL_VERSION_KEY: &str = "io.modelcontextprotocol/protocolVersion";
/// HTTP headers only carry visible ASCII; non-ASCII tool names travel
/// sentinel-encoded (spec `Mcp-Name`).
const BASE64_PREFIX: &str = "=?base64?";
const BASE64_SUFFIX: &str = "?=";
const APP_NAME: &str = config::APP_NAME;

fn log(message: &str) {
    eprintln!("[bilidown-mcp] {message}");
}

fn encode_header_value(value: &str) -> String {
    let needs_encoding = !value.is_ascii()
        || value.chars().any(|ch| ch.is_control() || ch == '\u{7f}')
        || value != value.trim()
        || (value.starts_with(BASE64_PREFIX) && value.ends_with(BASE64_SUFFIX));

    if !needs_encoding {
        return value.to_owned();
    }

    let payload = base64::engine::general_purpose::STANDARD.encode(value.as_bytes());
    format!("{BASE64_PREFIX}{payload}{BASE64_SUFFIX}")
}

/// `~/Library/Application Support/bilidown/config.json` on macOS,
/// `%APPDATA%` on Windows, XDG data elsewhere (Python `config_path`).
fn default_config_path() -> std::path::PathBuf {
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(dirs_like_appdata)
    } else if cfg!(target_os = "macos") {
        std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join("Library/Application Support")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
                    .join(".local/share")
            })
    };
    base.join(APP_NAME).join("config.json")
}

fn dirs_like_appdata() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    std::path::PathBuf::from(home).join("AppData/Roaming")
}

/// Find `key` at any depth in the nested config structure
/// (qfluentwidgets stores by group; digging avoids coupling to group names).
fn dig<'a>(node: &'a Value, key: &str) -> Option<&'a Value> {
    match node {
        Value::Object(map) => {
            if let Some(found) = map.get(key) {
                return Some(found);
            }
            map.values().find_map(|value| dig(value, key))
        }
        _ => None,
    }
}

/// Token from the config file, `None` when absent/unreadable.
pub fn load_config_token(explicit_path: Option<&Path>) -> Result<Option<String>> {
    let path = explicit_path
        .map(std::path::PathBuf::from)
        .unwrap_or_else(default_config_path);

    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            log(&format!("未找到配置文件：{}", path.display()));
            return Ok(None);
        }
        Err(err) => {
            return Err(err).with_context(|| format!("读取配置文件失败：{}", path.display()))
        }
    };

    let data: Value = serde_json::from_str(&text)
        .with_context(|| format!("读取配置文件失败：{}", path.display()))?;

    Ok(dig(&data, "mcp_token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .map(str::to_owned))
}

fn rpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// Build the forwarding headers for one message (Python `Bridge.build_headers`).
fn build_headers(message: &Value, token: &str) -> Vec<(String, String)> {
    let mut headers = vec![
        ("Content-Type".into(), "application/json".into()),
        (
            "Accept".into(),
            "application/json, text/event-stream".into(),
        ),
        ("Authorization".into(), format!("Bearer {token}")),
    ];

    let method = message.get("method").and_then(Value::as_str);
    if let Some(method) = method {
        headers.push(("Mcp-Method".into(), encode_header_value(method)));
    }

    if let Some(params) = message.get("params").and_then(Value::as_object) {
        if let Some(version) = params
            .get("_meta")
            .and_then(Value::as_object)
            .and_then(|meta| meta.get(PROTOCOL_VERSION_KEY))
            .and_then(Value::as_str)
        {
            headers.push(("MCP-Protocol-Version".into(), version.to_owned()));
        }

        let name = params
            .get("name")
            .or_else(|| params.get("uri"))
            .and_then(Value::as_str);
        if let Some(name) = name.filter(|name| !name.is_empty()) {
            headers.push(("Mcp-Name".into(), encode_header_value(name)));
        }
    }

    headers
}

/// One message → one response line (Python `Bridge.forward`). `None` for
/// notifications and unparseable bodies.
async fn forward(
    client: &reqwest::Client,
    url: &str,
    message: &Value,
    token: &str,
) -> Result<Option<Value>> {
    let body = serde_json::to_vec(message)?;
    let mut request = client.post(url).body(body);
    for (key, value) in build_headers(message, token) {
        request = request.header(&key, value);
    }

    let response = request.send().await?;
    let status = response.status();
    let raw = response.bytes().await?;

    if raw.is_empty() {
        return Ok(None);
    }

    match serde_json::from_slice::<Value>(&raw) {
        Ok(value) => Ok(Some(value)),
        // The server's error bodies are themselves JSON-RPC errors — pass
        // them through so the client sees the real reason.
        Err(_) if !status.is_success() => {
            let snippet = String::from_utf8_lossy(&raw[..raw.len().min(200)]).into_owned();
            Ok(Some(json!({
                "error": {"code": -32603, "message": format!("HTTP {status}: {snippet}")}
            })))
        }
        Err(_) => Ok(None),
    }
}

/// The bridge loop: stdin lines in, stdout lines out, until EOF.
pub async fn run(port: u16, token: String) -> Result<()> {
    let url = format!("http://127.0.0.1:{port}/mcp");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line.context("read stdin")?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let Ok(message) = serde_json::from_str::<Value>(line) else {
            log("收到非法 JSON，已忽略");
            continue;
        };
        if !message.is_object() {
            log("收到非对象消息，已忽略");
            continue;
        }

        let response = match forward(&client, &url, &message, &token).await {
            Ok(Some(value)) => value,
            Ok(None) => continue,
            Err(err) => {
                let Some(id) = message.get("id") else {
                    continue;
                };
                rpc_error(id, -32603, &format!("Cannot reach bilidown: {err}"))
            }
        };

        let encoded = ascii_escaped(&response);
        writeln!(out, "{encoded}")?;
        out.flush()?;
    }

    Ok(())
}

/// JSON with every non-ASCII char as `\uXXXX` — the bytes on the wire are
/// pure ASCII, immune to any single-byte/multi-byte codec mismatch
/// anywhere in the chain (stdio transport is UTF-8, but silent mojibake
/// beats a spec argument).
fn ascii_escaped(value: &Value) -> String {
    fn escape_into(buffer: &mut String, value: &Value) {
        match value {
            Value::Null => buffer.push_str("null"),
            Value::Bool(b) => buffer.push_str(if *b { "true" } else { "false" }),
            Value::Number(n) => buffer.push_str(&n.to_string()),
            Value::String(s) => escape_string(buffer, s),
            Value::Array(items) => {
                buffer.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        buffer.push(',');
                    }
                    escape_into(buffer, item);
                }
                buffer.push(']');
            }
            Value::Object(map) => {
                buffer.push('{');
                for (index, (key, item)) in map.iter().enumerate() {
                    if index > 0 {
                        buffer.push(',');
                    }
                    escape_string(buffer, key);
                    buffer.push(':');
                    escape_into(buffer, item);
                }
                buffer.push('}');
            }
        }
    }

    fn escape_string(buffer: &mut String, text: &str) {
        buffer.push('"');
        for ch in text.chars() {
            match ch {
                '"' => buffer.push_str("\\\""),
                '\\' => buffer.push_str("\\\\"),
                '\n' => buffer.push_str("\\n"),
                '\r' => buffer.push_str("\\r"),
                '\t' => buffer.push_str("\\t"),
                c if (c as u32) < 0x20 || !c.is_ascii() => {
                    buffer.push_str(&format!("\\u{:04x}", c as u32));
                }
                c => buffer.push(c),
            }
        }
        buffer.push('"');
    }

    let mut buffer = String::new();
    escape_into(&mut buffer, value);
    buffer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_values_are_ascii_or_sentinel() {
        assert_eq!(encode_header_value("list_tasks"), "list_tasks");
        assert_eq!(encode_header_value("工具"), "=?base64?5bel5YW3?=");
        // Leading whitespace must not survive header parsing.
        assert!(encode_header_value(" spaced").starts_with(BASE64_PREFIX));
    }

    #[test]
    fn dig_finds_token_at_any_depth() {
        let config = json!({"Mcp": {"mcp_token": "t", "mcp_port": 23330}});
        assert_eq!(dig(&config, "mcp_token"), Some(&json!("t")));
        assert_eq!(dig(&config, "mcp_port"), Some(&json!(23330)));
        assert_eq!(dig(&config, "absent"), None);
    }

    #[test]
    fn ascii_escaped_leaves_only_ascii() {
        let value = json!({"title": "中文", "n": 1, "list": ["a", null]});
        let encoded = ascii_escaped(&value);
        assert!(encoded.is_ascii(), "{encoded}");
        assert_eq!(serde_json::from_str::<Value>(&encoded).unwrap(), value);
    }

    #[tokio::test]
    async fn forward_round_trips_through_a_local_server() {
        use serde_json::json;
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(json!({"jsonrpc": "2.0", "id": 1, "result": {"ok": true}})),
            )
            .mount(&server)
            .await;

        let url = format!("{}/mcp", server.uri());
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::new();
        let message = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "parse_url", "_meta": {PROTOCOL_VERSION_KEY: "2026-07-28"}}
        });
        let response = forward(&client, &url, &message, "token")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response["result"]["ok"], json!(true));

        let requests = server.received_requests().await.unwrap();
        let sent = &requests[0];
        let auth = sent
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        assert_eq!(auth, "Bearer token");
        let version = sent
            .headers
            .get("mcp-protocol-version")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        assert_eq!(version, "2026-07-28");
        let name = sent
            .headers
            .get("mcp-name")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        assert_eq!(name, "parse_url");
    }
}
