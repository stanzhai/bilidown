//! List-family parsers (ticket 10): space pages, favorites folders,
//! collection/series lists, watch-later, history, weekly popular — plus the
//! pagination metadata and server-side keyword search those lists carry.
//!
//! Ports `src/util/parse/parser/{space,favlist,list,watch_later,history,
//! popular}.py` and the shared episode skeleton
//! `src/util/parse/episode/list_base.py`. Every parser fetches one page of
//! its list and folds the API's flat archive array into one [`TreeItem`]
//! node whose children are the downloadable episodes; the paging info the
//! UI's pager consumes rides along in [`ExtraData`].
//!
//! Tests pin the request shapes (params, page sizes, keyword param names)
//! and the field mapping against wiremock fixtures mirroring the real API.

use std::collections::HashMap;
use std::sync::LazyLock;

use parking_lot::Mutex;
use serde_json::{json, Map, Value};

use crate::tree::{Attribute, EpisodeData, Number, TreeItem};

/// Page size of the space arc-search endpoint (`SpaceParser.ps`).
pub const SPACE_PS: i64 = 40;
/// Page size of the favorites endpoint (`FavlistParser.ps`).
pub const FAVLIST_PS: i64 = 40;
/// Page size of the collection/series endpoints (`ListParser.ps`).
pub const LIST_PS: i64 = 30;
/// Page size of the watch-later endpoint (`WatchLaterParser.ps`).
pub const WATCH_LATER_PS: i64 = 20;
/// Page size of the history endpoint (`HistoryParser.ps`).
pub const HISTORY_PS: i64 = 20;

// Endpoint hosts/paths (constants keep the wiremock fixtures readable).
pub const SPACE_ARC_API: &str = "https://api.bilibili.com/x/space/wbi/arc/search";
pub const CARD_API: &str = "https://api.bilibili.com/x/web-interface/card";
pub const FAV_RESOURCE_API: &str = "https://api.bilibili.com/x/v3/fav/resource/list";
pub const SERIES_API: &str = "https://api.bilibili.com/x/series/series";
pub const SERIES_ARCHIVES_API: &str = "https://api.bilibili.com/x/series/archives";
pub const SEASONS_ARCHIVES_API: &str =
    "https://api.bilibili.com/x/polymer/web-space/seasons_archives_list";
pub const VIEW_API: &str = "https://api.bilibili.com/x/web-interface/wbi/view";
pub const TOVIEW_API: &str = "https://api.bilibili.com/x/v2/history/toview/web";
pub const HISTORY_API: &str = "https://api.bilibili.com/x/web-interface/history/search";
pub const WEEKLY_API: &str = "https://api.bilibili.com/x/web-interface/popular/series/one";

// Category labels the parse list shows (`Translator.EPISODE_TYPE` zh_CN).
pub const PROFILE_CATEGORY: &str = "个人空间";
pub const FAVORITES_CATEGORY: &str = "收藏夹";
pub const COLLECTION_CATEGORY: &str = "合集";
pub const WEEKLY_CATEGORY: &str = "每周必看";
pub const WATCH_LATER_CATEGORY: &str = "稍后再看";
pub const HISTORY_CATEGORY: &str = "历史记录";

// Item badges (`SpaceEpisodeParser`/`FavlistEpisodeParser` hard-coded
// zh_CN labels; the badge column is not translated).
pub const BADGE_CHARGING: &str = "充电专属";
pub const BADGE_LESSON: &str = "课程";
pub const BADGE_UNION: &str = "合作";
pub const BADGE_MULTI_PART: &str = "分P";
pub const BADGE_EXPIRED: &str = "已失效";

/// Errors of the list parsers. `Api` mirrors `check_response`: non-zero
/// business code with the server's message.
#[derive(Debug, thiserror::Error)]
pub enum ListError {
    #[error("network: {0}")]
    Network(#[from] network::HttpError),
    #[error("{0}")]
    Api(String),
    /// The link carries no usable list identity (`find_str` with check).
    #[error("无效的链接")]
    InvalidLink,
    /// Watch-later/history require a logged-in, unexpired session.
    #[error("请先登录账号")]
    LoginRequired,
    /// A play-page link whose video belongs to no collection/series.
    #[error("无效的链接")]
    NoListInLink,
}

/// Pagination block the pager widget consumes (`get_extra_data`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pagination {
    pub total_pages: i64,
    pub total_items: i64,
    pub current_page: i64,
}

/// What a list parse hands the UI besides the tree
/// (`ParserBase.get_extra_data`).
#[derive(Debug, Clone, Default)]
pub struct ExtraData {
    pub pagination: Option<Pagination>,
    pub server_search: bool,
    pub keyword: String,
}

/// One page parse result: the category it parsed as, the wrapper node
/// (root → category node → episodes), the extra data, and — when the link
/// points at one specific item of the list — the locate key for the view
/// model (`(field, value)`).
#[derive(Debug)]
pub struct ParseOutput {
    pub category: &'static str,
    pub node: TreeItem,
    pub extra: ExtraData,
    pub current_episode: Option<(String, String)>,
}

/// `check_response`: non-zero business code is an error carrying the
/// server's message.
pub fn check_response(response: &Value) -> Result<(), ListError> {
    if response.get("code").and_then(Value::as_i64).unwrap_or(-1) != 0 {
        return Err(ListError::Api(
            response
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("未知错误")
                .to_owned(),
        ));
    }
    Ok(())
}

fn total_pages(count: i64, ps: i64) -> i64 {
    // Python math.ceil(count / ps) — integer division rounds the wrong way
    // for negatives, so do the ceiling explicitly.
    (count + ps - 1) / ps
}

fn paged_extra(count: i64, ps: i64, pn: i64, server_search: bool, keyword: &str) -> ExtraData {
    ExtraData {
        pagination: Some(Pagination {
            total_pages: total_pages(count, ps),
            total_items: count,
            current_page: pn,
        }),
        server_search,
        keyword: keyword.to_owned(),
    }
}

/// `with_search_keyword`: tag the keyword onto a node title so the parse
/// list and parse history can tell search results apart. History and
/// watch-later roots have no own title, so the keyword alone becomes it.
pub fn with_search_keyword(title: &str, keyword: &str) -> String {
    if keyword.is_empty() {
        return title.to_owned();
    }
    let label = format!("搜索: {keyword}");
    if title.is_empty() {
        label
    } else {
        format!("{title} - {label}")
    }
}

/// `Units.unformat_episode_duration`: "HH:MM:SS"/"MM:SS"/"SS" → seconds.
fn unformat_duration(text: &str) -> i64 {
    let parts: Vec<i64> = text
        .split(':')
        .map(|p| p.trim().parse::<i64>().unwrap_or(0))
        .collect();
    match parts.len() {
        3 => parts[0] * 3600 + parts[1] * 60 + parts[2],
        2 => parts[0] * 60 + parts[1],
        1 => parts[0],
        _ => 0,
    }
}

/// Duration field of a list item (`get_episode_duration`): `duration`
/// (seconds), else `arc.duration`, else `length` ("MM:SS" text), else 0.
fn episode_duration(episode: &Value) -> i64 {
    if let Some(d) = episode.get("duration") {
        return d.as_i64().unwrap_or(0);
    }
    if let Some(d) = episode.get("arc").and_then(|a| a.get("duration")) {
        return d.as_i64().unwrap_or(0);
    }
    if let Some(l) = episode.get("length").and_then(Value::as_str) {
        return unformat_duration(l);
    }
    0
}

/// Episode side-data registry shared by the parsers in this process
/// (Python's process-global `EpisodeData.table`); one instance per app.
pub struct ListParseContext {
    pub episode_data: EpisodeData,
    uname_cache: Mutex<HashMap<i64, String>>,
}

impl Default for ListParseContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ListParseContext {
    pub fn new() -> Self {
        Self {
            episode_data: EpisodeData::new(),
            uname_cache: Mutex::new(HashMap::new()),
        }
    }

    /// Register a fresh episode and return its id (`EpisodeData.add_episode`).
    pub fn add_episode(&self) -> String {
        self.episode_data.add_episode()
    }

    fn set_uname(&self, mid: i64, name: &str) {
        self.uname_cache.lock().insert(mid, name.to_owned());
    }

    fn uname(&self, mid: i64) -> Option<String> {
        self.uname_cache.lock().get(&mid).cloned()
    }
}

// ---------------------------------------------------------------------------
// Space (个人空间)
// ---------------------------------------------------------------------------

/// Space page fetch (`SpaceParser`): arc search (wbi-signed) + uname from
/// the card endpoint, cached per mid so paging doesn't re-request it.
pub async fn parse_space(
    client: &network::HttpClient,
    base: &str,
    ctx: &ListParseContext,
    url: &str,
    pn: i64,
) -> Result<ParseOutput, ListError> {
    let mid = space_mid(url).ok_or(ListError::InvalidLink)?;
    let keyword = urls::search::extract_keyword(url).unwrap_or_default();

    let params = space_params(mid, pn, &keyword);
    let response: Value = client.get_json(&swap_host(SPACE_ARC_API, base), &params).await?;
    check_response(&response)?;

    let data = &response["data"];
    let uname = match ctx.uname(mid) {
        Some(name) => name,
        None => {
            let card: Value = client
                .get_json(
                    &swap_host(CARD_API, base),
                    &[("mid".to_owned(), mid.to_string())],
                )
                .await?;
            check_response(&card)?;
            let name = card["data"]["card"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            ctx.set_uname(mid, &name);
            name
        }
    };

    let episode_id = ctx.add_episode();
    ctx.episode_data.set_episode_data(
        &episode_id,
        json!({
            "space_owner": uname,
            "space_owner_id": mid,
        }),
    );

    let vlist = data["list"]["vlist"].as_array();
    let count = data["page"]["count"].as_i64().unwrap_or(0);

    let mut node = TreeItem::root(
        Number::Text(PROFILE_CATEGORY.to_owned()),
        &with_search_keyword(&uname, &keyword),
    );
    node.set_attribute(Attribute::TREE_NODE_BIT);

    let mut number = 0;
    for episode in vlist.map(Vec::as_slice).unwrap_or_default() {
        number += 1;
        let mut item = TreeItem::empty();
        item.aid = episode["aid"].as_i64().unwrap_or(0);
        item.bvid = episode["bvid"].as_str().unwrap_or_default().to_owned();
        item.cover = episode["pic"].as_str().unwrap_or_default().to_owned();
        item.duration = episode_duration(episode);
        item.ep_id = episode["season_id"].as_i64().unwrap_or(0);
        item.episode_id = episode_id.clone();
        item.number = Number::Int(number);
        item.pubtime = episode["created"].as_i64().unwrap_or(0);
        item.title = episode["title"].as_str().unwrap_or_default().to_owned();
        item.url = format!("https://www.bilibili.com/video/{}", item.bvid);
        item.badge = space_badge(episode).to_owned();

        if episode["is_lesson_video"].as_bool().unwrap_or(false) {
            item.set_attribute(Attribute::CHEESE_BIT);
        } else {
            item.set_attribute(Attribute::VIDEO_BIT);
        }
        item.set_attribute(Attribute::SPACE_BIT | Attribute::NEED_PARSE_BIT);
        node.add_child(item);
    }

    let mut wrapper = TreeItem::empty();
    wrapper.add_child(node);

    Ok(ParseOutput {
        category: PROFILE_CATEGORY,
        node: wrapper,
        extra: paged_extra(count, SPACE_PS, pn, true, &keyword),
        current_episode: None,
    })
}

/// Space arc-search request params (`get_search_arc_info`), verbatim.
pub fn space_params(mid: i64, pn: i64, keyword: &str) -> Vec<(String, String)> {
    vec![
        ("pn".into(), pn.to_string()),
        ("ps".into(), SPACE_PS.to_string()),
        ("tid".into(), "0".into()),
        ("special_type".into(), String::new()),
        ("order".into(), "pubdate".into()),
        ("mid".into(), mid.to_string()),
        ("index".into(), "0".into()),
        ("keyword".into(), keyword.to_owned()),
        ("order_avoided".into(), "true".into()),
        ("platform".into(), "web".into()),
        ("web_location".into(), "333.1387".into()),
        ("dm_img_list".into(), "[]".into()),
        (
            "dm_img_str".into(),
            "V2ViR0wgMS4wIChPcGVuR0wgRVMgMi4wIENocm9taXVtKQ".into(),
        ),
        (
            "dm_cover_img_str".into(),
            "QU5HTEUgKE5WSURJQSwgTlZJRElBIEdlRm9yY2UgUlRYIDQwNjAgTGFwdG9wIEdQVSAoMHgwMDAwMjhFMCkgRGlyZWN0M0QxMSB2c181XzAgcHNfNV8wLCBEM0QxMSlHb29nbGUgSW5jLiAoTlZJRElBKQ".into(),
        ),
        (
            "dm_img_inter".into(),
            r#"{"ds":[],"wh":[3688,4546,12],"of":[119,238,119]}"#.into(),
        ),
    ]
}

fn space_badge(episode: &Value) -> &'static str {
    if episode["is_charging_arc"].as_bool().unwrap_or(false) {
        BADGE_CHARGING
    } else if episode["is_lesson_video"].as_bool().unwrap_or(false) {
        BADGE_LESSON
    } else if episode["is_union_video"].as_bool().unwrap_or(false) {
        BADGE_UNION
    } else {
        ""
    }
}

/// `SpaceParser.get_mid`: first number of the path only, so query params
/// (e.g. a search keyword) can't干扰 the uid.
fn space_mid(url: &str) -> Option<i64> {
    static RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"/([0-9]+)").unwrap());
    let path = url::Url::parse(url)
        .map(|u| u.path().to_owned())
        .unwrap_or_else(|_| url.to_owned());
    RE.captures(&path)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
}

// ---------------------------------------------------------------------------
// Favorites folder (收藏夹)
// ---------------------------------------------------------------------------

/// Favorites page fetch (`FavlistParser`): keyword goes to the endpoint's
/// own `keyword` param — server-side search.
pub async fn parse_favlist(
    client: &network::HttpClient,
    base: &str,
    ctx: &ListParseContext,
    url: &str,
    pn: i64,
) -> Result<ParseOutput, ListError> {
    let media_id = favlist_media_id(url).ok_or(ListError::InvalidLink)?;
    let keyword = urls::search::extract_keyword(url).unwrap_or_default();

    let params = vec![
        ("media_id".into(), media_id.to_string()),
        ("pn".into(), pn.to_string()),
        ("ps".into(), FAVLIST_PS.to_string()),
        ("keyword".into(), keyword.clone()),
        ("order".into(), "mtime".into()),
        ("type".into(), "0".into()),
        ("tid".into(), "0".into()),
        ("platform".into(), "web".into()),
        ("web_location".into(), "333.1387".into()),
    ];
    let response: Value = client
        .get_json(&swap_host(FAV_RESOURCE_API, base), &params)
        .await?;
    check_response(&response)?;

    let data = &response["data"];
    let title = data["info"]["title"].as_str().unwrap_or_default().to_owned();

    let episode_id = ctx.add_episode();
    ctx.episode_data.set_episode_data(
        &episode_id,
        json!({
            "favorites_name": title,
            "favorites_id": data["info"]["id"].as_i64().unwrap_or(0),
            "favorites_owner": data["info"]["upper"]["name"].as_str().unwrap_or_default(),
            "favorites_owner_id": data["info"]["upper"]["mid"].as_i64().unwrap_or(0),
        }),
    );

    let medias = data["medias"].as_array();
    let count = data["info"]["media_count"].as_i64().unwrap_or(0);

    let mut node = TreeItem::root(
        Number::Text(FAVORITES_CATEGORY.to_owned()),
        &with_search_keyword(&title, &keyword),
    );
    node.set_attribute(Attribute::TREE_NODE_BIT);

    let mut number = 0;
    for episode in medias.map(Vec::as_slice).unwrap_or_default() {
        number += 1;
        let mut item = TreeItem::empty();
        item.badge = favlist_badge(episode);
        item.bvid = episode["bvid"].as_str().unwrap_or_default().to_owned();
        item.cover = episode["cover"].as_str().unwrap_or_default().to_owned();
        item.duration = episode_duration(episode);
        item.ep_id = episode["id"].as_i64().unwrap_or(0);
        item.episode_id = episode_id.clone();
        item.number = Number::Int(number);
        item.pubtime = episode["pubtime"].as_i64().unwrap_or(0);
        item.favtime = episode["fav_time"].as_i64().unwrap_or(0);
        item.title = favlist_title(episode);
        item.url = format!("https://www.bilibili.com/video/{}", item.bvid);

        if episode.get("ogv").is_some() {
            item.set_attribute(Attribute::BANGUMI_BIT);
        } else {
            item.set_attribute(Attribute::VIDEO_BIT);
        }
        if episode["page"].as_i64().unwrap_or(0) > 1 {
            item.set_attribute(Attribute::FAVORITE_WITH_MULTI_PART_VIDEO_BIT);
        }
        item.set_attribute(Attribute::FAVLIST_BIT | Attribute::NEED_PARSE_BIT);
        node.add_child(item);
    }

    let mut wrapper = TreeItem::empty();
    wrapper.add_child(node);

    Ok(ParseOutput {
        category: FAVORITES_CATEGORY,
        node: wrapper,
        extra: paged_extra(count, FAVLIST_PS, pn, true, &keyword),
        current_episode: None,
    })
}

fn favlist_badge(episode: &Value) -> String {
    if let Some(ogv) = episode.get("ogv") {
        return ogv["type_name"].as_str().unwrap_or_default().to_owned();
    }
    if episode["page"].as_i64().unwrap_or(0) > 1 {
        return BADGE_MULTI_PART.to_owned();
    }
    String::new()
}

fn favlist_title(episode: &Value) -> String {
    if let Some(ogv) = episode.get("ogv") {
        let title = ogv["title"].as_str().unwrap_or_default();
        let intro = ogv["intro"].as_str().unwrap_or_default();
        return format!("{title} - {intro}");
    }
    episode["title"].as_str().unwrap_or_default().to_owned()
}

/// `FavlistParser.get_media_id`: `ml{id}` in the path or `fid=` in the
/// query.
fn favlist_media_id(url: &str) -> Option<i64> {
    static ML: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"ml(\d+)").unwrap());
    static FID: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"fid=(\d+)").unwrap());
    ML.captures(url)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
        .or_else(|| {
            FID.captures(url)
                .and_then(|c| c.get(1))
                .and_then(|m| m.as_str().parse().ok())
        })
}

// ---------------------------------------------------------------------------
// Collection / series (合集列表)
// ---------------------------------------------------------------------------

/// Collection/series page fetch (`ListParser.parse`).
///
/// Three link shapes:
/// - `space/{mid}/lists/{id}?type=season|series` — the family is explicit;
/// - play-page `bilibili.com/list/{mid}?sid=…&oid=…&bvid=…` — identity
///   comes from the link's video (reverse lookup of its `ugc_season`) or
///   from `sid` probing the series endpoint first (a non-series id returns
///   code 147002; probing series avoids catching the season endpoint's
///   -404 as an exception, and the probe's meta is reused);
/// - index page with neither video nor sid — no list to parse.
pub async fn parse_list(
    client: &network::HttpClient,
    base: &str,
    ctx: &ListParseContext,
    url: &str,
    pn: i64,
) -> Result<ParseOutput, ListError> {
    let mut current_bvid: Option<String> = None;

    let (mid, archives, meta) = if let Some(family) = list_link_family(url) {
        let mid = list_mid(url).ok_or(ListError::InvalidLink)?;
        match family {
            ListFamily::Season => {
                let season_id = list_path_id(url).ok_or(ListError::InvalidLink)?;
                let (data, count) =
                    seasons_archives(client, base, mid, season_id, pn).await?;
                let meta = season_meta(&data);
                (data["mid"].as_i64().unwrap_or(mid), archives_page(&data, count), meta)
            }
            ListFamily::Series => {
                let series_id = list_path_id(url).ok_or(ListError::InvalidLink)?;
                let (data, count) = series_archives(client, base, mid, series_id, pn).await?;
                let meta = series_meta_request(client, base, series_id).await?;
                (mid, archives_page(&data, count), meta)
            }
        }
    } else {
        // Play page: reverse-lookup the video's collection when the link
        // has one, else probe `sid`, else there is no list in the link.
        let video_data = list_video_data(client, base, url).await?;
        let sid = query_u64(url, "sid");

        let season = video_data.as_ref().and_then(|v| v.get("ugc_season"));

        let season_matches = season
            .and_then(|s| s["id"].as_i64())
            .map_or(sid.is_none(), |season_id| sid.is_none_or(|sid| sid == season_id));
        if let Some(season) = season.filter(|_| season_matches) {
            let season_id = season["id"].as_i64().unwrap_or(0);
            let mid = season["mid"].as_i64().unwrap_or(0);
            let (data, count) = seasons_archives(client, base, mid, season_id, pn).await?;
            let meta = season_meta(&data);
            (mid, archives_page(&data, count), meta)
        } else if let Some(sid) = sid {
            // Probe series first: non-series ids answer 147002, and the
            // probe's meta is exactly what the series parse needs anyway.
            if let Some(meta) = series_meta_probe(client, base, sid).await? {
                let (data, count) = series_archives(client, base, meta_mid(&meta), sid, pn).await?;
                (meta_mid(&meta), archives_page(&data, count), meta)
            } else {
                let (data, count) = seasons_archives(client, base, list_mid(url).unwrap_or(0), sid, pn).await?;
                let meta = season_meta(&data);
                (data["mid"].as_i64().unwrap_or(0), archives_page(&data, count), meta)
            }
        } else if video_data.is_some() {
            return Err(ListError::NoListInLink);
        } else {
            return Err(ListError::InvalidLink);
        }
    };

    if let Some(video) = list_video_bvid(url) {
        current_bvid = Some(video);
    }

    let node_title = meta_title(&meta);
    let episode_id = ctx.add_episode();
    ctx.episode_data.set_episode_data(
        &episode_id,
        json!({ "collection_title": node_title, "space_owner_id": mid }),
    );

    let mut node = TreeItem::root(
        Number::Text(COLLECTION_CATEGORY.to_owned()),
        &node_title,
    );
    node.set_attribute(Attribute::TREE_NODE_BIT);

    let mut number = 0;
    for episode in &archives {
        number += 1;
        let mut item = TreeItem::empty();
        item.aid = episode["aid"].as_i64().unwrap_or(0);
        item.bvid = episode["bvid"].as_str().unwrap_or_default().to_owned();
        item.cover = episode["pic"].as_str().unwrap_or_default().to_owned();
        item.duration = episode_duration(episode);
        item.number = Number::Int(number);
        item.pubtime = episode["pubdate"].as_i64().unwrap_or(0);
        item.episode_id = episode_id.clone();
        item.title = episode["title"].as_str().unwrap_or_default().to_owned();
        item.url = format!("https://www.bilibili.com/video/{}", item.bvid);
        item.set_attribute(
            Attribute::COLLECTION_LIST_BIT
                | Attribute::COLLECTION_BIT
                | Attribute::VIDEO_BIT
                | Attribute::NEED_PARSE_BIT,
        );
        node.add_child(item);
    }

    let count = node.children.len() as i64 + (pn - 1) * LIST_PS;
    let extra = paged_extra(
        meta_total(&meta).max(count),
        LIST_PS,
        pn,
        false,
        "",
    );

    let mut wrapper = TreeItem::empty();
    wrapper.add_child(node);

    Ok(ParseOutput {
        category: COLLECTION_CATEGORY,
        node: wrapper,
        extra,
        current_episode: current_bvid.map(|bvid| ("bvid".to_owned(), bvid)),
    })
}

/// Which family a `lists` link declares (`type=season|series`).
enum ListFamily {
    Season,
    Series,
}

fn list_link_family(url: &str) -> Option<ListFamily> {
    match query_str(url, "type") {
        Some("season") => Some(ListFamily::Season),
        Some("series") => Some(ListFamily::Series),
        _ => None,
    }
}

fn list_mid(url: &str) -> Option<i64> {
    static RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"/([0-9]+)").unwrap());
    let path = url::Url::parse(url)
        .map(|u| u.path().to_owned())
        .unwrap_or_else(|_| url.to_owned());
    RE.captures(&path)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
}

fn list_path_id(url: &str) -> Option<i64> {
    list_mid(url)
}

/// `ListParser.get_video_data`: the link's own video, when it carries
/// oid/bvid; `None` without any request when it carries neither.
async fn list_video_data(
    client: &network::HttpClient,
    base: &str,
    url: &str,
) -> Result<Option<Value>, ListError> {
    static BVID: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"bvid=([Bb][Vv][0-9A-Za-z]+)").unwrap());

    let params: Vec<(String, String)> = if let Some(bvid) =
        BVID.captures(url).and_then(|c| c.get(1)).map(|m| m.as_str().to_owned())
    {
        vec![("bvid".into(), bvid)]
    } else if let Some(aid) = query_u64(url, "oid") {
        vec![("aid".into(), aid.to_string())]
    } else {
        return Ok(None);
    };

    let response: Value = client.get_json(&swap_host(VIEW_API, base), &params).await?;
    check_response(&response)?;
    Ok(Some(response["data"].clone()))
}

fn list_video_bvid(url: &str) -> Option<String> {
    static BVID: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"bvid=([Bb][Vv][0-9A-Za-z]+)").unwrap());
    BVID.captures(url)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_owned())
}

async fn seasons_archives(
    client: &network::HttpClient,
    base: &str,
    mid: i64,
    season_id: i64,
    pn: i64,
) -> Result<(Value, i64), ListError> {
    let params = vec![
        ("mid".into(), mid.to_string()),
        ("season_id".into(), season_id.to_string()),
        ("sort_reverse".into(), "false".into()),
        ("page_size".into(), LIST_PS.to_string()),
        ("page_num".into(), pn.to_string()),
        ("web_location".into(), "333.1387".into()),
    ];
    let response: Value = client
        .get_json(&swap_host(SEASONS_ARCHIVES_API, base), &params)
        .await?;
    check_response(&response)?;
    let count = response["data"]["page"]["total"].as_i64().unwrap_or(0);
    Ok((response["data"].clone(), count))
}

async fn series_archives(
    client: &network::HttpClient,
    base: &str,
    mid: i64,
    series_id: i64,
    pn: i64,
) -> Result<(Value, i64), ListError> {
    let params = vec![
        ("mid".into(), mid.to_string()),
        ("current_mid".into(), "0".into()),
        ("series_id".into(), series_id.to_string()),
        ("only_normal".into(), "true".into()),
        ("sort".into(), "desc".into()),
        ("ps".into(), LIST_PS.to_string()),
        ("pn".into(), pn.to_string()),
        ("web_location".into(), "333.1387".into()),
    ];
    let response: Value = client
        .get_json(&swap_host(SERIES_ARCHIVES_API, base), &params)
        .await?;
    check_response(&response)?;
    let count = response["data"]["page"]["total"].as_i64().unwrap_or(0);
    Ok((response["data"].clone(), count))
}

/// Series meta probe (`get_series_meta`): a series id answers code 0 with
/// its meta; anything else (147002 for a season id) means "not a series".
async fn series_meta_probe(
    client: &network::HttpClient,
    base: &str,
    sid: i64,
) -> Result<Option<Value>, ListError> {
    let params = vec![
        ("series_id".to_owned(), sid.to_string()),
        ("web_location".to_owned(), "333.1387".to_owned()),
    ];
    let response: Value = client.get_json(&swap_host(SERIES_API, base), &params).await?;
    if response.get("code").and_then(Value::as_i64).unwrap_or(-1) != 0 {
        return Ok(None);
    }
    Ok(Some(response["data"]["meta"].clone()))
}

async fn series_meta_request(
    client: &network::HttpClient,
    base: &str,
    series_id: i64,
) -> Result<Value, ListError> {
    let params = vec![
        ("series_id".to_owned(), series_id.to_string()),
        ("web_location".to_owned(), "333.1387".to_owned()),
    ];
    let response: Value = client.get_json(&swap_host(SERIES_API, base), &params).await?;
    check_response(&response)?;
    Ok(response["data"]["meta"].clone())
}

fn season_meta(data: &Value) -> Value {
    data["meta"].clone()
}

fn meta_mid(meta: &Value) -> i64 {
    meta["mid"].as_i64().unwrap_or(0)
}

fn meta_title(meta: &Value) -> String {
    // Season metas carry `title`, series metas `name`.
    meta.get("title")
        .and_then(Value::as_str)
        .or_else(|| meta.get("name").and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned()
}

fn meta_total(meta: &Value) -> i64 {
    meta["total"].as_i64().unwrap_or(0)
}

fn archives_page(data: &Value, _count: i64) -> Vec<Value> {
    data["archives"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Watch later (稍后再看)
// ---------------------------------------------------------------------------

/// Watch-later page fetch (`WatchLaterParser`): keyword param is `key`,
/// not `keyword`; requires a logged-in, unexpired session.
pub async fn parse_watch_later(
    client: &network::HttpClient,
    base: &str,
    ctx: &ListParseContext,
    url: &str,
    auth: &auth::AuthState,
    pn: i64,
) -> Result<ParseOutput, ListError> {
    require_login(auth)?;
    let keyword = urls::search::extract_keyword(url).unwrap_or_default();

    let params = vec![
        ("pn".into(), pn.to_string()),
        ("ps".into(), WATCH_LATER_PS.to_string()),
        ("viewed".into(), "0".into()),
        ("key".into(), keyword.clone()),
        ("asc".into(), "false".into()),
        ("need_split".into(), "true".into()),
        ("web_location".into(), "333.881".into()),
    ];
    let response: Value = client.get_json(&swap_host(TOVIEW_API, base), &params).await?;
    check_response(&response)?;

    let data = &response["data"];
    let episode_id = ctx.add_episode();
    ctx.episode_data.set_episode_data(
        &episode_id,
        json!({ "source_title": WATCH_LATER_CATEGORY }),
    );

    let list = data["list"].as_array();
    let count = data["count"].as_i64().unwrap_or(0);

    let mut node = TreeItem::root(
        Number::Text(WATCH_LATER_CATEGORY.to_owned()),
        &with_search_keyword("", &keyword),
    );
    node.set_attribute(Attribute::TREE_NODE_BIT);

    let mut number = 0;
    for episode in list.map(Vec::as_slice).unwrap_or_default() {
        number += 1;
        let mut item = TreeItem::empty();
        item.aid = episode["aid"].as_i64().unwrap_or(0);
        item.badge = watch_later_badge(episode);
        item.bvid = episode["bvid"].as_str().unwrap_or_default().to_owned();
        item.cid = episode["cid"].as_i64().unwrap_or(0);
        item.cover = episode["pic"].as_str().unwrap_or_default().to_owned();
        item.duration = episode_duration(episode);
        item.ep_id = watch_later_ep_id(episode);
        item.episode_id = episode_id.clone();
        item.number = Number::Int(number);
        item.pubtime = episode["pubdate"].as_i64().unwrap_or(0);
        item.favtime = episode["add_at"].as_i64().unwrap_or(0);
        item.title = episode["title"].as_str().unwrap_or_default().to_owned();
        item.url = format!("https://www.bilibili.com/video/{}", item.bvid);

        if episode.get("bangumi").is_some() {
            item.set_attribute(Attribute::BANGUMI_BIT);
        } else {
            item.set_attribute(Attribute::VIDEO_BIT);
        }
        item.set_attribute(Attribute::WATCH_LATER_BIT | Attribute::NEED_PARSE_BIT);
        node.add_child(item);
    }

    let mut wrapper = TreeItem::empty();
    wrapper.add_child(node);

    Ok(ParseOutput {
        category: WATCH_LATER_CATEGORY,
        node: wrapper,
        extra: paged_extra(count, WATCH_LATER_PS, pn, true, &keyword),
        current_episode: None,
    })
}

fn watch_later_badge(episode: &Value) -> String {
    if episode.get("bangumi").is_some() {
        return episode["pgc_label"].as_str().unwrap_or_default().to_owned();
    }
    String::new()
}

fn watch_later_ep_id(episode: &Value) -> i64 {
    if episode.get("bangumi").is_some() {
        return episode["bangumi"]["ep_id"].as_i64().unwrap_or(0);
    }
    0
}

// ---------------------------------------------------------------------------
// History (历史记录)
// ---------------------------------------------------------------------------

/// History page fetch (`HistoryParser`); requires login. Unknown
/// `business` values fall through to the video attribute so re-parse
/// always has a shape to dispatch on (see `test_reparse_worker`).
pub async fn parse_history(
    client: &network::HttpClient,
    base: &str,
    ctx: &ListParseContext,
    url: &str,
    auth: &auth::AuthState,
    pn: i64,
) -> Result<ParseOutput, ListError> {
    require_login(auth)?;
    let keyword = urls::search::extract_keyword(url).unwrap_or_default();

    let params = vec![
        ("pn".into(), pn.to_string()),
        ("keyword".into(), keyword.clone()),
        ("business".into(), "archive".into()),
        ("add_time_start".into(), "0".into()),
        ("add_time_end".into(), "0".into()),
        ("arc_max_duration".into(), "0".into()),
        ("arc_min_duration".into(), "0".into()),
        ("device_type".into(), "0".into()),
        ("web_location".into(), "333.1391".into()),
    ];
    let response: Value = client.get_json(&swap_host(HISTORY_API, base), &params).await?;
    check_response(&response)?;

    let data = &response["data"];
    let episode_id = ctx.add_episode();
    ctx.episode_data
        .set_episode_data(&episode_id, json!({ "source_title": HISTORY_CATEGORY }));

    let list = data["list"].as_array();
    let count = data["page"]["total"].as_i64().unwrap_or(0);

    let mut node = TreeItem::root(
        Number::Text(HISTORY_CATEGORY.to_owned()),
        &with_search_keyword("", &keyword),
    );
    node.set_attribute(Attribute::TREE_NODE_BIT);

    let mut number = 0;
    for episode in list.map(Vec::as_slice).unwrap_or_default() {
        number += 1;
        let mut item = TreeItem::empty();
        item.badge = history_badge(episode);
        item.bvid = episode["history"]["bvid"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        item.cid = episode["history"]["cid"].as_i64().unwrap_or(0);
        item.cover = episode["cover"].as_str().unwrap_or_default().to_owned();
        item.duration = episode_duration(episode);
        item.ep_id = episode["history"]["epid"].as_i64().unwrap_or(0);
        item.episode_id = episode_id.clone();
        item.number = Number::Int(number);
        item.viewtime = episode["view_at"].as_i64().unwrap_or(0);
        item.title = history_title(episode);
        item.url = history_url(episode);
        item.expired = episode["duration"].as_i64() == Some(0);

        match episode["history"]["business"].as_str() {
            Some("pgc") => item.set_attribute(Attribute::BANGUMI_BIT),
            Some("cheese") => item.set_attribute(Attribute::CHEESE_BIT),
            // archive and every unseen business value: a re-parse entry
            // must carry *some* shape bit; the video shape is the only
            // sane default (real videos download, non-videos fail loudly
            // at stream fetch).
            _ => item.set_attribute(Attribute::VIDEO_BIT),
        }
        item.set_attribute(Attribute::HISTORY_BIT | Attribute::NEED_PARSE_BIT);
        node.add_child(item);
    }

    let mut wrapper = TreeItem::empty();
    wrapper.add_child(node);

    Ok(ParseOutput {
        category: HISTORY_CATEGORY,
        node: wrapper,
        extra: paged_extra(count, HISTORY_PS, pn, true, &keyword),
        current_episode: None,
    })
}

fn history_badge(episode: &Value) -> String {
    if episode["duration"].as_i64() == Some(0) {
        return BADGE_EXPIRED.to_owned();
    }
    episode["badge"].as_str().unwrap_or_default().to_owned()
}

fn history_title(episode: &Value) -> String {
    let title = episode["title"].as_str().unwrap_or_default();
    if episode["history"]["business"].as_str() == Some("pgc") {
        let show = episode["show_title"].as_str().unwrap_or_default();
        if !show.is_empty() {
            return format!("{title} - {show}");
        }
    }
    title.to_owned()
}

fn history_url(episode: &Value) -> String {
    let uri = episode["uri"].as_str().unwrap_or_default();
    if !uri.is_empty() {
        return uri.to_owned();
    }
    format!(
        "https://www.bilibili.com/video/{}",
        episode["history"]["bvid"].as_str().unwrap_or_default()
    )
}

// ---------------------------------------------------------------------------
// Weekly popular (每周必看)
// ---------------------------------------------------------------------------

/// Weekly-issue fetch (`PopularParser`): one request, no paging, no search.
pub async fn parse_popular(
    client: &network::HttpClient,
    base: &str,
    ctx: &ListParseContext,
    url: &str,
) -> Result<ParseOutput, ListError> {
    let number = query_u64(url, "num").ok_or(ListError::InvalidLink)?;

    let params = vec![
        ("number".to_owned(), number.to_string()),
        ("web_location".to_owned(), "333.934".to_owned()),
    ];
    let response: Value = client.get_json(&swap_host(WEEKLY_API, base), &params).await?;
    check_response(&response)?;

    let data = &response["data"];
    let label = data["config"]["label"].as_str().unwrap_or_default().to_owned();

    let episode_id = ctx.add_episode();

    let mut node = TreeItem::root(Number::Text(WEEKLY_CATEGORY.to_owned()), &label);
    node.set_attribute(Attribute::TREE_NODE_BIT);

    let mut number = 0;
    for episode in data["list"].as_array().map(Vec::as_slice).unwrap_or_default() {
        number += 1;
        let mut item = TreeItem::empty();
        item.aid = episode["aid"].as_i64().unwrap_or(0);
        item.bvid = episode["bvid"].as_str().unwrap_or_default().to_owned();
        item.cid = episode["cid"].as_i64().unwrap_or(0);
        item.cover = episode["pic"].as_str().unwrap_or_default().to_owned();
        item.duration = episode_duration(episode);
        item.number = Number::Int(number);
        item.pubtime = episode["pubdate"].as_i64().unwrap_or(0);
        item.episode_id = episode_id.clone();
        item.title = episode["title"].as_str().unwrap_or_default().to_owned();
        let mut related = Map::new();
        related.insert("source_title".to_owned(), Value::String(label.clone()));
        item.related_titles = related;
        item.url = format!("https://www.bilibili.com/video/{}", item.bvid);
        item.set_attribute(Attribute::VIDEO_BIT | Attribute::WEEKLY_BIT);
        node.add_child(item);
    }

    let mut wrapper = TreeItem::empty();
    wrapper.add_child(node);

    Ok(ParseOutput {
        category: WEEKLY_CATEGORY,
        node: wrapper,
        extra: ExtraData::default(),
        current_episode: None,
    })
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn require_login(auth: &auth::AuthState) -> Result<(), ListError> {
    if !auth.is_login {
        return Err(ListError::LoginRequired);
    }
    Ok(())
}

fn query_u64(url: &str, name: &str) -> Option<i64> {
    let parsed = url::Url::parse(url).ok()?;
    parsed
        .query_pairs()
        .find(|(k, _)| k == name)
        .and_then(|(_, v)| v.parse().ok())
}

fn query_str<'a>(url: &'a str, name: &str) -> Option<&'a str> {
    // Naked lifetime: query_pairs borrows from `url`.
    let index = url.find(&format!("{name}="))?;
    let rest = &url[index + name.len() + 1..];
    let end = rest.find('&').unwrap_or(rest.len());
    Some(&rest[..end])
}

/// Swap the endpoint's host for the wiremock `base` in tests.
pub fn swap_host(api: &str, base: &str) -> String {
    if base.is_empty() {
        return api.to_owned();
    }
    let path = api
        .strip_prefix("https://")
        .and_then(|rest| rest.find('/').map(|i| &rest[i..]))
        .unwrap_or(api);
    format!("{base}{path}")
}
