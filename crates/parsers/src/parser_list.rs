//! Collection/series list parser (ticket 09): port of
//! `src/util/parse/parser/list.py`.
//!
//! Resolves `https://www.bilibili.com/list/{mid}?...` play-page links
//! and `space.bilibili.com/{mid}/lists/{id}?type=season|series` links
//! into the season/series archive list the
//! [`crate::episode::list_parsers::parse_list`] episode parser renders.


use std::collections::HashMap;

use serde_json::Value;

use crate::episode::list_parsers;
use crate::tree::{Attribute, EpisodeData, TreeItem};

const ORIGIN: &str = "https://api.bilibili.com";
pub const VIDEO_VIEW_API: &str = "https://api.bilibili.com/x/web-interface/wbi/view";
pub const SERIES_INFO_API: &str = "https://api.bilibili.com/x/series/series";
pub const SEASONS_ARCHIVES_API: &str =
    "https://api.bilibili.com/x/polymer/web-space/seasons_archives_list";
pub const SERIES_ARCHIVES_API: &str = "https://api.bilibili.com/x/series/archives";

const PS: i64 = 30;

#[derive(Debug, thiserror::Error)]
pub enum ListError {
    #[error("{0}")]
    Api(String),
    #[error("无效的链接")]
    InvalidLink,
    #[error(transparent)]
    Network(#[from] network::HttpError),
}

fn path_of(url: &str) -> Option<String> {
    // path-only: scheme//authority… stripped, query and fragment dropped
    let rest = url.split_once("://")?.1;
    let rest = rest.split('/').skip(1).collect::<Vec<_>>().join("/");
    let rest = rest.split(['?', '#']).next().unwrap_or("").to_owned();
    Some(format!("/{rest}"))
}

fn season_id_of(season: &Value) -> Option<String> {
    match season.get("id")? {
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

fn text_of(value: &Value, key: &str) -> String {
    match &value[key] {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

fn endpoint(base: &str, api: &str) -> String {
    format!("{base}{}", api.trim_start_matches(ORIGIN))
}

fn find_str(pattern: &str, text: &str) -> Option<String> {
    let re = regex::Regex::new(pattern).ok()?;
    re.captures(text)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_owned())
}

/// Path-only mid (`get_mid`).
fn get_mid(url: &str) -> Option<String> {
    let path = path_of(url)?;
    find_str(r"/([0-9]+)", &path)
}

fn get_sid(url: &str) -> Option<String> {
    find_str(r"sid=([0-9]+)", url)
}

/// Query `bvid` parameter only (`get_bvid`).
fn get_bvid(url: &str) -> Option<String> {
    find_str(r"bvid=([Bb][Vv][0-9A-Za-z]+)", url)
}

fn get_oid(url: &str) -> Option<String> {
    find_str(r"oid=([0-9]+)", url)
}

fn get_season_id(url: &str) -> Option<String> {
    find_str(r"/([0-9]+)\?type=season", url)
}

fn get_series_id(url: &str) -> Option<String> {
    find_str(r"/([0-9]+)\?type=series", url)
}

fn kind_of(url: &str) -> Option<&'static str> {
    find_str(r"type=(season|series)", url).and_then(|k| match k.as_str() {
        "season" => Some("season"),
        "series" => Some("series"),
        _ => None,
    })
}

/// One parsed list (`ListParser.parse` with `get_info_data=False` plus
/// the episode tree it drives).
#[derive(Debug)]
pub struct ListResult {
    pub info_data: Value,
    /// `(key, value)` locating the entry the URL points at
    /// (`current_episode_data`).
    pub current_episode_data: Option<(String, Value)>,
    pub pagination: Option<HashMap<String, i64>>,
}

async fn get_json_map(
    client: &network::HttpClient,
    url: &str,
    params: &[(&str, String)],
) -> Result<Value, ListError> {
    let query: Vec<(String, String)> = params
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    let value: Value = client.get_json(url, &query).await?;
    if value.get("code").and_then(Value::as_i64).unwrap_or(-1) != 0 {
        return Err(ListError::Api(
            value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("未知错误")
                .to_owned(),
        ));
    }
    Ok(value)
}

/// Fetch the play page's video (`get_video_data`); `None` (and zero
/// requests) when the link carries neither bvid nor oid.
async fn get_video_data(
    client: &network::HttpClient,
    base: &str,
    url: &str,
) -> Result<Option<Value>, ListError> {
    let params: Vec<(String, String)> = if let Some(bvid) = get_bvid(url) {
        vec![("bvid".to_owned(), bvid)]
    } else if let Some(aid) = get_oid(url) {
        vec![("aid".to_owned(), aid)]
    } else {
        return Ok(None);
    };

    let response = get_json_map(
        client,
        &endpoint(base, VIDEO_VIEW_API),
        &params
            .iter()
            .map(|(k, v)| (k.as_str(), v.clone()))
            .collect::<Vec<_>>(),
    )
    .await?;
    Ok(Some(response["data"].clone()))
}

/// Probe whether `sid` names a series; its meta when it does (`get_series_meta`).
async fn get_series_meta(
    client: &network::HttpClient,
    base: &str,
    sid: &str,
) -> Result<Option<Value>, ListError> {
    let response: Value = client
        .get_json(
            &endpoint(base, SERIES_INFO_API),
            &[
                ("series_id".to_owned(), sid.to_owned()),
                ("web_location".to_owned(), "333.1387".to_owned()),
            ],
        )
        .await?;

    if response.get("code").and_then(Value::as_i64).unwrap_or(-1) != 0 {
        return Ok(None);
    }
    Ok(Some(response["data"]["meta"].clone()))
}

async fn get_seasons_archives_list(
    client: &network::HttpClient,
    base: &str,
    mid: &str,
    season_id: &str,
    pn: i64,
) -> Result<Value, ListError> {
    get_json_map(
        client,
        &endpoint(base, SEASONS_ARCHIVES_API),
        &[
            ("mid", mid.to_owned()),
            ("season_id", season_id.to_owned()),
            ("sort_reverse", "false".to_owned()),
            ("page_size", PS.to_string()),
            ("page_num", pn.to_string()),
            ("web_location", "333.1387".to_owned()),
        ],
    )
    .await
}

async fn get_series_archives_list(
    client: &network::HttpClient,
    base: &str,
    mid: &str,
    series_id: &str,
    pn: i64,
) -> Result<Value, ListError> {
    get_json_map(
        client,
        &endpoint(base, SERIES_ARCHIVES_API),
        &[
            ("mid", mid.to_owned()),
            ("current_mid", 0.to_string()),
            ("series_id", series_id.to_owned()),
            ("only_normal", "true".to_owned()),
            ("sort", "desc".to_owned()),
            ("ps", PS.to_string()),
            ("pn", pn.to_string()),
            ("web_location", "333.1387".to_owned()),
        ],
    )
    .await
}

/// `parse`: resolve the URL to its archive-list payload.
pub async fn parse_list_url(
    client: &network::HttpClient,
    base: &str,
    url: &str,
    pn: i64,
) -> Result<ListResult, ListError> {
    let mid = get_mid(url).unwrap_or_default();

    let (mut info_data, current) = if kind_of(url) == Some("season") {
        let season_id = get_season_id(url).ok_or(ListError::InvalidLink)?;
        let data = get_seasons_archives_list(client, base, &mid, &season_id, pn).await?;
        (data, None)
    } else if kind_of(url) == Some("series") {
        let series_id = get_series_id(url).ok_or(ListError::InvalidLink)?;
        let mut data = get_series_archives_list(client, base, &mid, &series_id, pn).await?;
        // The series archives endpoint has no meta; fetch it separately.
        let meta = get_series_meta(client, base, &series_id).await?;
        data["data"]["meta"] = meta.unwrap_or(Value::Null);
        (data, None)
    } else {
        parse_play_page(client, base, url, pn, &mid).await?
    };

    let total = info_data["data"]["page"]["total"].as_i64().unwrap_or(0);
    let mut pagination = None;
    if total > 0 {
        let mut map = HashMap::new();
        map.insert("total_pages".to_owned(), (total + PS - 1) / PS);
        map.insert("total_items".to_owned(), total);
        map.insert("current_page".to_owned(), pn);
        pagination = Some(map);
    }
    if let Some((key, value)) = &current {
        info_data["data"]["_current_bvid"] = value.clone();
        let _ = key;
    }

    Ok(ListResult {
        info_data,
        current_episode_data: current,
        pagination,
    })
}

async fn parse_play_page(
    client: &network::HttpClient,
    base: &str,
    url: &str,
    pn: i64,
    mid: &str,
) -> Result<(Value, Option<(String, Value)>), ListError> {
    let video_data = get_video_data(client, base, url).await?;
    let sid = get_sid(url);
    let season = video_data.as_ref().and_then(|v| v.get("ugc_season"));

    let (info_data, current) = if let Some(season) = season.filter(|s| {
        sid.as_deref()
            .map(|sid| season_id_of(s) == Some(sid.to_owned()))
            .unwrap_or(true)
    }) {
        let season_mid = text_of(season, "mid");
        let season_id = text_of(season, "id");
        let data =
            get_seasons_archives_list(client, base, &season_mid, &season_id, pn).await?;
        (data, None)
    } else if let Some(sid) = sid.as_deref() {
        parse_by_sid(client, base, sid, mid, pn).await?
    } else {
        return Err(ListError::InvalidLink);
    };
    let _ = &current;

    let current = video_data
        .as_ref()
        .map(|v| ("bvid".to_owned(), v["bvid"].clone()));

    Ok((info_data, current))
}

async fn parse_by_sid(
    client: &network::HttpClient,
    base: &str,
    sid: &str,
    mid: &str,
    pn: i64,
) -> Result<(Value, Option<(String, Value)>), ListError> {
    let meta = get_series_meta(client, base, sid).await?;
    if let Some(meta) = meta {
        let mut data = get_series_archives_list(client, base, mid, sid, pn).await?;
        data["data"]["meta"] = meta;
        Ok((data, None))
    } else {
        let data = get_seasons_archives_list(client, base, mid, sid, pn).await?;
        Ok((data, None))
    }
}

/// Episode-tree rendering of a parsed list (`ListEpisodeParser.parse`).
pub fn build_list_tree(episode_data: &EpisodeData, info_data: &Value) -> TreeItem {
    list_parsers::parse_list(episode_data, info_data)
}

/// Convenience wrapper matching the Python `parse()` call shape:
/// fetch + render, with the current-video marker already applied.
pub async fn parse_and_build(
    client: &network::HttpClient,
    base: &str,
    url: &str,
    pn: i64,
    episode_data: &EpisodeData,
) -> Result<(ListResult, TreeItem), ListError> {
    let result = parse_list_url(client, base, url, pn).await?;
    let tree = build_list_tree(episode_data, &result.info_data);
    Ok((result, tree))
}

/// Re-exported so callers don't need to know the bit layout.
pub use crate::episode::list_base::wrap_node as wrap_list_node;

// Silence unused warnings for helpers retained for parity.
#[allow(unused_imports)]
use crate::episode::list_base::episode_type;
#[allow(unused_imports)]
use Attribute as _AttributeAlias;
