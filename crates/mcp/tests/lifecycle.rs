//! Library lifecycle: `start` honors the Mcp config group, auto-generates
//! + persists the token, and serves a real tools/list round trip.

use std::sync::Arc;

use serde_json::json;

#[tokio::test]
async fn start_disabled_returns_none() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store =
        config::ConfigStore::with_paths(tmp.path().join("config"), tmp.path().join("downloads"));
    store.set("MCP", "mcp_enabled", json!(false));

    let manager = Arc::new(
        tasks::TaskManager::open(
            tmp.path().join("task.db"),
            Arc::new(config::ConfigStore::with_paths(
                tmp.path().join("config"),
                tmp.path().join("downloads"),
            )),
        )
        .unwrap(),
    );

    let handle = mcp::start(Arc::new(parking_lot::Mutex::new(store)), manager)
        .await
        .unwrap();
    assert!(handle.is_none());
}

#[tokio::test]
async fn start_generates_and_persists_token_then_serves() {
    let tmp = tempfile::tempdir().unwrap();
    let shared = Arc::new(parking_lot::Mutex::new(config::ConfigStore::with_paths(
        tmp.path().join("config"),
        tmp.path().join("downloads"),
    )));
    shared.lock().set("MCP", "mcp_enabled", json!(true));
    shared.lock().set("MCP", "mcp_port", json!(0)); // ephemeral port

    let manager = Arc::new(
        tasks::TaskManager::open(
            tmp.path().join("task.db"),
            Arc::new(config::ConfigStore::with_paths(
                tmp.path().join("config"),
                tmp.path().join("downloads"),
            )),
        )
        .unwrap(),
    );

    let handle = mcp::start(shared.clone(), manager).await.unwrap().unwrap();
    let token = handle.token().to_owned();
    // 32 bytes → 43 URL-safe base64 chars, generated not configured.
    assert_eq!(token.len(), 43);
    // Persisted for the next run.
    assert_eq!(shared.lock().get("MCP", "mcp_token"), Some(json!(token)));

    let _ = rustls::crypto::ring::default_provider().install_default();
    let body = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/list",
        "params": {"_meta": {"io.modelcontextprotocol/protocolVersion": mcp::protocol::MODERN_VERSION}}
    });
    let response = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{}/mcp", handle.port()))
        .bearer_auth(&token)
        .header("Mcp-Method", "tools/list")
        .header("MCP-Protocol-Version", mcp::protocol::MODERN_VERSION)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let payload: serde_json::Value = response.json().await.unwrap();
    let names: Vec<&str> = payload["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"parse_url"), "names: {names:?}");
    assert!(names.contains(&"create_download"), "names: {names:?}");
    assert!(names.contains(&"list_tasks"), "names: {names:?}");

    // Wrong token is rejected.
    let response = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{}/mcp", handle.port()))
        .bearer_auth("wrong-token")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);

    handle.stop();
}
