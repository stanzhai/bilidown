//! Batch parse — a multi-line list of av/BV ids folded into one Parse
//! result. Port of the batch half of
//! `src/gui/dialog/misc/batch_parse.py` (input validation),
//! `DynamicParser.parse_url_list` (the walk) and
//! `VideoParser.parse(url, get_info_data=True)` (per-link view fetch).
//!
//! Validation is the dialog's contract: lines are split, trimmed and
//! blank-dropped; an empty list is "no links", any line lacking av/BV is
//! "invalid format". The walk fetches each link's `x/web-interface/wbi/view`
//! (wbi-signed) and folds every video's episodes into one shared root —
//! the same episode layer the single-video parse uses, so a multi-P link
//! contributes all its pages, a collection its sections.

use serde_json::Value;

use crate::tree::{Attribute, Number, TreeItem};

/// wbi-signed view endpoint (`VIDEO_VIEW_API`).
pub const VIDEO_VIEW_API: &str = "https://api.bilibili.com/x/web-interface/wbi/view";
/// Origin stripped when swapping hosts for tests (`base` prefix).
const ORIGIN_API: &str = "https://api.bilibili.com";

/// Category key of the batch base parser (`ParserType.VIDEO`).
pub const BATCH_CATEGORY: &str = "video";

/// Category label the parse list shows (`Translator.EPISODE_TYPE("VIDEO")`
/// → "视频").
pub const VIDEO_CATEGORY: &str = "视频";

/// Seconds between links in the walk (Python `time.sleep(0.5)`); exposed
/// so tests can zero it.
pub const LINK_INTERVAL: f32 = 0.5;

/// Errors surfaced by batch parse.
#[derive(Debug, thiserror::Error)]
pub enum BatchError {
    #[error("network: {0}")]
    Network(#[from] network::HttpError),
    /// `check_response`: non-zero business code.
    #[error("{0}")]
    Api(String),
    /// No non-blank lines in the input.
    #[error("No Links Provided")]
    NoLinks,
    /// A line carries neither av nor BV.
    #[error("Invalid Link Format")]
    InvalidLink,
    #[error("invalid video link")]
    InvalidVideo,
}

/// Validation outcome of the batch dialog's text box (`accept()`).
///
/// Blank lines are dropped, the rest kept verbatim (trimmed). Errors carry
/// the dialog's toast semantics: empty input vs. a non av/BV line.
pub fn validate_batch_input(text: &str) -> Result<Vec<String>, BatchError> {
    let lines: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();

    if lines.is_empty() {
        return Err(BatchError::NoLinks);
    }

    for line in &lines {
        if !line.contains("av") && !line.contains("BV") {
            return Err(BatchError::InvalidLink);
        }
    }

    Ok(lines)
}

/// One link's contribution to the batch result: its video title and the
/// episode items built from the view response.
#[derive(Debug)]
pub struct BatchPage {
    pub title: String,
    pub items: Vec<TreeItem>,
}

/// Lift the bvid a video link carries (`VideoParser.parse` id extraction).
///
/// `av` ids convert through the XOR/ALPHABET algorithm (`aid_to_bvid`);
/// BV ids pass through as matched. Fails when the link has neither.
pub fn bvid_of(url: &str) -> Result<String, BatchError> {
    let bvid = regex_bvid(url);
    if let Some(bv) = bvid {
        return Ok(bv);
    }
    aid_of(url).map(aid_to_bvid).ok_or(BatchError::InvalidVideo)
}

fn regex_bvid(url: &str) -> Option<String> {
    static RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(?:BV|bv)\w+").unwrap());
    RE.find(url).map(|m| m.as_str().to_owned())
}

fn aid_of(url: &str) -> Option<i64> {
    static RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(?:av|AV)([0-9]+)").unwrap());
    RE.captures(url)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
}

/// av number → BV id (`VideoParser.aid_to_bvid`).
pub fn aid_to_bvid(aid: i64) -> String {
    const XOR_CODE: i64 = 23442827791579;
    const MAX_AID: i64 = 1 << 51;
    const ALPHABET: &[u8] = b"FcwAPNKTMug3GV5Lj7EJnHpWsx4tb8haYeviqBz6rkCy12mUSDQX9RdoZf";
    const ENCODE_MAP: [usize; 9] = [8, 7, 0, 5, 1, 3, 2, 4, 6];

    let mut bvid = [0u8; 9];
    let mut tmp = (MAX_AID | aid) ^ XOR_CODE;

    for &index in ENCODE_MAP.iter() {
        bvid[index] = ALPHABET[(tmp % ALPHABET.len() as i64) as usize];
        tmp /= ALPHABET.len() as i64;
    }

    format!("BV1{}", String::from_utf8_lossy(&bvid))
}

/// Fetch one link's view data (`VideoParser.get_video_info` with
/// `get_info_data=True` semantics: raw `data` object).
pub async fn fetch_video_data(
    client: &network::HttpClient,
    base: &str,
    url: &str,
) -> Result<Value, BatchError> {
    let bvid = bvid_of(url)?;
    let response: Value = client
        .get_json(
            &format!("{base}{}", VIDEO_VIEW_API.trim_start_matches(ORIGIN_API)),
            &[("bvid".to_owned(), bvid)],
        )
        .await?;

    if response.get("code").and_then(Value::as_i64).unwrap_or(-1) != 0 {
        return Err(BatchError::Api(
            response
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("未知错误")
                .to_owned(),
        ));
    }

    Ok(response["data"].clone())
}

/// Side data written to [`crate::tree::EpisodeData`] for one video link
/// (`VideoEpisodeParser.episode_data_parser`): description, tid info,
/// uploader info.
pub fn video_episode_data(data: &Value) -> Value {
    serde_json::json!({
        "description": data.get("desc").and_then(Value::as_str).unwrap_or_default(),
        "tid": data.get("tid").and_then(Value::as_i64).unwrap_or(0),
        "tid_v2": data.get("tid_v2").and_then(Value::as_i64).unwrap_or(0),
        "uploader": data["owner"]["name"].as_str().unwrap_or_default(),
        "uploader_uid": data["owner"]["mid"].as_i64().unwrap_or(0),
        "uploader_face": data["owner"]["face"].as_str().unwrap_or_default(),
    })
}

/// Build one link's episode items from its view `data` — the
/// `VideoEpisodeParser` single/pages/ugc_season shapes. Batch links are
/// whole videos, so ugc_season sections fold flat (the Python batch walk
/// appends `node.children` — the per-episode leaves — to one shared
/// root; 章节 tree nodes exist only in the single-parse tree the UI
/// renders).
///
/// Multi-P pages become `PART_BIT` items with `parent_title`; a plain
/// video one `NORMAL_BIT` item. Numbering runs 1..n per link, matching a
/// fresh `update_page_node` call.
pub fn build_video_items(data: &Value) -> BatchPage {
    let bvid = data["bvid"].as_str().unwrap_or_default().to_owned();
    let aid = data["aid"].as_i64().unwrap_or(0);
    let cover = data["pic"].as_str().unwrap_or_default().to_owned();
    let owner = data["owner"]["name"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let title = data["title"].as_str().unwrap_or_default().to_owned();
    let mut items: Vec<TreeItem> = Vec::new();

    if let Some(season) = data.get("ugc_season").filter(|s| !s.is_null()) {
        let collection_title = season["title"].as_str().unwrap_or_default().to_owned();
        let sections = season["sections"].as_array().cloned().unwrap_or_default();
        let show_section = sections.len() > 1;

        for section in &sections {
            let section_title = section["title"].as_str().unwrap_or_default().to_owned();

            for episode in section["episodes"].as_array().cloned().unwrap_or_default() {
                let ep_bvid = episode["bvid"].as_str().unwrap_or_default().to_owned();
                let ep_aid = episode["aid"].as_i64().unwrap_or(0);
                let ep_cover = episode["arc"]["pic"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                let ep_pubdate = episode["arc"]["pubdate"].as_i64().unwrap_or(0);
                let ep_title = episode["title"].as_str().unwrap_or_default().to_owned();
                let pages = episode["pages"].as_array().cloned().unwrap_or_default();

                let fill = |item: &mut TreeItem, parent_title: Option<&str>| {
                    item.aid = ep_aid;
                    item.bvid = ep_bvid.clone();
                    item.set_attribute(Attribute::VIDEO_BIT | Attribute::COLLECTION_BIT);
                    item.related_titles.insert(
                        "collection_title".to_owned(),
                        Value::String(collection_title.clone()),
                    );
                    item.related_titles.insert(
                        "section_title".to_owned(),
                        Value::String(if show_section {
                            section_title.clone()
                        } else {
                            String::new()
                        }),
                    );
                    if let Some(parent) = parent_title {
                        item.related_titles
                            .insert("parent_title".to_owned(), Value::String(parent.to_owned()));
                    }
                };

                if pages.len() > 1 {
                    for page in &pages {
                        let part_number = page["page"].as_i64().unwrap_or(0);
                        let number = items.len() as i64 + 1;
                        let mut item = TreeItem::new(
                            Number::Int(number),
                            page["part"].as_str().unwrap_or_default(),
                            &ep_cover,
                            page["duration"].as_i64().unwrap_or(0),
                            0,
                            ep_pubdate,
                            &format!("https://www.bilibili.com/video/{ep_bvid}?p={part_number}"),
                            &owner,
                            serde_json::Map::new(),
                        );
                        item.cid = page["cid"].as_i64().unwrap_or(0);
                        item.part_number = part_number;
                        fill(&mut item, Some(&ep_title));
                        items.push(item);
                    }
                } else {
                    let number = items.len() as i64 + 1;
                    let mut item = TreeItem::new(
                        Number::Int(number),
                        &ep_title,
                        &ep_cover,
                        episode["duration"].as_i64().unwrap_or(0),
                        0,
                        ep_pubdate,
                        &format!("https://www.bilibili.com/video/{ep_bvid}"),
                        &owner,
                        serde_json::Map::new(),
                    );
                    item.cid = episode["cid"].as_i64().unwrap_or(0);
                    fill(&mut item, None);
                    items.push(item);
                }
            }
        }

        return BatchPage { title, items };
    }

    if let Some(pages) = data["pages"].as_array() {
        if pages.len() > 1 {
            for page in pages {
                let part_number = page["page"].as_i64().unwrap_or(0);
                let mut item = TreeItem::new(
                    Number::Int(items.len() as i64 + 1),
                    page["part"].as_str().unwrap_or_default(),
                    &cover,
                    page["duration"].as_i64().unwrap_or(0),
                    0,
                    page["ctime"].as_i64().unwrap_or(0),
                    &format!("https://www.bilibili.com/video/{bvid}?p={part_number}"),
                    &owner,
                    [("parent_title".to_owned(), Value::String(title.clone()))]
                        .into_iter()
                        .collect(),
                );
                item.aid = aid;
                item.bvid = bvid.clone();
                item.cid = page["cid"].as_i64().unwrap_or(0);
                item.part_number = part_number;
                item.set_attribute(Attribute::VIDEO_BIT | Attribute::PART_BIT);
                items.push(item);
            }
            return BatchPage { title, items };
        }
    }

    let mut item = TreeItem::new(
        Number::Int(1),
        &title,
        &cover,
        data["duration"].as_i64().unwrap_or(0),
        0,
        data["pubdate"].as_i64().unwrap_or(0),
        &format!("https://www.bilibili.com/video/{bvid}"),
        &owner,
        serde_json::Map::new(),
    );
    item.aid = aid;
    item.bvid = bvid.clone();
    item.cid = data["cid"].as_i64().unwrap_or(0);
    item.set_attribute(Attribute::VIDEO_BIT | Attribute::NORMAL_BIT);
    items.push(item);

    BatchPage { title, items }
}

/// The walk: parse every link and fold the results into one root
/// (`DynamicParser.parse_url_list`). `on_progress` receives the running
/// episode count after each link (the dialog's link counter). `interval`
/// is the pause between links; pass `Duration::ZERO` in tests.
pub async fn parse_batch<F>(
    client: &network::HttpClient,
    base: &str,
    urls: &[String],
    mut on_progress: F,
    interval: std::time::Duration,
) -> Result<TreeItem, BatchError>
where
    F: FnMut(usize, usize),
{
    let mut root = TreeItem::root(Number::Text(VIDEO_CATEGORY.to_owned()), "");
    root.set_attribute(Attribute::TREE_NODE_BIT);

    let total = urls.len();
    let mut episode_count = 0usize;

    for (index, url) in urls.iter().enumerate() {
        let data = fetch_video_data(client, base, url).await?;
        let page = build_video_items(&data);

        // Root title comes from the first link's node title
        // (DynamicEpisodeParser.init_root_node laziness).
        if index == 0 {
            root.title = page.title.clone();
        }

        episode_count += page.items.len();
        for mut item in page.items {
            item.set_attribute(Attribute::NEED_PARSE_BIT);
            root.add_child(item);
        }

        on_progress(index + 1, episode_count);

        if index + 1 < total && !interval.is_zero() {
            tokio::time::sleep(interval).await;
        }
    }

    Ok(root)
}
