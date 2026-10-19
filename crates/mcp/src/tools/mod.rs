//! Tool layer: schemas, dispatch to the per-family handlers, and the
//! shared preview (media-info) flow.

pub mod download;
pub mod parse;
pub mod task;

use std::future::Future;
use std::sync::Arc;

use parsers::preview::{Preview, Priorities};
use parsers::tree::TreeItem;
use serde_json::{Map, Value};

use crate::protocol::Tool;
use crate::state::{McpState, PreviewSummary};

/// Python `_do_parse` timeouts.
pub const PARSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);
pub const MEDIA_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Anonymous-session client (`list_parsers`' helper shape). The MCP server
/// has no logged-in cookie jar of its own; login-dependent content fails
/// at the media-info stage exactly as it does in Python.
pub fn http_client(state: &McpState) -> anyhow::Result<network::HttpClient> {
    network::HttpClient::new(network::ClientOptions {
        user_agent: state
            .config
            .lock()
            .get("Advanced", "user_agent")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| config::DEFAULT_USER_AGENT.to_owned()),
        proxy: network::ProxyConfig::none(),
        cookies_from_persistence: Vec::new(),
        signer: None,
    })
    .map_err(|e| anyhow::anyhow!(e.to_string()))
}

/// Login state read from the config-persisted cookie group.
pub fn auth_state(state: &McpState) -> auth::AuthState {
    let path = state.config.lock().path().to_path_buf();
    auth::AuthState::from_config_file(&path).unwrap_or_default()
}

/// Previewer flow (`Previewer.on_init` + `start_next_candidate`): try the
/// link target first, then up to two badgeless backups; first success
/// wins. All-failed records the error.
pub async fn preview_flow(state: &McpState) -> Option<PreviewSummary> {
    let client = http_client(state).ok()?;
    let apis = parsers::playurl::PlayurlApis::with_base(&state.api_base);

    // Candidates snapshotted under the view lock; at most three clones.
    let candidates: Vec<TreeItem> = {
        let view = state.view.lock();
        let items = view.get_all_items();
        if items.is_empty() {
            return None;
        }
        let primary = view
            .current_episode_bvid()
            .and_then(|bvid| items.iter().position(|item| item.bvid == bvid))
            .unwrap_or(0);
        // Badged items (paid / members-only) usually fail media info: try
        // them last.
        let mut rest: Vec<usize> = (0..items.len()).filter(|i| *i != primary).collect();
        rest.sort_by_key(|i| !items[*i].badge.is_empty());
        let mut order = vec![primary];
        order.extend(rest.into_iter().take(2));
        order.into_iter().map(|i| items[i].clone()).collect()
    };

    let mut summary: Option<PreviewSummary> = None;
    let mut from_fallback = false;
    for item in &candidates {
        if !parsers::preview::check_need_parse(item.attribute) {
            summary = Some(PreviewSummary::default());
            break;
        }
        let mut preview = Preview::new(Priorities::default());
        let outcome = match parsers::playurl::fetch_media_info(&client, &apis, item, None).await {
            Ok(fetched) => {
                preview
                    .start(item.attribute, &item.bvid, item.cid, async { Ok(fetched) })
                    .await
            }
            Err(error) => Err(error),
        };
        match outcome {
            Ok(()) => {
                let info = &preview.info;
                summary = Some(PreviewSummary {
                    video_quality: info.video_quality_choices.clone(),
                    audio_quality: info.audio_quality_choices.clone(),
                    video_codec: info.video_codec_choices.clone(),
                    title: info.episode_title.clone(),
                    number: info.episode_number.clone(),
                    from_fallback,
                    error: None,
                });
                break;
            }
            Err(error) => {
                from_fallback = true;
                summary = Some(PreviewSummary {
                    error: Some(error.to_string()),
                    from_fallback: true,
                    ..Default::default()
                });
            }
        }
    }
    *state.preview.lock() = summary.clone();
    summary
}

/// All nine tools in registry order (Python `tools/__init__.py`).
pub fn register_all(state: Arc<McpState>) -> Vec<Tool> {
    let mut tools = Vec::new();

    for schema in parse::parse_schemas() {
        let state = state.clone();
        let name = schema.name;
        tools.push(Tool {
            schema,
            handler: Box::new(move |args| match name {
                "parse_url" => tokio_block(parse::tool_parse_url(state.clone(), args)),
                _ => parse::tool_get_episodes(state.clone(), args),
            }),
        });
    }

    for schema in task::task_schemas() {
        let state = state.clone();
        let name = schema.name;
        tools.push(Tool {
            schema,
            handler: Box::new(move |args| task::dispatch(state.clone(), name, args)),
        });
    }

    for schema in download::download_schemas() {
        let state = state.clone();
        let name = schema.name;
        tools.push(Tool {
            schema,
            handler: Box::new(move |args| download::dispatch(state.clone(), name, args)),
        });
    }

    tools
}

/// `parse_url` is the one async tool; run it on the tokio runtime the
/// server owns (`tokio::task::block_in_place` needs multi-thread).
pub fn tokio_block<F: Future>(future: F) -> F::Output {
    tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(future))
}

/// Shared argument reader: `arguments.get(key)` as a string.
pub fn str_arg<'a>(args: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}
