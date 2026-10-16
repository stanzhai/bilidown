//! Playurl fetching and stream selection shared by the Preview and the
//! downloader (ticket 12). Ports `src/util/parse/preview/{previewer,worker,
//! video_info,audio_info}.py`, `src/util/parse/quality.py`,
//! `src/util/download/parse/{video_info,audio_info,query_worker}.py` and the
//! pure halves of `src/util/download/downloader/parse_worker.py`.
//!
//! Priority lists are plain `&[i64]` parameters — the caller reads them from
//! the config store's `Download` group (`crates/config/src/defaults.json`).

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use network::cdn::CdnConfig;
use network::probe::{resolve_download_url, AllCandidatesFailed, ResolvedUrl};
use network::{ClientOptions, HttpClient, HttpError};
use parking_lot::Mutex;
use serde_json::{json, Value};
use url::Url;

use crate::tree::{Attribute, TreeItem};

// ---------------------------------------------------------------------------
// Endpoints (parser/base.py, lesson.py)
// ---------------------------------------------------------------------------

pub const VIDEO_PLAYURL_API: &str = "https://api.bilibili.com/x/player/wbi/playurl";
pub const BANGUMI_PLAYURL_API: &str = "https://api.bilibili.com/pgc/player/web/playurl";
pub const CHEESE_PLAYURL_API: &str = "https://api.bilibili.com/pugv/player/web/playurl";
pub const LESSON_PLAY_DETAIL_URL: &str =
    "https://mall.bilibili.com/mall-search-items/items/course/section/play/detail";
pub const AUDIO_URL_API: &str = "https://www.bilibili.com/audio/music-service-c/web/url";

/// API bases, injectable so wiremock tests can mount the endpoints
/// (`crates/parsers/tests/area.rs` precedent).
#[derive(Clone, Debug)]
pub struct PlayurlApis {
    pub video: String,
    pub bangumi: String,
    pub cheese: String,
    pub lesson: String,
    pub audio: String,
}

impl Default for PlayurlApis {
    fn default() -> Self {
        Self {
            video: VIDEO_PLAYURL_API.into(),
            bangumi: BANGUMI_PLAYURL_API.into(),
            cheese: CHEESE_PLAYURL_API.into(),
            lesson: LESSON_PLAY_DETAIL_URL.into(),
            audio: AUDIO_URL_API.into(),
        }
    }
}

impl PlayurlApis {
    pub fn with_base(base: &str) -> Self {
        Self {
            video: format!("{base}/x/player/wbi/playurl"),
            bangumi: format!("{base}/pgc/player/web/playurl"),
            cheese: format!("{base}/pugv/player/web/playurl"),
            lesson: format!("{base}/mall-search-items/items/course/section/play/detail"),
            audio: format!("{base}/audio/music-service-c/web/url"),
        }
    }
}

// Preview-side request quality ids (previewer.py).
pub const PREVIEW_VIDEO_QN: i64 = 127;
pub const PREVIEW_BANGUMI_QN: i64 = 80;
pub const PREVIEW_CHEESE_QN: i64 = 0;

// ---------------------------------------------------------------------------
// Media maps (src/util/common/data/media_info.py)
// ---------------------------------------------------------------------------

pub const LESSON_VIDEO_QUALITY_ID: i64 = 80;
pub const QUALITY_AUTO: i64 = 200;
pub const AUDIO_QUALITY_AUTO: i64 = 30300;
pub const CODEC_AUTO: i64 = 20;
pub const FLAC_QUALITY_ID: i64 = 30251;
pub const DOLBY_QUALITY_ID: i64 = 30250;
pub const DOLBY_RAW_ID: i64 = 30255;
pub const M4A_QUALITY_ID: i64 = 30280;

pub static VIDEO_QUALITY_MAP: LazyLock<HashMap<&'static str, i64>> = LazyLock::new(|| {
    [
        ("auto", 200),
        ("8K", 127),
        ("DOLBY_VISION", 126),
        ("HDR", 125),
        ("4K_SDR", 122),
        ("4K", 120),
        ("1080P60", 116),
        ("1080P+", 112),
        ("AI", 100),
        ("1080P", 80),
        ("720P", 64),
        ("480P", 32),
        ("360P", 16),
    ]
    .into_iter()
    .collect()
});

pub static AUDIO_QUALITY_MAP: LazyLock<HashMap<&'static str, i64>> = LazyLock::new(|| {
    [
        ("auto", 30300),
        ("HI_RES", 30251),
        ("DOLBY_ATMOS", 30250),
        ("192K", 30280),
        ("132K", 30232),
        ("64K", 30216),
    ]
    .into_iter()
    .collect()
});

pub static AUDIO_REORDER_MAP: LazyLock<HashMap<i64, i64>> = LazyLock::new(|| {
    [
        (30255, 0),
        (30251, 1),
        (30250, 2),
        (30280, 3),
        (30232, 4),
        (30216, 5),
    ]
    .into_iter()
    .collect()
});

pub static VIDEO_CODEC_MAP: LazyLock<HashMap<&'static str, i64>> = LazyLock::new(|| {
    [
        ("auto", 20),
        ("AVC/H.264", 7),
        ("HEVC/H.265", 12),
        ("AV1", 13),
    ]
    .into_iter()
    .collect()
});

pub static CODEC_PREFIX_MAP: LazyLock<HashMap<&'static str, i64>> = LazyLock::new(|| {
    [
        ("avc1", 7),
        ("hev1", 12),
        ("hvc1", 12),
        ("dvh1", 12),
        ("av01", 13),
    ]
    .into_iter()
    .collect()
});

fn label_of(map: &HashMap<&'static str, i64>, id: i64) -> Option<&'static str> {
    map.iter().find(|(_, &v)| v == id).map(|(k, _)| *k)
}

pub fn video_quality_label(id: i64) -> Option<&'static str> {
    label_of(&VIDEO_QUALITY_MAP, id)
}

pub fn audio_quality_label(id: i64) -> Option<&'static str> {
    label_of(&AUDIO_QUALITY_MAP, id)
}

pub fn codec_label(id: i64) -> Option<&'static str> {
    label_of(&VIDEO_CODEC_MAP, id)
}

/// `codec_list` builder from a dash video node list (quality.py): first
/// dot-segment of `codecs`, prefix-mapped, deduped preserving order.
pub fn codec_list_from_nodes(nodes: &[Value]) -> Vec<i64> {
    let mut out = Vec::new();
    for node in nodes {
        let Some(codecs) = node.get("codecs").and_then(Value::as_str) else {
            continue;
        };
        let prefix = codecs.split('.').next().unwrap_or_default();
        if let Some(&id) = CODEC_PREFIX_MAP.get(prefix) {
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// MediaType (previewer.py enum MediaType)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MediaType {
    #[default]
    Unknown,
    Dash,
    Mp4,
    Flv,
    M4a,
}

pub fn detect_media_type(info_data: &Value) -> MediaType {
    if info_data.get("dash").is_some() {
        MediaType::Dash
    } else if let Some(f) = info_data.get("format").and_then(Value::as_str) {
        if f.starts_with("mp4") {
            MediaType::Mp4
        } else if f.starts_with("flv") {
            MediaType::Flv
        } else if f.starts_with("m4a") {
            MediaType::M4a
        } else {
            MediaType::Unknown
        }
    } else {
        MediaType::Unknown
    }
}

// ---------------------------------------------------------------------------
// Errors / response checks
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum PlayurlError {
    #[error("{0}")]
    Api(String),
    #[error(transparent)]
    Http(#[from] HttpError),
}

/// Preview-side `check_response`: 412/87008 get dedicated messages, anything
/// else uses `message` / `未知错误`.
pub fn check_response(payload: &Value) -> Result<(), PlayurlError> {
    let code = payload.get("code").and_then(Value::as_i64).unwrap_or(0);
    if code == 0 {
        return Ok(());
    }
    let message = match code {
        412 => "Request blocked, client IP has been banned",
        87008 => "Paid content requires purchasing",
        _ => payload
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("未知错误"),
    };
    Err(PlayurlError::Api(message.to_string()))
}

// ---------------------------------------------------------------------------
// Fetch per content type (previewer.py dispatch + parse_worker.py)
// ---------------------------------------------------------------------------

/// Fetched playurl body: the `data`/`result` object with `parser_type` and
/// `query_url` injected, exactly like the Python layer.
#[derive(Clone, Debug)]
pub struct FetchedMedia {
    pub parser_type: &'static str,
    pub media_type: MediaType,
    pub info_data: Value,
}

fn with_context(
    mut body: Value,
    parser_type: &'static str,
    base: &str,
    params: &[(String, String)],
) -> Value {
    let mut url =
        Url::parse(base).unwrap_or_else(|_| Url::parse("https://api.bilibili.com/").unwrap());
    {
        let mut pairs = url.query_pairs_mut();
        pairs.clear();
        for (k, v) in params {
            pairs.append_pair(k, v);
        }
    }
    body["parser_type"] = Value::String(parser_type.into());
    body["query_url"] = Value::String(url.to_string());
    body
}

fn take_body(payload: &Value, key: &str) -> Result<Value, PlayurlError> {
    payload
        .get(key)
        .cloned()
        .ok_or_else(|| PlayurlError::Api(format!("response missing {key}")))
}

pub async fn fetch_video_info(
    client: &HttpClient,
    apis: &PlayurlApis,
    bvid: &str,
    cid: i64,
    qn: i64,
) -> Result<FetchedMedia, PlayurlError> {
    let params: Vec<(String, String)> = vec![
        ("bvid".into(), bvid.into()),
        ("cid".into(), cid.to_string()),
        ("qn".into(), qn.to_string()),
        ("fnver".into(), "0".into()),
        ("fnval".into(), "4048".into()),
        ("fourk".into(), "1".into()),
    ];
    let payload: Value = client.get_json(&apis.video, &params).await?;
    check_response(&payload)?;
    let body = take_body(&payload, "data")?;
    Ok(FetchedMedia {
        parser_type: "video",
        media_type: detect_media_type(&body),
        info_data: with_context(body, "video", &apis.video, &params),
    })
}

pub async fn fetch_bangumi_info(
    client: &HttpClient,
    apis: &PlayurlApis,
    bvid: &str,
    cid: i64,
    qn: i64,
) -> Result<FetchedMedia, PlayurlError> {
    let params: Vec<(String, String)> = vec![
        ("bvid".into(), bvid.into()),
        ("cid".into(), cid.to_string()),
        ("qn".into(), qn.to_string()),
        ("fnver".into(), "0".into()),
        ("fnval".into(), "143312".into()),
        ("fourk".into(), "1".into()),
    ];
    let payload: Value = client.get_json(&apis.bangumi, &params).await?;
    check_response(&payload)?;
    let body = take_body(&payload, "result")?;
    Ok(FetchedMedia {
        parser_type: "bangumi",
        media_type: detect_media_type(&body),
        info_data: with_context(body, "bangumi", &apis.bangumi, &params),
    })
}

pub async fn fetch_cheese_info(
    client: &HttpClient,
    apis: &PlayurlApis,
    aid: i64,
    cid: i64,
    qn: i64,
    ep_id: i64,
) -> Result<FetchedMedia, PlayurlError> {
    let params: Vec<(String, String)> = vec![
        ("avid".into(), aid.to_string()),
        ("cid".into(), cid.to_string()),
        ("qn".into(), qn.to_string()),
        ("fnver".into(), "0".into()),
        ("fnval".into(), "16".into()),
        ("fourk".into(), "1".into()),
        ("ep_id".into(), ep_id.to_string()),
    ];
    let payload: Value = client.get_json(&apis.cheese, &params).await?;
    check_response(&payload)?;
    let body = take_body(&payload, "data")?;
    Ok(FetchedMedia {
        parser_type: "cheese",
        media_type: detect_media_type(&body),
        info_data: with_context(body, "cheese", &apis.cheese, &params),
    })
}

pub async fn fetch_lesson_info(
    client: &HttpClient,
    apis: &PlayurlApis,
    course_id: i64,
    lesson_id: i64,
    item_id: i64,
    section_id: i64,
) -> Result<FetchedMedia, PlayurlError> {
    let body = json!({
        "courseId": course_id,
        "lessonId": lesson_id,
        "itemId": item_id,
        "sectionId": section_id,
    });
    let payload: Value = client.post_json(&apis.lesson, &[], &body).await?;
    check_response(&payload)?;
    let data = take_body(&payload, "data")?;
    build_lesson_media_info(data)
}

/// `build_lesson_media_info` (lesson.py): synthesize a durl-only mp4 payload.
fn build_lesson_media_info(data: Value) -> Result<FetchedMedia, PlayurlError> {
    let video_url = data
        .get("videoUrl")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if video_url.is_empty() {
        return Err(PlayurlError::Api("接口未返回该小节的播放地址".into()));
    }
    let video_time = data.get("videoTime").and_then(Value::as_i64).unwrap_or(0);
    let info = json!({
        "format": "mp4",
        "quality": LESSON_VIDEO_QUALITY_ID,
        "accept_quality": [LESSON_VIDEO_QUALITY_ID],
        "timelength": 0,
        "durl": [{
            "order": 1,
            "length": video_time * 1000,
            "size": 0,
            "url": video_url,
            "backup_url": [],
        }],
    });
    Ok(FetchedMedia {
        parser_type: "lesson",
        media_type: MediaType::Mp4,
        info_data: info,
    })
}

pub async fn fetch_audio_info(
    client: &HttpClient,
    apis: &PlayurlApis,
    sid: i64,
) -> Result<FetchedMedia, PlayurlError> {
    let params: Vec<(String, String)> = vec![
        ("sid".into(), sid.to_string()),
        ("privilege".into(), "2".into()),
        ("quality".into(), "2".into()),
    ];
    let payload: Value = client.get_json(&apis.audio, &params).await?;
    check_response(&payload)?;
    let mut body = take_body(&payload, "data")?;
    // Python forces format = "m4a" after the fetch (both preview + download).
    body["format"] = Value::String("m4a".into());
    Ok(FetchedMedia {
        parser_type: "audio",
        media_type: MediaType::M4a,
        info_data: with_context(body, "audio", &apis.audio, &params),
    })
}

/// DRM guard (preview post-process).
pub fn check_drm(info_data: &Value) -> Result<(), PlayurlError> {
    let drm = info_data
        .get("is_drm")
        .map(|v| !v.is_null() && v != &Value::Bool(false) && v.as_i64() != Some(0))
        .unwrap_or(false);
    if drm {
        Err(PlayurlError::Api("不支持下载受 DRM 保护的媒体".into()))
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Declared quality map (quality.py `parse_declared_quality_map`)
// ---------------------------------------------------------------------------

/// quality id → declared codec ids, sorted quality DESC. Built from
/// `support_formats` capped at the account ceiling (highest id actually present
/// in `dash.video`); `None` when `support_formats`/`dash.video` are missing or
/// empty — the caller then falls back to the `dash.video`-keyed lists (the
/// bangumi/cheese interfaces never declare, so they always take that path).
pub fn parse_declared_quality_map(info_data: &Value) -> Option<Vec<(i64, Vec<i64>)>> {
    let formats = info_data
        .get("support_formats")?
        .as_array()
        .filter(|a| !a.is_empty())?;
    let dash_video = info_data
        .get("dash")?
        .get("video")?
        .as_array()
        .filter(|a| !a.is_empty())?;
    let highest = dash_video
        .iter()
        .filter_map(|n| n.get("id").and_then(Value::as_i64))
        .max()?;

    let mut out: Vec<(i64, Vec<i64>)> = Vec::new();
    for f in formats {
        let Some(q) = f.get("quality").and_then(Value::as_i64) else {
            continue;
        };
        if q > highest || out.iter().any(|(id, _)| *id == q) {
            continue;
        }
        let mut codecs = Vec::new();
        for codec in f
            .get("codecs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(prefix) = codec
                .as_str()
                .map(|c| c.split('.').next().unwrap_or_default())
            else {
                continue;
            };
            if let Some(&id) = CODEC_PREFIX_MAP.get(prefix) {
                if !codecs.contains(&id) {
                    codecs.push(id);
                }
            }
        }
        out.push((q, codecs));
    }
    if out.is_empty() {
        return None;
    }
    out.sort_by_key(|(id, _)| std::cmp::Reverse(*id));
    Some(out)
}

// ---------------------------------------------------------------------------
// Available streams (preview worker.py / download video_info.py)
// ---------------------------------------------------------------------------

/// quality id → insertion-ordered (codec id, node) pairs; the Vec preserves
/// codec order for the `codec_list[0]` fallback.
pub type VideoInfoMap = HashMap<i64, Vec<(i64, Value)>>;

#[derive(Debug, Default)]
pub struct AvailableStreams {
    pub quality_ids: Vec<i64>,
    pub video_map: VideoInfoMap,
    pub audio_ids: Vec<i64>,
    pub audio_map: HashMap<i64, Value>,
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        Value::String(s) => !s.is_empty(),
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
    }
}

pub fn build_available_streams(
    parser_type: &str,
    media_type: MediaType,
    info_data: &Value,
) -> AvailableStreams {
    let mut out = AvailableStreams::default();

    match media_type {
        MediaType::Dash => {
            let videos = info_data
                .get("dash")
                .and_then(|d| d.get("video"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for node in &videos {
                let (Some(id), Some(codec)) = (
                    node.get("id").and_then(Value::as_i64),
                    node.get("codecid").and_then(Value::as_i64),
                ) else {
                    continue;
                };
                out.video_map
                    .entry(id)
                    .or_default()
                    .push((codec, node.clone()));
            }
            out.quality_ids = match parser_type {
                "video" => parse_declared_quality_map(info_data)
                    .map(|d| d.into_iter().map(|(id, _)| id).collect()),
                _ => None,
            }
            .unwrap_or_else(|| {
                let mut ids: Vec<i64> = out.video_map.keys().copied().collect();
                ids.sort_unstable_by(|a, b| b.cmp(a));
                ids
            });
        }
        MediaType::Mp4 | MediaType::Flv => {
            for id in info_data
                .get("accept_quality")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
            {
                let Some(id) = id.as_i64() else { continue };
                out.quality_ids.push(id);
                out.video_map.insert(
                    id,
                    vec![(
                        7,
                        json!({
                            "id": id,
                            "codecid": 7,
                            "frame_rate": 0,
                            "bandwidth": 0,
                            "timelength": 0,
                            "url_entry_list": info_data.get("durl").cloned().unwrap_or(Value::Null),
                        }),
                    )],
                );
            }
        }
        MediaType::Unknown | MediaType::M4a => {}
    }

    match media_type {
        MediaType::Dash => {
            let dash = info_data.get("dash");
            // flac: node forced to id 30251.
            if let Some(node) = dash
                .and_then(|d| d.get("flac"))
                .and_then(|f| f.get("audio"))
                .filter(|v| truthy(v))
            {
                out.audio_map.insert(FLAC_QUALITY_ID, node.clone());
                out.audio_ids.push(FLAC_QUALITY_ID);
            }
            // dolby: first entry, raw 30255 unified to 30250.
            if let Some(node) = dash
                .and_then(|d| d.get("dolby"))
                .and_then(|d| d.get("audio"))
                .and_then(Value::as_array)
                .filter(|a| !a.is_empty())
                .and_then(|a| a.first())
            {
                out.audio_map.insert(DOLBY_QUALITY_ID, node.clone());
                out.audio_ids.push(DOLBY_QUALITY_ID);
            }
            if let Some(list) = dash.and_then(|d| d.get("audio")).and_then(Value::as_array) {
                for node in list {
                    let Some(id) = node.get("id").and_then(Value::as_i64) else {
                        continue;
                    };
                    if !out.audio_ids.contains(&id) {
                        out.audio_ids.push(id);
                        out.audio_map.insert(id, node.clone());
                    }
                }
            }
        }
        MediaType::M4a => {
            out.audio_map.insert(
                M4A_QUALITY_ID,
                json!({
                    "id": M4A_QUALITY_ID,
                    "codecs": "mp4a.40.2",
                    "backup_url": info_data.get("cdns").cloned().unwrap_or(Value::Null),
                    "bandwidth": 192000,
                }),
            );
            out.audio_ids.push(M4A_QUALITY_ID);
        }
        _ => {}
    }

    out
}

/// `get_available_codec_list(qid)` (both sides): the codecs actually present
/// for that quality, else the declared codec list for it.
pub fn available_codecs(
    streams: &AvailableStreams,
    declared: Option<&[(i64, Vec<i64>)]>,
    qid: i64,
) -> Vec<i64> {
    if let Some(entries) = streams.video_map.get(&qid).filter(|e| !e.is_empty()) {
        return entries.iter().map(|(c, _)| *c).collect();
    }
    declared
        .and_then(|d| d.iter().find(|(id, _)| *id == qid))
        .map(|(_, codecs)| codecs.clone())
        .unwrap_or_default()
}

/// Preview-side audio ordering: `audio_reorder_map` rank, unknown ids first
/// (Python's None-first sort).
pub fn sort_audio_ids_preview(ids: &mut [i64]) {
    ids.sort_by_key(|id| AUDIO_REORDER_MAP.get(id).copied().unwrap_or(-1));
}

// ---------------------------------------------------------------------------
// Selection (preview worker.py `_resolve_target`)
// ---------------------------------------------------------------------------

fn pick_by_priority(priority: &[i64], available: &[i64], fallback: i64) -> i64 {
    priority
        .iter()
        .find(|p| available.contains(p))
        .copied()
        .unwrap_or(fallback)
}

/// Resolve a requested quality (`QUALITY_AUTO` = best by priority) against the
/// available list; falls back to the first (best) available entry.
pub fn resolve_quality(available: &[i64], requested: i64, priority: &[i64]) -> Option<i64> {
    if available.is_empty() {
        return None;
    }
    Some(if requested == QUALITY_AUTO {
        pick_by_priority(priority, available, available[0])
    } else if available.contains(&requested) {
        requested
    } else {
        available[0]
    })
}

/// Resolve a requested codec (`CODEC_AUTO` = best by priority); falls back to
/// the first available codec.
pub fn resolve_codec(available: &[i64], requested: i64, priority: &[i64]) -> Option<i64> {
    if available.is_empty() {
        return None;
    }
    Some(
        if requested == CODEC_AUTO || !available.contains(&requested) {
            pick_by_priority(priority, available, available[0])
        } else {
            requested
        },
    )
}

// ---------------------------------------------------------------------------
// Supplement (preview worker.py `start_video_parse`)
// ---------------------------------------------------------------------------

/// `fetch_video_streams`: video playurl GET (fnval 4048) for one qn; empty on
/// non-zero code or transport error, like Python.
pub async fn fetch_video_streams(
    client: &HttpClient,
    apis: &PlayurlApis,
    bvid: &str,
    cid: i64,
    qn: i64,
) -> Vec<Value> {
    let params: Vec<(String, String)> = vec![
        ("bvid".into(), bvid.into()),
        ("cid".into(), cid.to_string()),
        ("qn".into(), qn.to_string()),
        ("fnver".into(), "0".into()),
        ("fnval".into(), "4048".into()),
        ("fourk".into(), "1".into()),
    ];
    let Ok(payload) = client.get_json::<Value>(&apis.video, &params).await else {
        return Vec::new();
    };
    if payload.get("code").and_then(Value::as_i64).unwrap_or(0) != 0 {
        return Vec::new();
    }
    payload
        .get("data")
        .and_then(|d| d.get("dash"))
        .and_then(|d| d.get("video"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// `merge_video_streams`: fill only slots (id, codecid) not already present.
pub fn merge_video_streams(map: &mut VideoInfoMap, streams: &[Value]) {
    for node in streams {
        let (Some(id), Some(codec)) = (
            node.get("id").and_then(Value::as_i64),
            node.get("codecid").and_then(Value::as_i64),
        ) else {
            continue;
        };
        let entry = map.entry(id).or_default();
        if !entry.iter().any(|(c, _)| *c == codec) {
            entry.push((codec, node.clone()));
        }
    }
}

// ---------------------------------------------------------------------------
// Codec fallback warning (translator.py:355 + preview/download video_info.py)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct CodecFallbackWarning {
    pub requested_codec: i64,
    pub actual_codec: i64,
    pub message: String,
}

impl CodecFallbackWarning {
    pub fn category(&self) -> &'static str {
        "WARNING"
    }
}

static WARNED: LazyLock<Mutex<HashSet<(i64, i64)>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Deduped process-global codec-fallback warning; `None` when nothing was
/// requested, `auto` was requested, the codec matched, or the pair was already
/// reported.
pub fn warn_codec_fallback(requested: Option<i64>, actual: i64) -> Option<CodecFallbackWarning> {
    let requested = requested?;
    if requested == CODEC_AUTO || requested == actual {
        return None;
    }
    if !WARNED.lock().insert((requested, actual)) {
        return None;
    }
    let message = format!(
        "This video has no {}, using {}",
        codec_label(requested).unwrap_or_default(),
        codec_label(actual).unwrap_or_default()
    );
    Some(CodecFallbackWarning {
        requested_codec: requested,
        actual_codec: actual,
        message,
    })
}

pub fn clear_warned_for_test() {
    WARNED.lock().clear();
}

// ---------------------------------------------------------------------------
// URL extraction + file sizes (worker.py / query_worker.py)
// ---------------------------------------------------------------------------

/// The 5 unique url keys of worker.py's stream-url extraction.
const URL_KEYS: [&str; 5] = ["baseUrl", "base_url", "backupUrl", "backup_url", "url"];

pub fn extract_stream_urls(node: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for key in URL_KEYS {
        match node.get(key) {
            Some(Value::Array(list)) => {
                out.extend(list.iter().filter_map(Value::as_str).map(str::to_string))
            }
            Some(Value::String(s)) => out.push(s.clone()),
            _ => {}
        }
    }
    out
}

/// Preview file-size probe (worker.py: min 10240).
pub async fn probe_file_size(urls: &[String], cdn: &CdnConfig) -> Option<u64> {
    let probe = HttpClient::probe(ClientOptions::default()).ok()?;
    resolve_download_url(&probe, urls, cdn, 10240)
        .await
        .ok()
        .map(|r| r.file_size)
}

/// Download-side stream URL resolution (query_worker.py: min 1024).
pub async fn query_dash_url(
    urls: &[String],
    cdn: &CdnConfig,
) -> Result<ResolvedUrl, AllCandidatesFailed> {
    let probe = HttpClient::probe(ClientOptions::default())
        .map_err(|_| AllCandidatesFailed { total: 0, hosts: 0 })?;
    resolve_download_url(&probe, urls, cdn, 1024).await
}

/// MP4/FLV preview: re-query with the selected qn swapped into `query_url`,
/// summing durl sizes/lengths. `data` vs `result` per parser type.
pub async fn query_mp4_info(
    client: &HttpClient,
    parser_type: &str,
    info_data: &Value,
    qid: i64,
) -> Result<(u64, i64), PlayurlError> {
    let query_url = info_data
        .get("query_url")
        .and_then(Value::as_str)
        .ok_or_else(|| PlayurlError::Api("missing query_url".into()))?;
    let qn_re = regex::Regex::new(r"([?&])qn=\d+").unwrap();
    let rewritten = qn_re.replace(query_url, format!("${{1}}qn={qid}"));
    let payload: Value = client.get_json(&rewritten, &[]).await?;
    check_response(&payload)?;
    let body = take_body(
        &payload,
        if parser_type == "bangumi" {
            "result"
        } else {
            "data"
        },
    )?;
    let mut file_size = 0u64;
    let mut timelength = 0i64;
    if let Some(durl) = body.get("durl").and_then(Value::as_array) {
        for part in durl {
            file_size += part.get("size").and_then(Value::as_u64).unwrap_or(0);
            timelength += part.get("length").and_then(Value::as_i64).unwrap_or(0);
        }
    }
    Ok((file_size, timelength))
}

/// `check_is_full_video` (both sides): DASH is always full; MP4/FLV compare
/// timelengths.
pub fn check_is_full_video(media_type: MediaType, info_data: &Value, media_info: &Value) -> bool {
    match media_type {
        MediaType::Dash => true,
        MediaType::Mp4 | MediaType::Flv => {
            info_data
                .get("timelength")
                .and_then(Value::as_i64)
                .unwrap_or(0)
                == media_info
                    .get("timelength")
                    .and_then(Value::as_i64)
                    .unwrap_or(0)
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Download-side pure helpers (tickets 13/16 consume these)
// ---------------------------------------------------------------------------

pub fn stream_file_ext(media_type: MediaType) -> &'static str {
    match media_type {
        MediaType::Dash => "m4s",
        MediaType::Mp4 => "mp4",
        MediaType::Flv => "flv",
        _ => "m4a",
    }
}

pub fn audio_file_ext(quality_id: i64) -> &'static str {
    match quality_id {
        FLAC_QUALITY_ID => "flac",
        DOLBY_QUALITY_ID => "ec3",
        _ => "m4a",
    }
}

pub fn video_file_name(task_id: &str, media_type: MediaType) -> String {
    format!("video_{task_id}.{}", stream_file_ext(media_type))
}

pub fn video_part_name(task_id: &str, index: usize, media_type: MediaType) -> String {
    format!("video_{task_id}_{index}.{}", stream_file_ext(media_type))
}

pub fn audio_file_name(task_id: &str, quality_id: i64) -> String {
    format!("audio_{task_id}.{}", audio_file_ext(quality_id))
}

pub const FILE_KEY_VIDEO: &str = "video";
pub const FILE_KEY_VIDEO_PART: &str = "video_part";
pub const FILE_KEY_AUDIO: &str = "audio";

// ---------------------------------------------------------------------------
// Dispatch helper (previewer.py `_fetch_media_info_by_type`)
// ---------------------------------------------------------------------------

/// Dispatch on the item attribute. `task_qn` is the download-side request
/// quality; the preview passes `None` to use the per-type preview qn.
pub async fn fetch_media_info(
    client: &HttpClient,
    apis: &PlayurlApis,
    item: &TreeItem,
    task_qn: Option<i64>,
) -> Result<FetchedMedia, PlayurlError> {
    let attr = item.attribute;
    if attr.contains(Attribute::VIDEO_BIT) {
        fetch_video_info(
            client,
            apis,
            &item.bvid,
            item.cid,
            task_qn.unwrap_or(PREVIEW_VIDEO_QN),
        )
        .await
    } else if attr.contains(Attribute::BANGUMI_BIT) {
        fetch_bangumi_info(
            client,
            apis,
            &item.bvid,
            item.cid,
            task_qn.unwrap_or(PREVIEW_BANGUMI_QN),
        )
        .await
    } else if attr.contains(Attribute::CHEESE_BIT) {
        fetch_cheese_info(
            client,
            apis,
            item.aid,
            item.cid,
            task_qn.unwrap_or(PREVIEW_CHEESE_QN),
            item.ep_id,
        )
        .await
    } else if attr.contains(Attribute::LESSON_BIT) {
        fetch_lesson_info(
            client,
            apis,
            item.course_id,
            item.lesson_id,
            item.item_id,
            item.section_id,
        )
        .await
    } else if attr.contains(Attribute::AUDIO_BIT) {
        fetch_audio_info(client, apis, item.sid).await
    } else {
        Err(PlayurlError::Api("未知内容类型".into()))
    }
}
