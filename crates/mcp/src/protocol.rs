//! JSON-RPC method dispatch (`src/util/mcp/protocol.py`).
//!
//! Protocol shape only — no HTTP. The transport layer ([`crate::server`])
//! does auth, origin and header-consistency checks and hands the parsed
//! message here.

use serde_json::{json, Map, Value};

/// The protocol revision this server implements.
pub const MODERN_VERSION: &str = "2026-07-28";
/// Legacy revisions the dual-era endpoint still accepts.
pub const LEGACY_VERSIONS: [&str; 3] = ["2025-11-25", "2025-06-18", "2025-03-26"];

/// Every revision the server will negotiate.
pub fn supported_versions() -> Vec<&'static str> {
    let mut all = vec![MODERN_VERSION];
    all.extend(LEGACY_VERSIONS);
    all
}

/// `params._meta` key carrying the per-request protocol version.
pub const PROTOCOL_VERSION_KEY: &str = "io.modelcontextprotocol/protocolVersion";
/// `params._meta`/`_meta` key carrying server identity in discover.
pub const SERVER_INFO_KEY: &str = "io.modelcontextprotocol/serverInfo";

pub const SERVER_INSTRUCTIONS: &str = "Controls the Bili23 Downloader desktop application. Parse a Bilibili URL first, then inspect the parsed episodes and create download tasks from them. Parsing replaces whatever is currently shown in the application's parse list, so it is refused while the user has items selected there.";

pub const TOOLS_CACHE_TTL_MS: i64 = 60_000;
pub const DISCOVER_CACHE_TTL_MS: i64 = 300_000;
pub const CACHE_SCOPE: &str = "public";

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;
pub const HEADER_MISMATCH: i64 = -32020;
pub const UNSUPPORTED_PROTOCOL_VERSION: i64 = -32022;

/// One tool entry of `tools/list` (name/title/description/inputSchema,
/// fixed order).
#[derive(Clone)]
pub struct ToolSchema {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

/// A tool result (`text_result`/`error_result`).
#[derive(Debug)]
pub struct ToolOutput {
    /// Text shown to the model first; the pretty JSON follows as a second
    /// content block when structured data is present.
    pub text: String,
    pub structured: Option<Value>,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            structured: None,
            is_error: false,
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            structured: None,
            is_error: true,
        }
    }

    pub fn with_structured(text: impl Into<String>, structured: Value) -> Self {
        Self {
            text: text.into(),
            structured: Some(structured),
            is_error: false,
        }
    }
}

/// Render a tool output into the MCP result object (`text_result`).
pub fn tool_result_value(output: &ToolOutput) -> Value {
    if let Some(structured) = &output.structured {
        let json = serde_json::to_string_pretty(structured).unwrap_or_default();
        let mut content = Vec::new();
        if !output.text.is_empty() {
            content.push(json!({"type": "text", "text": output.text}));
        }
        content.push(json!({"type": "text", "text": json}));
        return json!({
            "content": content,
            "isError": output.is_error,
            "structuredContent": structured,
        });
    }
    json!({
        "content": [{"type": "text", "text": output.text}],
        "isError": output.is_error,
    })
}

/// The dispatch target: protocol layer calls the tool handler with the
/// raw `arguments` object.
pub type ToolHandler = Box<dyn Fn(&Map<String, Value>) -> ToolOutput + Send + Sync>;

/// One registered tool.
pub struct Tool {
    pub schema: ToolSchema,
    pub handler: ToolHandler,
}

pub fn make_error(request_id: Option<&Value>, code: i64, message: &str) -> Value {
    make_error_data(request_id, code, message, None)
}

pub fn make_error_data(
    request_id: Option<&Value>,
    code: i64,
    message: &str,
    data: Option<Value>,
) -> Value {
    let mut error = json!({"code": code, "message": message});
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({"jsonrpc": "2.0", "id": request_id, "error": error})
}

pub fn make_result(request_id: Option<&Value>, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": request_id, "result": result})
}

pub fn unsupported_version_error(request_id: Option<&Value>, requested: &str) -> Value {
    make_error_data(
        request_id,
        UNSUPPORTED_PROTOCOL_VERSION,
        "Unsupported protocol version",
        Some(json!({"supported": supported_versions(), "requested": requested})),
    )
}

/// The protocol version a request declares, if any (modern `_meta`, legacy
/// `initialize.params.protocolVersion`).
pub fn get_request_version(message: &Map<String, Value>) -> Option<String> {
    let params = message.get("params")?.as_object()?;
    let meta = params.get("_meta").and_then(Value::as_object);
    if let Some(version) = meta
        .and_then(|m| m.get(PROTOCOL_VERSION_KEY))
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
    {
        return Some(version.to_owned());
    }
    params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

/// Modern requests carry their version in `params._meta`.
pub fn is_modern_request(message: &Map<String, Value>) -> bool {
    message
        .get("params")
        .and_then(Value::as_object)
        .and_then(|p| p.get("_meta"))
        .and_then(Value::as_object)
        .and_then(|m| m.get(PROTOCOL_VERSION_KEY))
        .and_then(Value::as_str)
        .is_some_and(|v| !v.is_empty())
}

/// `{"name": "Bili23 Downloader", "version": ...}`.
pub fn server_info() -> Value {
    json!({"name": "Bili23 Downloader", "version": env!("CARGO_PKG_VERSION")})
}

/// Outcome of handling one message: HTTP status + optional response body
/// (`None` = notification, 202 with no body).
pub struct Handled {
    pub status: u16,
    pub response: Option<Value>,
}

impl Handled {
    fn ok(response: Value) -> Self {
        Self {
            status: 200,
            response: Some(response),
        }
    }
}

/// JSON-RPC method dispatch over the registered tools.
pub struct Dispatcher {
    pub tools: Vec<Tool>,
}

impl Dispatcher {
    pub fn new(tools: Vec<Tool>) -> Self {
        Self { tools }
    }

    fn find(&self, name: &str) -> Option<&Tool> {
        self.tools.iter().find(|t| t.schema.name == name)
    }

    /// Handle one parsed JSON-RPC message.
    pub fn handle(&self, message: &Map<String, Value>) -> Handled {
        if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Handled {
                status: 400,
                response: Some(make_error(
                    None,
                    INVALID_REQUEST,
                    "Invalid JSON-RPC 2.0 message",
                )),
            };
        }
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return Handled {
                status: 400,
                response: Some(make_error(
                    message.get("id"),
                    INVALID_REQUEST,
                    "Missing method",
                )),
            };
        };
        let request_id = message.get("id");
        // No id = notification: accepted with 202 and no body.
        let is_notification = !message.contains_key("id");
        let version = get_request_version(message);

        if let Some(version) = &version {
            if !supported_versions().contains(&version.as_str()) {
                if is_notification {
                    return Handled {
                        status: 400,
                        response: None,
                    };
                }
                return Handled {
                    status: 400,
                    response: Some(unsupported_version_error(request_id, version)),
                };
            }
        }
        let modern = is_modern_request(message);

        if is_notification {
            return Handled {
                status: 202,
                response: None,
            };
        }

        let params = message.get("params").and_then(Value::as_object);
        match method {
            "server/discover" => Handled::ok(make_result(request_id, self.discover())),
            "initialize" => {
                let requested = params
                    .and_then(|p| p.get("protocolVersion"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let negotiated = if supported_versions().contains(&requested) {
                    requested
                } else {
                    LEGACY_VERSIONS[0]
                };
                Handled::ok(make_result(
                    request_id,
                    json!({
                        "protocolVersion": negotiated,
                        "capabilities": {"tools": {"listChanged": false}},
                        "serverInfo": server_info(),
                        "instructions": SERVER_INSTRUCTIONS,
                    }),
                ))
            }
            "ping" => Handled::ok(make_result(request_id, json!({}))),
            "tools/list" => {
                let tools: Vec<Value> = self
                    .tools
                    .iter()
                    .map(|t| {
                        json!({
                            "name": t.schema.name,
                            "title": t.schema.title,
                            "description": t.schema.description,
                            "inputSchema": t.schema.input_schema,
                        })
                    })
                    .collect();
                let mut result = json!({"tools": tools});
                // Cache hints belong to the modern revision only.
                if modern {
                    result["resultType"] = json!("complete");
                    result["ttlMs"] = json!(TOOLS_CACHE_TTL_MS);
                    result["cacheScope"] = json!(CACHE_SCOPE);
                }
                Handled::ok(make_result(request_id, result))
            }
            "tools/call" => self.call_tool(params, request_id, modern),
            // Spec: unimplemented methods are HTTP 404 + -32601.
            _ => Handled {
                status: 404,
                response: Some(make_error(
                    request_id,
                    METHOD_NOT_FOUND,
                    &format!("Method not found: {method}"),
                )),
            },
        }
    }

    fn discover(&self) -> Value {
        json!({
            "resultType": "complete",
            "supportedVersions": supported_versions(),
            "capabilities": {"tools": {"listChanged": false}},
            "instructions": SERVER_INSTRUCTIONS,
            "ttlMs": DISCOVER_CACHE_TTL_MS,
            "cacheScope": CACHE_SCOPE,
            "_meta": {SERVER_INFO_KEY: server_info()},
        })
    }

    fn call_tool(
        &self,
        params: Option<&Map<String, Value>>,
        request_id: Option<&Value>,
        modern: bool,
    ) -> Handled {
        let params = params.cloned().unwrap_or_default();
        let name = params.get("name").and_then(Value::as_str);
        let Some(name) = name else {
            return Handled::ok(make_error(request_id, INVALID_PARAMS, "Missing tool name"));
        };
        let Some(tool) = self.find(name) else {
            return Handled::ok(make_error(
                request_id,
                INVALID_PARAMS,
                &format!("Unknown tool: {name}"),
            ));
        };
        let arguments = match params.get("arguments") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(map)) => map.clone(),
            Some(_) => {
                return Handled::ok(make_error(
                    request_id,
                    INVALID_PARAMS,
                    "Tool arguments must be an object",
                ))
            }
        };
        // Tool-level failures are isError results, not JSON-RPC errors:
        // self-correctable business errors go back to the model.
        let mut result = tool_result_value(&(tool.handler)(&arguments));
        if modern {
            result["resultType"] = json!("complete");
        }
        Handled::ok(make_result(request_id, result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn echo_tool() -> Tool {
        Tool {
            schema: ToolSchema {
                name: "echo",
                title: "Echo",
                description: "echo",
                input_schema: json!({"type": "object"}),
            },
            handler: Box::new(|args| ToolOutput::with_structured("echoed", json!(args.clone()))),
        }
    }

    fn message(body: Value) -> Map<String, Value> {
        body.as_object().unwrap().clone()
    }

    #[test]
    fn initialize_echoes_supported_version() {
        let d = Dispatcher::new(vec![echo_tool()]);
        let m = message(json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18"}
        }));
        let h = d.handle(&m);
        assert_eq!(h.status, 200);
        let result = h.response.unwrap()["result"].clone();
        assert_eq!(result["protocolVersion"], json!("2025-06-18"));
        assert_eq!(
            result["capabilities"],
            json!({"tools": {"listChanged": false}})
        );
        assert_eq!(result["serverInfo"]["name"], json!("Bili23 Downloader"));
        assert_eq!(result["instructions"], json!(SERVER_INSTRUCTIONS));
    }

    #[test]
    fn initialize_falls_back_to_newest_legacy() {
        let d = Dispatcher::new(vec![echo_tool()]);
        let m = message(json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {}
        }));
        let h = d.handle(&m);
        assert_eq!(
            h.response.unwrap()["result"]["protocolVersion"],
            json!("2025-11-25")
        );
    }

    #[test]
    fn ping_returns_empty_result() {
        let d = Dispatcher::new(vec![echo_tool()]);
        let m = message(json!({"jsonrpc": "2.0", "id": 7, "method": "ping"}));
        let h = d.handle(&m);
        assert_eq!(h.status, 200);
        assert_eq!(h.response.unwrap()["result"], json!({}));
    }

    #[test]
    fn tools_list_modern_carries_cache_hints_legacy_does_not() {
        let d = Dispatcher::new(vec![echo_tool()]);
        let modern = d.handle(&message(json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/list",
            "params": {"_meta": {PROTOCOL_VERSION_KEY: MODERN_VERSION}}
        })));
        let result = modern.response.unwrap()["result"].clone();
        assert_eq!(result["resultType"], json!("complete"));
        assert_eq!(result["ttlMs"], json!(TOOLS_CACHE_TTL_MS));
        assert_eq!(result["cacheScope"], json!("public"));
        assert_eq!(result["tools"][0]["name"], json!("echo"));
        assert_eq!(result["tools"][0]["inputSchema"], json!({"type": "object"}));

        let legacy = d.handle(&message(
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        ));
        let result = legacy.response.unwrap()["result"].clone();
        assert!(result.get("resultType").is_none());
        assert!(result.get("ttlMs").is_none());
    }

    #[test]
    fn tool_failure_is_iserror_not_jsonrpc_error() {
        let d = Dispatcher::new(vec![Tool {
            schema: ToolSchema {
                name: "boom",
                title: "Boom",
                description: "boom",
                input_schema: json!({"type": "object"}),
            },
            handler: Box::new(|_| ToolOutput::error("business failure")),
        }]);
        let m = message(json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": "boom"}
        }));
        let h = d.handle(&m);
        assert_eq!(h.status, 200);
        let result = h.response.unwrap()["result"].clone();
        assert_eq!(result["isError"], json!(true));
        assert_eq!(result["content"][0]["text"], json!("business failure"));
    }

    #[test]
    fn invalid_params_for_bad_tool_calls() {
        let d = Dispatcher::new(vec![echo_tool()]);
        let missing = d.handle(&message(json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {}
        })));
        assert_eq!(
            missing.response.unwrap()["error"]["message"],
            json!("Missing tool name")
        );

        let unknown = d.handle(&message(json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "nope"}
        })));
        assert_eq!(
            unknown.response.unwrap()["error"]["message"],
            json!("Unknown tool: nope")
        );

        let bad_args = d.handle(&message(json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "echo", "arguments": [1]}
        })));
        assert_eq!(
            bad_args.response.unwrap()["error"]["message"],
            json!("Tool arguments must be an object")
        );
    }

    #[test]
    fn notification_returns_202_without_body() {
        let d = Dispatcher::new(vec![echo_tool()]);
        let h = d.handle(&message(json!({
            "jsonrpc": "2.0", "method": "notifications/initialized"
        })));
        assert_eq!(h.status, 202);
        assert!(h.response.is_none());
    }

    #[test]
    fn unknown_method_is_404_with_method_not_found() {
        let d = Dispatcher::new(vec![echo_tool()]);
        let h = d.handle(&message(
            json!({"jsonrpc": "2.0", "id": 3, "method": "bogus/x"}),
        ));
        assert_eq!(h.status, 404);
        assert_eq!(
            h.response.unwrap()["error"]["message"],
            json!("Method not found: bogus/x")
        );
    }

    #[test]
    fn non_2_0_message_is_400() {
        let d = Dispatcher::new(vec![echo_tool()]);
        let h = d.handle(&message(
            json!({"jsonrpc": "1.0", "id": 1, "method": "ping"}),
        ));
        assert_eq!(h.status, 400);
        assert_eq!(
            h.response.unwrap()["error"]["message"],
            json!("Invalid JSON-RPC 2.0 message")
        );
        let h = d.handle(&message(json!({"id": 1})));
        assert_eq!(h.status, 400);
        assert_eq!(
            h.response.unwrap()["error"]["message"],
            json!("Invalid JSON-RPC 2.0 message")
        );
    }

    #[test]
    fn unsupported_version_is_400_with_negotiation_data() {
        let d = Dispatcher::new(vec![echo_tool()]);
        let m = message(json!({
            "jsonrpc": "2.0", "id": 5, "method": "tools/list",
            "params": {"_meta": {PROTOCOL_VERSION_KEY: "2020-01-01"}}
        }));
        let h = d.handle(&m);
        assert_eq!(h.status, 400);
        let error = h.response.unwrap()["error"].clone();
        assert_eq!(error["code"], json!(UNSUPPORTED_PROTOCOL_VERSION));
        assert_eq!(error["data"]["requested"], json!("2020-01-01"));
    }

    #[test]
    fn modern_tool_call_gets_result_type() {
        let d = Dispatcher::new(vec![echo_tool()]);
        let m = message(json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": {"name": "echo", "_meta": {PROTOCOL_VERSION_KEY: MODERN_VERSION}}
        }));
        let result = d.handle(&m).response.unwrap()["result"].clone();
        assert_eq!(result["resultType"], json!("complete"));
        assert_eq!(result["isError"], json!(false));
        assert_eq!(result["content"][0]["text"], json!("echoed"));
        assert!(result.get("structuredContent").is_some());
    }

    #[test]
    fn discover_shape() {
        let d = Dispatcher::new(vec![echo_tool()]);
        let h = d.handle(&message(
            json!({"jsonrpc": "2.0", "id": 1, "method": "server/discover"}),
        ));
        let result = h.response.unwrap()["result"].clone();
        assert_eq!(result["resultType"], json!("complete"));
        assert_eq!(result["ttlMs"], json!(DISCOVER_CACHE_TTL_MS));
        assert_eq!(
            result["_meta"][SERVER_INFO_KEY]["name"],
            json!("Bili23 Downloader")
        );
    }
}

// Header-consistency validation (`MCPRequestHandler._check_headers`):
// pure over the parsed body + header values, shared with tests.
pub struct HeaderCheck<'a> {
    pub protocol_version: &'a str,
    pub method: &'a str,
    pub name: &'a str,
}

/// Validate that mirrored HTTP headers agree with the request body.
/// Returns the error response when they do not.
pub fn check_headers(message: &Map<String, Value>, headers: &HeaderCheck) -> Option<Value> {
    let modern = is_modern_request(message);
    let request_id = message.get("id");

    let body_version = message
        .get("params")
        .and_then(Value::as_object)
        .and_then(|p| p.get("_meta"))
        .and_then(Value::as_object)
        .and_then(|m| m.get(PROTOCOL_VERSION_KEY))
        .and_then(Value::as_str)
        .unwrap_or("");

    if modern {
        if headers.protocol_version.is_empty() {
            return Some(make_error(
                request_id,
                HEADER_MISMATCH,
                "Missing MCP-Protocol-Version header",
            ));
        }
        if headers.protocol_version != body_version {
            return Some(make_error(
                request_id,
                HEADER_MISMATCH,
                &format!(
                    "MCP-Protocol-Version header '{}' does not match body value '{}'",
                    headers.protocol_version, body_version
                ),
            ));
        }
    }
    if !headers.protocol_version.is_empty()
        && !supported_versions().contains(&headers.protocol_version)
    {
        return Some(unsupported_version_error(
            request_id,
            headers.protocol_version,
        ));
    }
    let body_method = message.get("method").and_then(Value::as_str).unwrap_or("");
    if modern && headers.method.is_empty() {
        return Some(make_error(
            request_id,
            HEADER_MISMATCH,
            "Missing Mcp-Method header",
        ));
    }
    if !headers.method.is_empty() && headers.method != body_method {
        return Some(make_error(
            request_id,
            HEADER_MISMATCH,
            &format!(
                "Mcp-Method header '{}' does not match body method '{}'",
                headers.method, body_method
            ),
        ));
    }
    if body_method == "tools/call" {
        let body_name = message
            .get("params")
            .and_then(Value::as_object)
            .and_then(|p| p.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if modern && headers.name.is_empty() {
            return Some(make_error(
                request_id,
                HEADER_MISMATCH,
                "Missing Mcp-Name header",
            ));
        }
        if !headers.name.is_empty() && decode_header_value(headers.name) != body_name {
            return Some(make_error(
                request_id,
                HEADER_MISMATCH,
                &format!("Mcp-Name header does not match body value '{body_name}'"),
            ));
        }
    }
    None
}

pub const BASE64_PREFIX: &str = "=?base64?";
pub const BASE64_SUFFIX: &str = "?=";

/// Decode a `=?base64?<payload>?=` sentinel header value; undecodable
/// payloads pass through untouched so the consistency check rejects.
pub fn decode_header_value(value: &str) -> String {
    let Some(payload) = value
        .strip_prefix(BASE64_PREFIX)
        .and_then(|rest| rest.strip_suffix(BASE64_SUFFIX))
    else {
        return value.to_owned();
    };
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(payload)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_else(|| value.to_owned())
}

#[cfg(test)]
mod header_tests {
    use super::*;

    #[test]
    fn modern_request_requires_matching_headers() {
        let body = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {
                "name": "echo",
                "_meta": {PROTOCOL_VERSION_KEY: MODERN_VERSION}
            }
        });
        let m = body.as_object().unwrap().clone();
        let ok = HeaderCheck {
            protocol_version: MODERN_VERSION,
            method: "tools/call",
            name: "echo",
        };
        assert!(check_headers(&m, &ok).is_none());

        let no_version = HeaderCheck {
            protocol_version: "",
            method: "tools/call",
            name: "echo",
        };
        let err = check_headers(&m, &no_version).unwrap();
        assert_eq!(
            err["error"]["message"],
            json!("Missing MCP-Protocol-Version header")
        );

        let mismatch = HeaderCheck {
            protocol_version: "2025-03-26",
            method: "tools/call",
            name: "echo",
        };
        let err = check_headers(&m, &mismatch).unwrap();
        assert_eq!(
            err["error"]["message"],
            json!(format!(
                "MCP-Protocol-Version header '2025-03-26' does not match body value '{MODERN_VERSION}'"
            ))
        );
    }

    #[test]
    fn unsupported_version_header_is_rejected() {
        let m = json!({"jsonrpc": "2.0", "id": 1, "method": "ping"})
            .as_object()
            .unwrap()
            .clone();
        let bad = HeaderCheck {
            protocol_version: "1999-01-01",
            method: "ping",
            name: "",
        };
        let err = check_headers(&m, &bad).unwrap();
        assert_eq!(err["error"]["code"], json!(UNSUPPORTED_PROTOCOL_VERSION));
    }

    #[test]
    fn sentinel_encoded_name_must_decode_to_body_value() {
        use base64::Engine;
        let payload = base64::engine::general_purpose::STANDARD.encode("中文工具");
        let sentinel = format!("{BASE64_PREFIX}{payload}{BASE64_SUFFIX}");
        let body = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "中文工具"}
        });
        let m = body.as_object().unwrap().clone();
        let ok = HeaderCheck {
            protocol_version: "",
            method: "tools/call",
            name: &sentinel,
        };
        assert!(check_headers(&m, &ok).is_none());

        let wrong = HeaderCheck {
            protocol_version: "",
            method: "tools/call",
            name: "=?base64?bm90?=",
        };
        assert!(check_headers(&m, &wrong).is_some());
    }

    #[test]
    fn legacy_headers_are_optional_but_must_agree() {
        let m = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})
            .as_object()
            .unwrap()
            .clone();
        let none = HeaderCheck {
            protocol_version: "",
            method: "",
            name: "",
        };
        assert!(check_headers(&m, &none).is_none());

        let wrong_method = HeaderCheck {
            protocol_version: "",
            method: "tools/call",
            name: "",
        };
        let err = check_headers(&m, &wrong_method).unwrap();
        assert!(err["error"]["message"]
            .as_str()
            .unwrap()
            .contains("does not match body method"));
    }
}
