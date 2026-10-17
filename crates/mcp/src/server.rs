//! HTTP transport (`src/util/mcp/server.py`): axum loopback endpoint with
//! Bearer auth, origin pinning and mirrored-header validation.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::post;
use axum::Router;
use serde_json::Value;

use crate::protocol::{self, Dispatcher};

pub const ENDPOINT_PATH: &str = "/mcp";
pub const MAX_BODY_SIZE: usize = 1024 * 1024;

const REALM: &str = r#"Bearer realm="bilidown MCP""#;

/// Everything the endpoint needs; built once by [`crate::start`].
pub struct McpServer {
    pub dispatcher: Arc<Dispatcher>,
    pub token: String,
}

fn plain_response(status: StatusCode, text: &str) -> Response {
    Response::builder()
        .status(status)
        .header(header::SERVER, "bilidown-mcp")
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::CONNECTION, "close")
        .body(Body::from(text.to_owned()))
        .unwrap()
}

fn json_response(status: StatusCode, payload: Option<Value>) -> Response {
    let mut builder = Response::builder()
        .status(status)
        .header(header::SERVER, "bilidown-mcp")
        .header(header::CONNECTION, "close");
    let body = match payload {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(serde_json::to_vec(&value).unwrap_or_default())
        }
        None => Body::empty(),
    };
    builder.body(body).unwrap()
}

fn error_response(status: StatusCode, payload: Value) -> Response {
    json_response(status, Some(payload))
}

/// Build the axum app (exported for `tower::ServiceExt::oneshot` tests).
pub fn app(server: Arc<McpServer>) -> Router {
    Router::new()
        .route("/", post(handle))
        .route("/mcp", post(handle))
        .fallback(fallback)
        .with_state(server)
}

async fn fallback(request: Request) -> Response {
    match *request.method() {
        axum::http::Method::GET | axum::http::Method::DELETE => {
            plain_response(StatusCode::METHOD_NOT_ALLOWED, "Method Not Allowed")
        }
        _ => plain_response(StatusCode::NOT_FOUND, "Not Found"),
    }
}

async fn handle(
    axum::extract::State(server): axum::extract::State<Arc<McpServer>>,
    headers: HeaderMap,
    request: Request,
) -> Response {
    // 1. Origin: same-origin browser fetches only. Non-browser clients
    //    (no Origin) pass; "null" and cross-site origins are refused.
    if let Some(origin) = headers.get(header::ORIGIN) {
        let origin = origin.to_str().unwrap_or_default();
        if origin == "null" || !is_local_origin(origin) {
            return plain_response(StatusCode::FORBIDDEN, "Forbidden");
        }
    }

    // 2. Body: bounded read before anything else touches it.
    let body = match axum::body::to_bytes(request.into_body(), MAX_BODY_SIZE + 1).await {
        Ok(bytes) => bytes,
        Err(_) => return plain_response(StatusCode::BAD_REQUEST, "Bad Request"),
    };

    // 3. Auth: constant-time compare of the bearer token.
    let expected = &server.token;
    let provided = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let authorized = match provided {
        Some(provided) => {
            !expected.is_empty()
                && subtle::ConstantTimeEq::ct_eq(provided.as_bytes(), expected.as_bytes())
                    .unwrap_u8()
                    == 1
        }
        None => false,
    };
    if !authorized {
        return Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(header::SERVER, "bilidown-mcp")
            .header(header::WWW_AUTHENTICATE, REALM)
            .header(header::CONNECTION, "close")
            .body(Body::empty())
            .unwrap();
    }

    // 4. Size gate after auth mirrors Python's ordering (auth precedes
    //    the size check there too).
    if body.len() > MAX_BODY_SIZE {
        return plain_response(StatusCode::PAYLOAD_TOO_LARGE, "Payload Too Large");
    }
    if body.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            protocol::make_error(None, protocol::INVALID_REQUEST, "Empty request body"),
        );
    }

    let message: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                protocol::make_error(None, protocol::PARSE_ERROR, "Parse error"),
            )
        }
    };
    let Some(map) = message.as_object() else {
        return error_response(
            StatusCode::BAD_REQUEST,
            protocol::make_error(
                None,
                protocol::INVALID_REQUEST,
                "Invalid JSON-RPC 2.0 message",
            ),
        );
    };

    // 5. Mirrored headers vs body.
    let header_str = |name: &str| -> String {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    };
    let check = protocol::HeaderCheck {
        protocol_version: &header_str("MCP-Protocol-Version"),
        method: &header_str("Mcp-Method"),
        name: &header_str("Mcp-Name"),
    };
    if let Some(error) = protocol::check_headers(map, &check) {
        return error_response(StatusCode::BAD_REQUEST, error);
    }

    let handled = server.dispatcher.handle(map);
    let status = StatusCode::from_u16(handled.status).unwrap_or(StatusCode::OK);
    json_response(status, handled.response)
}

fn is_local_origin(origin: &str) -> bool {
    let (scheme_host, port) = match origin.rsplit_once(':') {
        // IPv6 literal keeps its brackets; the split is the port.
        Some((head, tail)) if tail.chars().all(|c| c.is_ascii_digit()) && !tail.is_empty() => {
            (head, Some(tail))
        }
        _ => (origin, None),
    };
    let host_ok = matches!(
        scheme_host,
        "http://localhost" | "http://127.0.0.1" | "http://[::1]"
    );
    host_ok && port.is_some_and(|p| p.parse::<u16>().is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Tool, ToolOutput, ToolSchema};
    use axum::body::Body as AxumBody;
    use axum::http::Request as HttpRequest;
    use serde_json::json;
    use tower::ServiceExt;

    fn server(token: &str) -> Arc<McpServer> {
        let dispatcher = Dispatcher::new(vec![Tool {
            schema: ToolSchema {
                name: "echo",
                title: "Echo",
                description: "echo",
                input_schema: json!({"type": "object"}),
            },
            handler: Box::new(|_| ToolOutput::text("ok")),
        }]);
        Arc::new(McpServer {
            dispatcher: Arc::new(dispatcher),
            token: token.to_owned(),
        })
    }

    async fn post_json(
        app: &Router,
        token: Option<&str>,
        body: Value,
        extra: &[(&str, &str)],
    ) -> (StatusCode, HeaderMap, Option<Value>) {
        let mut builder =
            HttpRequest::post("/mcp").header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        for (name, value) in extra {
            builder = builder.header(*name, *value);
        }
        let response = app
            .clone()
            .oneshot(builder.body(AxumBody::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec();
        let value = if bytes.is_empty() {
            None
        } else {
            serde_json::from_slice(&bytes).ok()
        };
        (status, headers, value)
    }

    fn ping() -> Value {
        json!({"jsonrpc": "2.0", "id": 1, "method": "ping"})
    }

    #[tokio::test]
    async fn bearer_auth_gates_the_endpoint() {
        let app = app(server("secret"));
        let (status, headers, _) = post_json(&app, Some("wrong"), ping(), &[]).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            headers.get(header::WWW_AUTHENTICATE).unwrap(),
            r#"Bearer realm="bilidown MCP""#
        );

        let (status, _, _) = post_json(&app, None, ping(), &[]).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, _, body) = post_json(&app, Some("secret"), ping(), &[]).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.unwrap()["result"], json!({}));
    }

    #[tokio::test]
    async fn empty_expected_token_rejects_everything() {
        let app = app(server(""));
        let (status, _, _) = post_json(&app, Some(""), ping(), &[]).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn origin_matrix() {
        let app = app(server("secret"));
        for origin in [
            "http://localhost:8080",
            "http://127.0.0.1:5173",
            "http://[::1]:9000",
        ] {
            let (status, _, _) = post_json(
                &app,
                Some("secret"),
                ping(),
                &[(header::ORIGIN.as_str(), origin)],
            )
            .await;
            assert_eq!(status, StatusCode::OK, "origin {origin} should pass");
        }
        for origin in [
            "null",
            "https://evil.example",
            "http://evil.example:80",
            "http://[::2]:1",
        ] {
            let (status, _, _) = post_json(
                &app,
                Some("secret"),
                ping(),
                &[(header::ORIGIN.as_str(), origin)],
            )
            .await;
            assert_eq!(
                status,
                StatusCode::FORBIDDEN,
                "origin {origin} should be rejected"
            );
        }
    }

    #[tokio::test]
    async fn path_gate_and_methods() {
        let app = app(server("secret"));
        // Root path also serves the endpoint (Python: "" or "/mcp").
        let request = HttpRequest::post("/")
            .header(header::AUTHORIZATION, "Bearer secret")
            .body(AxumBody::from(ping().to_string()))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let request = HttpRequest::get("/mcp")
            .header(header::AUTHORIZATION, "Bearer secret")
            .body(AxumBody::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);

        let request = HttpRequest::delete("/")
            .header(header::AUTHORIZATION, "Bearer secret")
            .body(AxumBody::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);

        let request = HttpRequest::post("/other")
            .header(header::AUTHORIZATION, "Bearer secret")
            .body(AxumBody::from(ping().to_string()))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn empty_body_and_bad_json() {
        let app = app(server("secret"));
        let request = HttpRequest::post("/mcp")
            .header(header::AUTHORIZATION, "Bearer secret")
            .body(AxumBody::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let request = HttpRequest::post("/mcp")
            .header(header::AUTHORIZATION, "Bearer secret")
            .body(AxumBody::from("not json"))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["error"]["code"], json!(-32700));

        let request = HttpRequest::post("/mcp")
            .header(header::AUTHORIZATION, "Bearer secret")
            .body(AxumBody::from("[1,2]"))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn oversized_body_is_rejected() {
        let app = app(server("secret"));
        let big = "x".repeat(MAX_BODY_SIZE + 1);
        let request = HttpRequest::post("/mcp")
            .header(header::AUTHORIZATION, "Bearer secret")
            .body(AxumBody::from(big))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn modern_request_needs_consistent_headers() {
        let app = app(server("secret"));
        let body = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {
                "name": "echo",
                "_meta": {"io.modelcontextprotocol/protocolVersion": protocol::MODERN_VERSION}
            }
        });
        let (status, _, value) = post_json(
            &app,
            Some("secret"),
            body.clone(),
            &[("Mcp-Method", "tools/call")],
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(value.unwrap()["error"]["code"], json!(-32020));

        let (status, _, value) = post_json(
            &app,
            Some("secret"),
            body,
            &[
                ("MCP-Protocol-Version", protocol::MODERN_VERSION),
                ("Mcp-Method", "tools/call"),
                ("Mcp-Name", "echo"),
            ],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value.unwrap()["result"]["content"][0]["text"], json!("ok"));
    }

    #[tokio::test]
    async fn notification_is_202_with_no_body() {
        let app = app(server("secret"));
        let body = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        let (status, headers, value) = post_json(&app, Some("secret"), body, &[]).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert!(value.is_none());
        assert_eq!(headers.get(header::CONNECTION).unwrap(), "close");
        assert_eq!(headers.get(header::SERVER).unwrap(), "bilidown-mcp");
    }
}
