//! Parse tools (`src/util/mcp/tools/parse.py`): parse_url + get_episodes
//! over the headless `ParseListView`.

use std::sync::Arc;

use serde_json::{json, Map, Value};

use parsers::episode::list_base::wrap_node;
use parsers::episode::video::USER_UPLOADS;
use parsers::tree::{Attribute, EpisodeData, TreeItem};
use parsers::view::ParseListView;
use urls::classify::{parser_type, ParserType};

use crate::protocol::{ToolOutput, ToolSchema};
use crate::state::{McpState, ParseSnapshot, PreviewSummary};
use crate::tools::{auth_state, http_client, preview_flow, PARSE_TIMEOUT};

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 500;

/// MCP category label per parser family (Python `get_category_name`).
pub fn category_name(parser: ParserType) -> &'static str {
    match parser {
        ParserType::Video => USER_UPLOADS,
        ParserType::Bangumi => "ANIME",
        ParserType::Cheese => "CHEESE",
        ParserType::Lesson => "LESSON",
        ParserType::List => parsers::list::COLLECTION_CATEGORY,
        ParserType::Favlist => parsers::list::FAVORITES_CATEGORY,
        ParserType::Space => parsers::list::PROFILE_CATEGORY,
        ParserType::Popular => parsers::list::WEEKLY_CATEGORY,
        ParserType::Festival => "festival",
        ParserType::B23 => "b23",
        ParserType::WatchLater => parsers::list::WATCH_LATER_CATEGORY,
        ParserType::History => parsers::list::HISTORY_CATEGORY,
        ParserType::Audio => "AUDIO",
    }
}

fn clamp_limit(value: Option<&Value>) -> usize {
    match value {
        Some(Value::Number(n)) if n.is_u64() || n.is_i64() => {
            n.as_i64().unwrap_or(DEFAULT_LIMIT as i64).clamp(1, MAX_LIMIT as i64) as usize
        }
        _ => DEFAULT_LIMIT,
    }
}

/// Position ids are 1-based indexes into `get_all_items()`.
fn episode_to_dict(id: &str, item: &TreeItem, is_link_target: bool, downloaded: bool) -> Value {
    let mut data = json!({
        "episode_id": id,
        "title": item.title,
        "number": item.number.to_json(),
        "duration": item.duration,
    });
    if is_link_target {
        data["is_link_target"] = json!(true);
    }
    if !item.badge.is_empty() {
        data["badge"] = json!(item.badge);
    }
    if !item.bvid.is_empty() {
        data["bvid"] = json!(item.bvid);
    }
    if item.attribute.contains(Attribute::NEED_PARSE_BIT) {
        data["needs_reparse"] = json!(true);
    }
    if downloaded {
        data["already_downloaded"] = json!(true);
    }
    data
}

/// Everything the parse list currently holds, keyed by position id.
struct Collected {
    total: usize,
    episodes: Vec<Value>,
    link_target_id: Option<String>,
    link_target_episode: Option<Value>,
}

fn collect_episodes(view: &ParseListView, limit: usize, downloaded: &std::collections::HashSet<String>) -> Collected {
    let items = view.get_all_items();
    let total = items.len();

    let link_target = view
        .current_episode_bvid()
        .and_then(|bvid| items.iter().position(|item| item.bvid == bvid))
        .map(|index| index + 1);

    let mut episodes = Vec::new();
    for (index, item) in items.iter().take(limit).enumerate() {
        let id = (index + 1).to_string();
        episodes.push(episode_to_dict(&id, item, link_target == Some(index + 1), downloaded.contains(&id)));
    }

    let link_target_episode = link_target
        .filter(|id| *id > limit)
        .map(|id| episode_to_dict(&id.to_string(), items[id - 1], true, downloaded.contains(&id.to_string())));

    Collected { total, episodes, link_target_id: link_target.map(|i| i.to_string()), link_target_episode }
}

fn link_target_note(collected: &Collected) -> String {
    let Some(id) = &collected.link_target_id else {
        return String::new();
    };
    let title = collected
        .link_target_episode
        .as_ref()
        .and_then(|e| e.get("title"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            collected
                .episodes
                .iter()
                .find(|e| e.get("episode_id").and_then(Value::as_str) == Some(id.as_str()))
                .and_then(|e| e.get("title"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
    match title {
        Some(title) if !title.is_empty() => format!(" The link points to episode_id {id} ({title})."),
        _ => format!(" The link points to episode_id {id}."),
    }
}

fn fallback_note(source: &Value) -> String {
    if source.get("from_fallback").and_then(Value::as_bool) != Some(true) {
        return String::new();
    }
    let title = source.get("title").and_then(Value::as_str).unwrap_or_default();
    format!(
        " Note: the linked item did not provide media information, so the qualities in 'available' are those of another item ({title}) and may differ from what the item you download actually offers."
    )
}

/// Media-info block shared by both tools: options + source, or the error.
fn media_info_fields(preview: &Option<PreviewSummary>) -> (Value, Value) {
    let Some(preview) = preview else {
        return (json!({}), json!({}));
    };
    if preview.error.is_some() {
        return (json!({}), json!({}));
    }
    let mut options = Map::new();
    let to_names = |entries: &[(String, i64)]| -> Value {
        Value::Array(entries.iter().map(|(name, _)| json!(name)).collect())
    };
    if !preview.video_quality.is_empty() {
        options.insert("video_quality".into(), to_names(&preview.video_quality));
    }
    if !preview.audio_quality.is_empty() {
        options.insert("audio_quality".into(), to_names(&preview.audio_quality));
    }
    if !preview.video_codec.is_empty() {
        options.insert("video_codec".into(), to_names(&preview.video_codec));
    }
    let mut source = Map::new();
    if !preview.title.is_empty() {
        source.insert("title".into(), json!(preview.title));
        if !preview.number.is_empty() {
            source.insert("number".into(), json!(preview.number));
        }
        if preview.from_fallback {
            source.insert("from_fallback".into(), json!(true));
        }
    }
    (Value::Object(options), Value::Object(source))
}

pub fn media_info_error(preview: &Option<PreviewSummary>) -> Option<String> {
    let preview = preview.as_ref()?;
    if let Some(error) = &preview.error {
        return Some(error.clone());
    }
    // No preview at all after a parse = never retrieved.
    (preview.video_quality.is_empty() && preview.audio_quality.is_empty()).then(|| {
        "media information was not retrieved".to_owned()
    })
}

// ---------------------------------------------------------------------------
// parse_url
// ---------------------------------------------------------------------------

// The parse guard is held for the whole parse on purpose (one parse at a
// time); this tool runs under `block_in_place`, so no worker starves.
#[allow(clippy::await_holding_lock)]
pub async fn tool_parse_url(state: Arc<McpState>, arguments: &Map<String, Value>) -> ToolOutput {
    let url = arguments
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned();

    if url.is_empty() {
        return ToolOutput::error("The 'url' argument is required.");
    }

    let Some(parser) = parser_type(&url) else {
        return ToolOutput::error(format!(
            "'{url}' is not a recognized Bilibili link. Accepted forms include a full bilibili.com URL, a b23.tv short link, or a bare av / BV / ep / ss / md id."
        ));
    };

    let Some(_guard) = state.parse_guard.try_lock_for(std::time::Duration::from_secs(2)) else {
        return ToolOutput::error(
            "Another parse is already running. Only one parse can run at a time because it replaces the application's parse list. Retry once it finishes.",
        );
    };

    if state.user_has_selection() {
        return ToolOutput::error(
            "The user currently has items selected in the parse list. Parsing a new link would discard that selection. Ask the user to finish or clear it first.",
        );
    }

    let limit = clamp_limit(arguments.get("limit"));

    let outcome = run_parse(&state, &url, parser).await;
    let result = match outcome {
        Err(error) => {
            record_snapshot(&state);
            return ToolOutput::error(format!("Parsing failed: {error}"));
        }
        Ok(parsed) => parsed,
    };

    // Update the view under its lock, then run the preview flow.
    {
        let mut view = state.view.lock();
        let _ = view.update_tree(result.wrapper, result.current_episode);
    }
    let preview = preview_flow(&state).await;

    let collected = {
        let view = state.view.lock();
        let downloaded = state.downloaded.lock();
        collect_episodes(&view, limit, &downloaded)
    };

    // Auto-check the link target (view.update_tree already did) — record
    // the post-parse fingerprint so the guard ignores it next time.
    record_snapshot(&state);


    let mut structured = json!({"category": result.category});
    structured["total"] = json!(collected.total);
    structured["returned"] = json!(collected.episodes.len());
    if let Some(id) = &collected.link_target_id {
        structured["link_target_episode_id"] = json!(id);
        if let Some(episode) = &collected.link_target_episode {
            structured["link_target_episode"] = episode.clone();
        }
    }
    structured["episodes"] = Value::Array(collected.episodes.clone());

    let mut summary = format!("Parsed {} item(s) from {url}.", collected.total);
    summary.push_str(&link_target_note(&collected));

    let (options, source) = media_info_fields(&preview);
    if let Some(error) = media_info_error(&preview) {
        structured["media_info_available"] = json!(false);
        structured["media_info_error"] = json!(error);
        summary.push_str(&format!(
            " Media information is unavailable ({error}), so downloads cannot be created yet."
        ));
    } else {
        if options.as_object().is_some_and(|o| !o.is_empty()) {
            structured["available"] = options.clone();
        }
        if source.as_object().is_some_and(|o| !o.is_empty()) {
            structured["media_info_source"] = source.clone();
        }
        summary.push_str(&fallback_note(&source));
    }
    if collected.episodes.len() < collected.total {
        summary.push_str(&format!(
            " Showing the first {}; call get_episodes with a higher limit to see more.",
            collected.episodes.len()
        ));
    }

    ToolOutput::with_structured(summary, structured)
}

/// Record the current (all, checked) fingerprint after a parse settles.
fn record_snapshot(state: &McpState) {
    let view = state.view.lock();
    *state.snapshot.lock() = Some(ParseSnapshot {
        all: view
            .get_all_items()
            .iter()
            .enumerate()
            .map(|(i, _)| (i + 1).to_string())
            .collect(),
        checked: view
            .get_all_items()
            .iter()
            .enumerate()
            .filter(|(_, item)| item.checked == parsers::tree::CheckState::Checked)
            .map(|(i, _)| (i + 1).to_string())
            .collect(),
    });
}

/// One parse outcome: wrapper tree + locator + category label.
pub struct ParsedTree {
    pub wrapper: TreeItem,
    pub current_episode: Option<(String, String)>,
    pub category: &'static str,
}

/// Run the family dispatch (`ParseWorker.run` minus the Qt plumbing).
pub async fn run_parse(state: &McpState, url: &str, parser: ParserType) -> anyhow::Result<ParsedTree> {
    let client = http_client(state)?;
    let base = state.api_base.clone();
    let auth = auth_state(state);
    let raw = reqwest::Client::new();

    // b23 / festival: expand once, then re-classify.
    let mut url = url.to_owned();
    let mut parser = parser;
    if url.contains("b23") {
        url = tokio::time::timeout(PARSE_TIMEOUT, urls::expand::expand_b23(&raw, &url))
            .await
            .map_err(|_| anyhow::anyhow!("Parsing timed out after 90s."))??;
        parser = parser_type(&url).ok_or_else(|| anyhow::anyhow!("无效的链接"))?;
    }
    if url.contains("festival") {
        url = tokio::time::timeout(PARSE_TIMEOUT, urls::expand::resolve_festival(&raw, &url))
            .await
            .map_err(|_| anyhow::anyhow!("Parsing timed out after 90s."))??;
        parser = parser_type(&url).ok_or_else(|| anyhow::anyhow!("无效的链接"))?;
    }

    // `parsing()` clears stale side data, then registers this parse as
    // active — mirrors `EpisodeData.parsing()` in `ParseWorker.run`.
    let _guard = state.episode_data.parsing();
    state.episode_data.clear_cache();

    tokio::time::timeout(PARSE_TIMEOUT, async {
        dispatch_parse(&client, &base, &url, parser, &state.episode_data, &auth).await
    })
    .await
    .map_err(|_| anyhow::anyhow!("Parsing timed out after 90s."))?
}

/// Family dispatch → wrapped tree + locator + category.
async fn dispatch_parse(
    client: &network::HttpClient,
    base: &str,
    url: &str,
    parser: ParserType,
    episode_data: &Arc<EpisodeData>,
    auth: &auth::AuthState,
) -> anyhow::Result<ParsedTree> {
    use parsers::media_parser::{
        parse_bangumi_url, parse_cheese_url, parse_lesson_url, parse_video_url, ParseOutcome,
    };

    let wrap = |node: TreeItem, current: Option<(String, String)>, category: &'static str| {
        let (_title, wrapper) = wrap_node(node);
        ParsedTree { wrapper, current_episode: current, category }
    };

    match parser {
        ParserType::Video => {
            let mut current_url = url.to_owned();
            // A redirect_url chains into another family: re-dispatch.
            loop {
                match parse_video_url(client, base, &current_url, episode_data).await? {
                    ParseOutcome::Redirect(next) => {
                        current_url = next;
                        let Some(next_parser) = urls::classify::parser_type(&current_url) else {
                            anyhow::bail!("无效的链接");
                        };
                        if next_parser == ParserType::Video {
                            continue;
                        }
                        return Box::pin(dispatch_parse(
                            client, base, &current_url, next_parser, episode_data, auth,
                        ))
                        .await;
                    }
                    ParseOutcome::Interactive(tree) => {
                        return Ok(wrap(tree, None, category_name(parser)));
                    }
                    ParseOutcome::Episode(node, (key, value)) => {
                        return Ok(wrap(node, Some((key, value.to_string())), category_name(parser)));
                    }
                }
            }
        }
        // Bangumi/cheese/lesson: the tree is built inside the URL parser,
        // and the locate key comes from the same info the tree was built
        // from (`current_ep_id` / `locationInfo`).
        ParserType::Bangumi => {
            let (node, current) = parse_bangumi_url(client, base, url, episode_data).await?;
            Ok(wrap(node, current, "ANIME"))
        }
        ParserType::Cheese => {
            let (node, current) = parse_cheese_url(client, base, url, episode_data).await?;
            Ok(wrap(node, current, "CHEESE"))
        }
        ParserType::Lesson => {
            let (node, current) = parse_lesson_url(client, base, url, auth, episode_data).await?;
            Ok(wrap(node, current, "LESSON"))
        }
        // List family: the shared side-data table via the context.
        ParserType::Space => {
            let ctx = parsers::list::ListParseContext::with_episode_data(episode_data.clone());
            let output = parsers::list::parse_space(client, base, &ctx, url, 1).await?;
            Ok(wrap(output.node, output.current_episode, output.category))
        }
        ParserType::Favlist => {
            let ctx = parsers::list::ListParseContext::with_episode_data(episode_data.clone());
            let output = parsers::list::parse_favlist(client, base, &ctx, url, 1).await?;
            Ok(wrap(output.node, output.current_episode, output.category))
        }
        ParserType::List => {
            let ctx = parsers::list::ListParseContext::with_episode_data(episode_data.clone());
            let output = parsers::list::parse_list(client, base, &ctx, url, 1).await?;
            Ok(wrap(output.node, output.current_episode, output.category))
        }
        ParserType::Popular => {
            let ctx = parsers::list::ListParseContext::with_episode_data(episode_data.clone());
            let output = parsers::list::parse_popular(client, base, &ctx, url).await?;
            Ok(wrap(output.node, output.current_episode, output.category))
        }
        ParserType::WatchLater => {
            let ctx = parsers::list::ListParseContext::with_episode_data(episode_data.clone());
            let output = parsers::list::parse_watch_later(client, base, &ctx, url, auth, 1).await?;
            Ok(wrap(output.node, output.current_episode, output.category))
        }
        ParserType::History => {
            let ctx = parsers::list::ListParseContext::with_episode_data(episode_data.clone());
            let output = parsers::list::parse_history(client, base, &ctx, url, auth, 1).await?;
            Ok(wrap(output.node, output.current_episode, output.category))
        }
        ParserType::Audio => {
            let info = parsers::parse_audio(client, base, url).await?;
            let node = parsers::build_audio_tree(&info.data);
            Ok(wrap(node, None, "AUDIO"))
        }
        // These expand before dispatch ever runs.
        ParserType::Festival | ParserType::B23 => anyhow::bail!("无效的链接"),
    }
}

// ---------------------------------------------------------------------------
// get_episodes
// ---------------------------------------------------------------------------

pub fn tool_get_episodes(state: Arc<McpState>, arguments: &Map<String, Value>) -> ToolOutput {
    let limit = clamp_limit(arguments.get("limit"));

    let (collected, preview) = {
        let view = state.view.lock();
        let downloaded = state.downloaded.lock();
        (collect_episodes(&view, limit, &downloaded), state.preview.lock().clone())
    };

    if collected.total == 0 {
        return ToolOutput::with_structured(
            "The parse list is empty. Call parse_url first.",
            json!({"total": 0, "returned": 0, "episodes": []}),
        );
    }

    let mut structured = json!({
        "total": collected.total,
        "returned": collected.episodes.len(),
    });
    if let Some(id) = &collected.link_target_id {
        structured["link_target_episode_id"] = json!(id);
        if let Some(episode) = &collected.link_target_episode {
            structured["link_target_episode"] = episode.clone();
        }
    }
    structured["episodes"] = Value::Array(collected.episodes.clone());

    let (options, source) = media_info_fields(&preview);
    if options.as_object().is_some_and(|o| !o.is_empty()) {
        structured["available"] = options.clone();
    }
    if source.as_object().is_some_and(|o| !o.is_empty()) {
        structured["media_info_source"] = source.clone();
    }

    let mut summary = format!("{} item(s) in the parse list.", collected.total);
    summary.push_str(&link_target_note(&collected));
    summary.push_str(&fallback_note(&source));

    ToolOutput::with_structured(summary, structured)
}

pub fn parse_schemas() -> Vec<ToolSchema> {
    let limit_schema = json!({
        "type": "integer",
        "description": format!("Maximum number of episodes to return (1-{MAX_LIMIT}, default {DEFAULT_LIMIT})."),
        "minimum": 1,
        "maximum": MAX_LIMIT,
    });
    vec![
        ToolSchema {
            name: "parse_url",
            title: "Parse Bilibili Link",
            description: "Parse a Bilibili link and load its episodes into the application's parse list. Accepts a full URL, a b23.tv short link, or a bare av / BV / ep / ss / md id. This replaces whatever is currently in the parse list, and is refused while the user has items selected there. Call this before create_download. The result's 'available' field lists the qualities and codecs this content actually offers, which are the valid values for create_download's options. When the link points at one specific item of a multi-part video, season or collection, 'link_target_episode_id' names it (and that item carries 'is_link_target'); prefer it over guessing from titles, since every part of a video shares one bvid. Its absence means the link addressed the whole listing rather than a single item.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "The Bilibili link or id to parse."},
                    "limit": limit_schema,
                },
                "required": ["url"],
                "additionalProperties": false,
            }),
        },
        ToolSchema {
            name: "get_episodes",
            title: "List Parsed Episodes",
            description: "List the episodes currently loaded in the application's parse list, including the episode_id values needed by create_download, and 'link_target_episode_id' identifying which item the parsed link pointed at, when it pointed at one.",
            input_schema: json!({
                "type": "object",
                "properties": {"limit": limit_schema},
                "additionalProperties": false,
            }),
        },
    ]
}

