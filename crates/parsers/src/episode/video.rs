//! av/BV video episode parser (ticket 09): port of
//! `src/util/parse/episode/video.py` — single video, multi-part (分P),
//! and ugc_season (合集) shapes — plus its side data
//! (`_video_episode_data_parser`).
//!
//! Shared media helpers (badge/attribute maps, timestamps) live here for
//! the bangumi/cheese/lesson parsers too.

use serde_json::{Map, Value};

use crate::episode::list_base::episode_duration;
use crate::tree::{Attribute, EpisodeData, Number, TreeItem};

/// `common/data.badge_map`: attribute bit → badge.
pub const BADGE_MAP: &[(u32, &str)] = &[(3, "充电专属")];

/// `Translator.EPISODE_TYPE("USER_UPLOADS")`.
pub const USER_UPLOADS: &str = "User Uploads";

/// Badge of an attribute bit-field (`get_episode_badge`).
pub fn episode_badge(attribute: i64) -> String {
    for (bit, badge) in BADGE_MAP {
        if attribute & (1 << bit) != 0 {
            return (*badge).to_owned();
        }
    }
    String::new()
}

/// `Time.timestamp_from_string` with `"%Y-%m-%d %H:%M:%S"`. Python's
/// naive `datetime.timestamp()` reads the wall clock in the host zone;
/// the app targets zh-CN (UTC+8), so interpret CST.
pub fn timestamp_from_string(text: &str) -> i64 {
    let mut parts = text.split(['-', ' ', ':']).map(|p| p.parse::<i64>().unwrap_or(0));
    let mut fields = [0i64; 6];
    for field in fields.iter_mut() {
        *field = parts.next().unwrap_or(0);
    }
    let [y, mo, d, h, mi, s] = fields;
    days_from_civil(y, mo, d) * 86_400 + h * 3_600 + mi * 60 + s - 8 * 3_600
}

/// Days since epoch (Howard Hinnant's `days_from_civil`).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or_default()
}

fn int(value: &Value, key: &str) -> i64 {
    value[key].as_i64().unwrap_or(0)
}

fn badge_of(episode: &Value) -> String {
    if episode["is_upower_exclusive"].as_bool().unwrap_or(false) {
        return "充电专属".to_owned();
    }
    episode_badge(int(episode, "attribute"))
}

/// Side data shared by every video-shaped parse
/// (`_video_episode_data_parser`).
pub fn video_episode_data(info_data: &Value) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("description".into(), Value::String(text(info_data, "desc").to_owned()));
    map.insert("tid".into(), Value::from(int(info_data, "tid")));
    map.insert("tid_v2".into(), Value::from(int(info_data, "tid_v2")));
    map.insert("uploader".into(), Value::String(text(&info_data["owner"], "name").to_owned()));
    map.insert("uploader_uid".into(), Value::from(int(&info_data["owner"], "mid")));
    map.insert("uploader_face".into(), Value::String(text(&info_data["owner"], "face").to_owned()));
    map
}

/// `VideoEpisodeParser.parse`: dispatch on shape and build the tree.
/// Returns the node plus the `(key, value)` current-entry locator.
pub fn parse_video(
    episode_data: &EpisodeData,
    episode_id: &str,
    info_data: &Value,
) -> (TreeItem, (String, Value)) {
    let mut map = video_episode_data(info_data);
    for (k, v) in episode_data.get_episode_data(episode_id).as_object().cloned().into_iter().flatten() {
        map.insert(k, v);
    }
    episode_data.set_episode_data(episode_id, Value::Object(map));

    let mut node = if info_data.get("ugc_season").is_some_and(|s| !s.is_null()) {
        ugc_season_parser(info_data, episode_id)
    } else if info_data["pages"].as_array().map(|p| p.len()).unwrap_or(0) > 1 {
        pages_parser(info_data, episode_id)
    } else {
        single_parser(info_data, episode_id)
    };
    let current = ("cid".to_owned(), Value::from(get_cid(info_data)));
    let _ = &mut node;
    (node, current)
}

/// `single_parser`: one root-labelled child.
pub fn single_parser(info_data: &Value, episode_id: &str) -> TreeItem {
    let mut root = TreeItem::root(Number::Text(USER_UPLOADS.to_owned()), "");
    root.set_attribute(Attribute::TREE_NODE_BIT);

    let mut item = TreeItem::empty();
    item.aid = int(info_data, "aid");
    item.bvid = text(info_data, "bvid").to_owned();
    item.cid = int(info_data, "cid");
    item.cover = text(info_data, "pic").to_owned();
    item.duration = episode_duration(info_data);
    item.number = Number::Int(1);
    item.pubtime = int(info_data, "pubdate");
    item.title = text(info_data, "title").to_owned();
    item.badge = badge_of(info_data);
    item.url = format!("https://www.bilibili.com/video/{}", item.bvid);
    item.episode_id = episode_id.to_owned();
    item.set_attribute(Attribute::VIDEO_BIT | Attribute::NORMAL_BIT);
    root.add_child(item);
    root
}

/// `pages_parser`: 分P root, one child per page.
pub fn pages_parser(info_data: &Value, episode_id: &str) -> TreeItem {
    let parent_title = text(info_data, "title").to_owned();
    let mut root = TreeItem::root(Number::Text("分P".to_owned()), &parent_title);
    root.set_attribute(Attribute::TREE_NODE_BIT);

    let mut number = 0;
    for page in info_data["pages"].as_array().unwrap_or(&Vec::new()) {
        number += 1;
        let mut item = TreeItem::empty();
        item.aid = int(info_data, "aid");
        item.bvid = text(info_data, "bvid").to_owned();
        item.cid = int(page, "cid");
        item.cover = text(info_data, "pic").to_owned();
        item.duration = episode_duration(page);
        item.number = Number::Int(number);
        item.pubtime = int(page, "ctime");
        item.title = text(page, "part").to_owned();
        item.badge = badge_of(info_data);
        item.url = format!(
            "https://www.bilibili.com/video/{}?p={}",
            item.bvid,
            int(page, "page")
        );
        item.episode_id = episode_id.to_owned();
        item.related_titles.insert("parent_title".into(), Value::String(parent_title.clone()));
        item.set_attribute(Attribute::VIDEO_BIT | Attribute::PART_BIT);
        root.add_child(item);
    }
    root
}

/// `ugc_season_parser`: 合集 root, 章节 sections (flattened when the
/// season has a single section), 分P sub-layers for multi-page episodes.
pub fn ugc_season_parser(info_data: &Value, episode_id: &str) -> TreeItem {
    let season = &info_data["ugc_season"];
    let collection_title = text(season, "title").to_owned();
    let mut root = TreeItem::root(Number::Text("合集".to_owned()), &collection_title);
    root.set_attribute(Attribute::TREE_NODE_BIT);

    let sections = season["sections"].as_array().cloned().unwrap_or_default();
    let section_count = sections.len();
    let mut number = 0;

    for section in &sections {
        let section_title = text(section, "title").to_owned();
        let mut section_node = TreeItem::root(Number::Text("章节".to_owned()), &section_title);
        section_node.set_attribute(Attribute::TREE_NODE_BIT);

        for episode in section["episodes"].as_array().cloned().unwrap_or_default() {
            let arc = &episode["arc"];
            let badge = badge_of(&episode);
            let mut related = Map::new();
            related.insert(
                "collection_title".into(),
                Value::String(collection_title.clone()),
            );
            related.insert(
                "section_title".into(),
                Value::String(if section_count > 1 { section_title.clone() } else { String::new() }),
            );
            let multi = episode["pages"].as_array().map(|p| p.len()).unwrap_or(0) > 1;

            if multi {
                let parent_title = text(&episode, "title").to_owned();
                let mut page_node =
                    TreeItem::root(Number::Text("分P".to_owned()), &parent_title);
                page_node.set_attribute(Attribute::TREE_NODE_BIT);

                for page in episode["pages"].as_array().cloned().unwrap_or_default() {
                    number += 1;
                    let mut item = TreeItem::empty();
                    item.aid = int(&episode, "aid");
                    item.bvid = text(&episode, "bvid").to_owned();
                    item.cid = int(&page, "cid");
                    item.cover = text(arc, "pic").to_owned();
                    item.duration = episode_duration(&page);
                    item.number = Number::Int(number);
                    item.pubtime = int(arc, "pubdate");
                    item.title = text(&page, "part").to_owned();
                    item.badge = badge.clone();
                    item.url = format!(
                        "https://www.bilibili.com/video/{}?p={}",
                        item.bvid,
                        int(&page, "page")
                    );
                    item.episode_id = episode_id.to_owned();
                    item.related_titles = related.clone();
                    item
                        .related_titles
                        .insert("parent_title".into(), Value::String(parent_title.clone()));
                    item.set_attribute(Attribute::VIDEO_BIT | Attribute::COLLECTION_BIT);
                    page_node.add_child(item);
                }
                attach(root_or_section(&mut root, &mut section_node, section_count), page_node);
            } else {
                number += 1;
                let mut item = TreeItem::empty();
                item.aid = int(&episode, "aid");
                item.bvid = text(&episode, "bvid").to_owned();
                item.cid = int(&episode, "cid");
                item.cover = text(arc, "pic").to_owned();
                item.duration = episode_duration(&episode);
                item.number = Number::Int(number);
                item.pubtime = int(arc, "pubdate");
                item.title = text(&episode, "title").to_owned();
                item.badge = badge.clone();
                item.url = format!("https://www.bilibili.com/video/{}", item.bvid);
                item.episode_id = episode_id.to_owned();
                item.related_titles = related.clone();
                item.set_attribute(Attribute::VIDEO_BIT | Attribute::COLLECTION_BIT);
                attach(root_or_section(&mut root, &mut section_node, section_count), item);
            }
        }

        if section_count > 1 {
            root.add_child(section_node);
        }
    }
    root
}

fn root_or_section<'a>(root: &'a mut TreeItem, section: &'a mut TreeItem, count: usize) -> &'a mut TreeItem {
    if count > 1 {
        section
    } else {
        root
    }
}

fn attach(parent: &mut TreeItem, child: TreeItem) {
    parent.add_child(child);
}

/// `get_cid`: the current page's cid when the link pins `?p=`, else the
/// video's.
pub fn get_cid(info_data: &Value) -> i64 {
    if let Some(current) = info_data.get("current_page_number").and_then(Value::as_i64) {
        for page in info_data["pages"].as_array().unwrap_or(&Vec::new()) {
            if int(page, "page") == current {
                return int(page, "cid");
            }
        }
    }
    int(info_data, "cid")
}
