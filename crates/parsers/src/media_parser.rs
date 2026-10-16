//! Media-link HTTP parsers (ticket 09): ports of
//! `src/util/parse/parser/{video,bangumi,cheese,lesson}.py`.
//!
//! All take `(&HttpClient, base)` and swap the API host for the wiremock
//! `base` the same way `batch::fetch_video_data` does, so tests hit the
//! mock server. `ParseOutcome` covers the dispatch `VideoParser.parse`
//! performs: a redirect link re-parses, a stein (interactive) video
//! walks its node graph, everything else builds the episode tree.

use std::collections::{HashMap, HashSet, VecDeque};

use regex::Regex;
use serde_json::{json, Value};

use crate::episode::bangumi::parse_bangumi;
use crate::episode::media::{parse_cheese, parse_lesson};
use crate::episode::video::parse_video;
use crate::tree::{Attribute, EpisodeData, Number, TreeItem};

use crate::batch::aid_to_bvid;
use crate::list::{check_response, swap_host};

pub const VIDEO_VIEW_API: &str = "https://api.bilibili.com/x/web-interface/wbi/view";
pub const STEIN_EDGEINFO_API: &str = "https://api.bilibili.com/x/stein/edgeinfo_v2";
pub const PLAYER_V2_API: &str = "https://api.bilibili.com/x/player/wbi/v2";
pub const MEDIA_REVIEW_API: &str = "https://api.bilibili.com/pgc/review/user";
pub const BANGUMI_SEASON_API: &str = "https://api.bilibili.com/pgc/view/web/season";
pub const CHEESE_SEASON_API: &str = "https://api.bilibili.com/pugv/view/web/season/v2";
pub const LESSON_API_BASE: &str = "https://mall.bilibili.com/mall-search-items";
pub const LESSON_PLAY_DETAIL_URL: &str = "https://mall.bilibili.com/mall-search-items/items/course/section/play/detail";
pub const LESSON_VIDEO_QUALITY_ID: i64 = 80;

/// Errors of the media parsers, mirroring `check_response` plus the
/// Python raisers (`ValueError("无效的链接")`, the stein graph error,
/// the login guard).
#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("network: {0}")]
    Network(#[from] network::HttpError),
    #[error("{0}")]
    Api(String),
    #[error("无效的链接")]
    InvalidLink,
    #[error("请先登录账号")]
    LoginRequired,
}

/// What `VideoParser.parse` decides a link is.
#[derive(Debug)]
pub enum ParseOutcome {
    /// `redirect_url` present — the caller should re-parse this link.
    Redirect(String),
    /// A stein-gated interactive video; the tree is the walked node
    /// graph (`node_list`).
    Interactive(TreeItem),
    /// A regular video tree with its `(key, value)` locator.
    Episode(TreeItem, (String, Value)),
}

fn find_str(pattern: &str, url: &str) -> Option<String> {
    Regex::new(pattern)
        .ok()
        .and_then(|re| re.find(url).map(|m| m.as_str().to_owned()))
}

/// `VideoParser.parse` — av/BV dispatch, view fetch, page-number
/// override, then redirect / interactive / episode dispatch.
pub async fn parse_video_url(
    client: &network::HttpClient,
    base: &str,
    url: &str,
    episode_data: &EpisodeData,
) -> Result<ParseOutcome, MediaError> {
    let bvid = video_bvid(url).ok_or(MediaError::InvalidLink)?;

    let mut data = get_video_info(client, base, &bvid, url).await?;

    if let Some(redirect) = data.get("redirect_url").and_then(Value::as_str) {
        return Ok(ParseOutcome::Redirect(redirect.to_owned()));
    }

    if data["rights"]["is_stein_gate"].as_i64() == Some(1) {
        let tree = parse_interactive(client, base, &data, episode_data).await?;
        return Ok(ParseOutcome::Interactive(tree));
    }

    let episode_id = episode_data.add_episode();
    let (node, current) = parse_video(episode_data, &episode_id, &data);
    let _ = &mut data;
    Ok(ParseOutcome::Episode(node, current))
}

/// `get_bvid`/`get_aid` combined: BV pass-through, av converted.
pub fn video_bvid(url: &str) -> Option<String> {
    if let Some(bv) = find_str(r"(?:BV|bv)\w+", url) {
        return Some(bv);
    }
    find_str(r"(?:av|AV)([0-9]+)", url)
        .and_then(|m| m[2..].parse::<i64>().ok())
        .map(aid_to_bvid)
}

/// `get_page_number`: the `?p=` page of a multi-part link.
pub fn get_page_number(url: &str) -> Option<i64> {
    Regex::new(r"p=(\d+)")
        .unwrap()
        .captures(url)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
}

/// `get_video_info`: view fetch, `current_page_number` injection, and —
/// when the caller wants to parse — the page-override of
/// `VideoParser.parse(get_info_data=True)`.
pub async fn get_video_info(
    client: &network::HttpClient,
    base: &str,
    bvid: &str,
    url: &str,
) -> Result<Value, MediaError> {
    let params = [("bvid".to_owned(), bvid.to_owned())];
    let response: Value = client
        .get_json(&swap_host(VIDEO_VIEW_API, base), &params)
        .await?;
    check_response(&response).map_err(|e| MediaError::Api(e.to_string()))?;

    let mut data = response["data"].clone();

    // page pin: remember which page the link targets (get_video_info).
    if let Some(page) = get_page_number(url) {
        data["current_page_number"] = json!(page);
    }

    // get_info_data override: a pinned multi-part link folds its page
    // data into the top-level fields and drops `pages`.
    if data.get("pages").is_some() {
        if let Some(page_number) = get_page_number(url) {
            if let Some(page) = data["pages"]
                .as_array()
                .and_then(|pages| pages.iter().find(|p| p["page"].as_i64() == Some(page_number)))
                .cloned()
            {
                data.as_object_mut().map(|d| d.remove("pages"));
                data["cid"] = page["cid"].clone();
                data["title"] = page["part"].clone();
                data["duration"] = page["duration"].clone();
                data["pubdate"] = page["ctime"].clone();
            }
        }
    }

    Ok(data)
}

// ---------------------------------------------------------------------------
// Interactive (stein) videos
// ---------------------------------------------------------------------------

/// One walked node of the story graph.
#[derive(Debug, Clone)]
pub struct StoryNode {
    pub cid: i64,
    pub title: String,
    pub options: Vec<StoryOption>,
}

/// One choice leaving a node.
#[derive(Debug, Clone)]
pub struct StoryOption {
    pub edge_id: i64,
    pub name: String,
    pub target_node_cid: Option<i64>,
}

/// `InteractiveVideoParser.parse` — BFS the story graph from the entry
/// `(cid, edge_id=0)` and fold every visited node into one episode tree.
/// `on_node` mirrors the Python `_update_ui_progress` callback seam.
pub async fn parse_interactive(
    client: &network::HttpClient,
    base: &str,
    data: &Value,
    episode_data: &EpisodeData,
) -> Result<TreeItem, MediaError> {
    let mut context = InteractiveContext::new(client, base, data).await?;

    let episode_id = episode_data.add_episode();
    episode_data.set_episode_data(
        &episode_id,
        Value::Object(crate::episode::video::video_episode_data(data)),
    );

    // BFS over (cid, edge_id) states, deduped like `visited_states`.
    let mut pending: VecDeque<(i64, i64)> = VecDeque::new();
    pending.push_back((context.cid, 0));
    let mut nodes: Vec<StoryNode> = Vec::new();

    while let Some((cid, edge_id)) = pending.pop_front() {
        let node = context.get_edge_info(cid, edge_id).await?;

        for option in &node.options {
            if let Some(target) = option.target_node_cid {
                let state = (target, option.edge_id);
                if !context.visited.contains(&state) {
                    pending.push_back(state);
                }
            }
        }

        nodes.push(node.clone());
    }

    // DynamicEpisodeParser.update shapes each node as one episode item.
    let info_title = data["title"].as_str().unwrap_or_default().to_owned();
    let mut root = TreeItem::root(Number::Text("互动视频".to_owned()), &info_title);
    root.set_attribute(Attribute::TREE_NODE_BIT);

    for (index, node) in nodes.iter().enumerate() {
        let mut item = TreeItem::empty();
        item.episode_id = episode_id.clone();
        item.badge = if data["is_upower_exclusive"].as_bool().unwrap_or(false) {
            "充电专属".to_owned()
        } else {
            String::new()
        };
        item.aid = data["aid"].as_i64().unwrap_or(0);
        item.bvid = data["bvid"].as_str().unwrap_or_default().to_owned();
        item.cid = node.cid;
        item.cover = data["pic"].as_str().unwrap_or_default().to_owned();
        item.duration = 0;
        item.number = Number::Int(index as i64 + 1);
        item.pubtime = data["pubdate"].as_i64().unwrap_or(0);
        item.title = node.title.clone();
        item.related_titles
            .insert("collection_title".into(), Value::String(info_title.clone()));
        item.url = format!("https://www.bilibili.com/video/{}", item.bvid);
        item.set_attribute(Attribute::INTERACTIVE_BIT | Attribute::VIDEO_BIT);
        root.add_child(item);
    }

    Ok(root)
}

/// The graph walker state (`InteractiveVideoParser` minus the GUI
/// signal emissions).
struct InteractiveContext<'a> {
    client: &'a network::HttpClient,
    base: String,
    bvid: String,
    cid: i64,
    graph_version: i64,
    visited: HashSet<(i64, i64)>,
    node_map: HashMap<i64, StoryNode>,
}

impl<'a> InteractiveContext<'a> {
    async fn new(
        client: &'a network::HttpClient,
        base: &str,
        data: &Value,
    ) -> Result<Self, MediaError> {
        let aid = data["aid"].as_i64().unwrap_or(0);
        let cid = data["cid"].as_i64().unwrap_or(0);

        let params = [
            ("aid".to_owned(), aid.to_string()),
            ("cid".to_owned(), cid.to_string()),
            ("isGaiaAvoided".to_owned(), "false".to_owned()),
            ("web_location".to_owned(), "1315873".to_owned()),
            ("dm_img_list".to_owned(), "[]".to_owned()),
            (
                "dm_img_str".to_owned(),
                "V2ViR0wgMS4wIChPcGVuR0wgRVMgMi4wIENocm9taXVtKQ".to_owned(),
            ),
            (
                "dm_cover_img_str".to_owned(),
                "QU5HTEUgKE5WSURJQSwgTlZJRElBIEdlRm9yY2UgUlRYIDQwNjAgTGFwdG9wIEdQVSAoMHgwMDAwMjhFMCkgRGlyZWN0M0QxMSB2c181XzAgcHNfNV8wLCBEM1QxMSlHb29nbGUgSW5jLiAoTlZJRElBKQ".to_owned(),
            ),
            (
                "dm_img_inter".to_owned(),
                r#"{"ds":[],"wh":[5073,6031,29],"of":[206,412,206]}"#.to_owned(),
            ),
        ];
        let response: Value = client
            .get_json(&swap_host(PLAYER_V2_API, base), &params)
            .await?;
        check_response(&response).map_err(|e| MediaError::Api(e.to_string()))?;

        let graph_version = response["data"]["interaction"]["graph_version"]
            .as_i64()
            .ok_or_else(|| {
                MediaError::Api("无法获取 graph_version，可能不是互动视频".to_owned())
            })?;

        Ok(Self {
            client,
            base: base.to_owned(),
            bvid: data["bvid"].as_str().unwrap_or_default().to_owned(),
            cid,
            graph_version,
            visited: HashSet::new(),
            node_map: HashMap::new(),
        })
    }

    /// `_get_node_title`: node title, else the first story_list title.
    fn node_title(data: &Value) -> String {
        if let Some(title) = data.get("title").and_then(Value::as_str) {
            if !title.is_empty() {
                return title.to_owned();
            }
        }
        data["story_list"]
            .as_array()
            .and_then(|list| list.first())
            .and_then(|s| s.get("title"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    }

    /// `_build_options` from the edges block.
    fn build_options(edges: &Value) -> Vec<StoryOption> {
        let mut options = Vec::new();
        for question in edges["questions"].as_array().cloned().unwrap_or_default() {
            let show = question.get("type").and_then(Value::as_i64).unwrap_or(1) != 0;
            for choice in question["choices"].as_array().cloned().unwrap_or_default() {
                if !show {
                    continue;
                }
                options.push(StoryOption {
                    edge_id: choice["id"].as_i64().unwrap_or(0),
                    name: choice.get("option").and_then(Value::as_str).unwrap_or("").to_owned(),
                    target_node_cid: choice.get("cid").and_then(Value::as_i64),
                });
            }
        }
        options
    }

    /// `get_edge_info`: fetch one node, merge its options, mark visited.
    async fn get_edge_info(&mut self, cid: i64, edge_id: i64) -> Result<StoryNode, MediaError> {
        let params = [
            ("bvid".to_owned(), self.bvid.clone()),
            ("graph_version".to_owned(), self.graph_version.to_string()),
            ("edge_id".to_owned(), edge_id.to_string()),
        ];
        let response: Value = client_get(self.client, &swap_host(STEIN_EDGEINFO_API, &self.base), &params).await?;
        check_response(&response).map_err(|e| MediaError::Api(e.to_string()))?;

        let data = &response["data"];
        let title = Self::node_title(data);
        let options = Self::build_options(&data["edges"]);

        self.visited.insert((cid, edge_id));

        let node = self.node_map.entry(cid).or_insert_with(|| StoryNode {
            cid,
            title: title.clone(),
            options: Vec::new(),
        });
        if !title.is_empty() && node.title != title {
            node.title = title;
        }
        // `_merge_options`: dedup by edge_id.
        let existing: HashSet<i64> = node.options.iter().map(|o| o.edge_id).collect();
        for option in options {
            if !existing.contains(&option.edge_id) {
                node.options.push(option);
            }
        }

        Ok(node.clone())
    }
}

async fn client_get(
    client: &network::HttpClient,
    url: &str,
    params: &[(String, String)],
) -> Result<Value, network::HttpError> {
    client.get_json(url, params).await
}

// ---------------------------------------------------------------------------
// Bangumi
// ---------------------------------------------------------------------------

/// `BangumiParser.parse`: ep/ss/md dispatch → season fetch → tree.
pub async fn parse_bangumi_url(
    client: &network::HttpClient,
    base: &str,
    url: &str,
    episode_data: &EpisodeData,
) -> Result<TreeItem, MediaError> {
    let (param, ep_id) = bangumi_param(client, base, url).await?;

    let response: Value = client
        .get_json(&swap_host(BANGUMI_SEASON_API, base), &[(param.0, param.1)])
        .await?;
    check_response(&response).map_err(|e| MediaError::Api(e.to_string()))?;

    let mut result = response["result"].clone();
    if let Some(ep) = ep_id {
        result["current_ep_id"] = json!(ep);
    }

    let episode_id = episode_data.add_episode();
    Ok(parse_bangumi(episode_data, &episode_id, &result))
}

/// `get_ep_id`/`get_season_id`/`get_media_id`: which season param the
/// link resolves to (md links need one extra review fetch).
pub async fn bangumi_param(
    client: &network::HttpClient,
    base: &str,
    url: &str,
) -> Result<((String, String), Option<i64>), MediaError> {
    match find_str(r"ep|ss|md", url).as_deref() {
        Some("ep") => {
            let id = capture_id(r"ep([0-9]+)", url).ok_or(MediaError::InvalidLink)?;
            Ok((("ep_id".to_owned(), id.to_string()), Some(id)))
        }
        Some("ss") => {
            let id = capture_id(r"ss([0-9]+)", url).ok_or(MediaError::InvalidLink)?;
            Ok((("season_id".to_owned(), id.to_string()), None))
        }
        Some("md") => {
            let media_id = capture_id(r"md([0-9]+)", url).ok_or(MediaError::InvalidLink)?;
            let response: Value = client
                .get_json(
                    &swap_host(MEDIA_REVIEW_API, base),
                    &[("media_id".to_owned(), media_id.to_string())],
                )
                .await?;
            check_response(&response).map_err(|e| MediaError::Api(e.to_string()))?;
            let season_id = response["result"]["media"]["season_id"]
                .as_i64()
                .unwrap_or(0);
            Ok((("season_id".to_owned(), season_id.to_string()), None))
        }
        _ => Err(MediaError::InvalidLink),
    }
}

fn capture_id(pattern: &str, url: &str) -> Option<i64> {
    Regex::new(pattern)
        .ok()
        .and_then(|re| re.captures(url))
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
}

// ---------------------------------------------------------------------------
// Cheese
// ---------------------------------------------------------------------------

/// `CheeseParser.parse`: ep/ss dispatch → pugv season fetch → tree.
pub async fn parse_cheese_url(
    client: &network::HttpClient,
    base: &str,
    url: &str,
    episode_data: &EpisodeData,
) -> Result<TreeItem, MediaError> {
    let ((key, value), ep_id) = match find_str(r"ep|ss", url).as_deref() {
        Some("ep") => (
            ("ep_id".to_owned(), capture_id(r"ep([0-9]+)", url).ok_or(MediaError::InvalidLink)?.to_string()),
            Some(capture_id(r"ep([0-9]+)", url).ok_or(MediaError::InvalidLink)?),
        ),
        Some("ss") => (
            ("season_id".to_owned(), capture_id(r"ss([0-9]+)", url).ok_or(MediaError::InvalidLink)?.to_string()),
            None,
        ),
        _ => return Err(MediaError::InvalidLink),
    };

    let response: Value = client
        .get_json(&swap_host(CHEESE_SEASON_API, base), &[(key, value)])
        .await?;
    check_response(&response).map_err(|e| MediaError::Api(e.to_string()))?;

    let mut data = response["data"].clone();
    if let Some(ep) = ep_id {
        data["current_ep_id"] = json!(ep);
    }

    let episode_id = episode_data.add_episode();
    Ok(parse_cheese(episode_data, &episode_id, &data))
}

// ---------------------------------------------------------------------------
// Mall lesson
// ---------------------------------------------------------------------------

/// `LessonParser.parse`: login guard → id extraction → POST detail →
/// tree. The three ids are merged into the data so the episode parser
/// can build play URLs.
pub async fn parse_lesson_url(
    client: &network::HttpClient,
    base: &str,
    url: &str,
    auth: &auth::AuthState,
    episode_data: &EpisodeData,
) -> Result<TreeItem, MediaError> {
    if !auth.is_login {
        return Err(MediaError::LoginRequired);
    }

    let (course_id, lesson_id, item_id) = lesson_ids(url)?;

    let response: Value = client
        .post_json(
            &swap_host(&format!("{LESSON_API_BASE}/items/course/h5/detail"), base),
            &[],
            &json!({
                "courseId": course_id,
                "lessonId": lesson_id,
                "itemId": item_id,
            }),
        )
        .await?;
    check_response(&response).map_err(|e| MediaError::Api(e.to_string()))?;

    let mut data = match response.get("data") {
        Some(Value::Object(map)) => map.clone(),
        _ => {
            return Err(MediaError::Api(
                "接口未返回课程数据，请确认该课程是否已购买".to_owned(),
            ))
        }
    };
    data.insert("courseId".into(), json!(course_id));
    data.insert("lessonId".into(), json!(lesson_id));
    data.insert("itemId".into(), json!(item_id));

    let episode_id = episode_data.add_episode();
    Ok(parse_lesson(
        episode_data,
        &episode_id,
        &Value::Object(data),
    ))
}

/// `get_id_from_url`: strictly-numeric query params.
pub fn lesson_ids(url: &str) -> Result<(i64, i64, i64), MediaError> {
    fn id_of(url: &str, name: &str) -> Result<i64, MediaError> {
        let query = url.split_once('?').map(|(_, q)| q).unwrap_or_default();
        let prefix = format!("{name}=");
        let value = query
            .split('&')
            .find_map(|pair| pair.strip_prefix(&prefix))
            .unwrap_or_default()
            .trim();
        if value.is_empty() || !value.chars().all(|c| c.is_ascii_digit()) {
            return Err(MediaError::InvalidLink);
        }
        value.parse().map_err(|_| MediaError::InvalidLink)
    }

    Ok((
        id_of(url, "courseId")?,
        id_of(url, "lessonId")?,
        id_of(url, "itemId")?,
    ))
}

/// `build_lesson_play_payload`: section play request body.
pub fn build_lesson_play_payload(
    course_id: i64,
    lesson_id: i64,
    item_id: i64,
    section_id: i64,
) -> Value {
    json!({
        "courseId": course_id,
        "lessonId": lesson_id,
        "itemId": item_id,
        "sectionId": section_id,
    })
}

/// `build_lesson_media_info`: wrap the single mp4 direct link into the
/// durl playurl shape the download side consumes.
pub fn build_lesson_media_info(data: &Value) -> Result<Value, MediaError> {
    let video_url = data["videoUrl"].as_str().unwrap_or_default();
    if video_url.is_empty() {
        return Err(MediaError::Api("接口未返回该小节的播放地址".to_owned()));
    }

    Ok(json!({
        "format": "mp4",
        "quality": LESSON_VIDEO_QUALITY_ID,
        "accept_quality": [LESSON_VIDEO_QUALITY_ID],
        "timelength": 0,
        "durl": [{
            "order": 1,
            "length": data.get("videoTime").and_then(Value::as_i64).unwrap_or(0) * 1000,
            "size": 0,
            "url": video_url,
            "backup_url": [],
        }],
    }))
}

