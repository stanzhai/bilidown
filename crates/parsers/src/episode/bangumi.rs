//! Bangumi episode parser (ticket 09): port of
//! `src/util/parse/episode/bangumi.py`.

use serde_json::{Map, Value};

use crate::episode::list_base::{episode_duration, int, text};
use crate::episode::video::timestamp_from_string;
use crate::tree::{Attribute, EpisodeData, Number, TreeItem};

/// `Translator.EPISODE_TYPE(category)` mapping used by the bangumi
/// family (`bangumi_type_map`).
pub fn bangumi_episode_type(category: &str) -> String {
    match category {
        "ANIME" => "番剧".to_owned(),
        "MOVIE" => "电影".to_owned(),
        "DOCUMENTARY" => "纪录片".to_owned(),
        "CHN_ANIME" => "国创".to_owned(),
        "TV" => "电视剧".to_owned(),
        "VARIETY" => "综艺".to_owned(),
        _ => "番剧".to_owned(),
    }
}


/// `update_info_data`: prepend the 正片 section built from `episodes`,
/// then drop sections whose episodes lack bvid/cid (the "UP主陪你看"
/// chapters). Returns `(sections, episode_number_map)` with the map
/// skipping 预告 badges.
pub fn build_sections(info: &Value) -> (Vec<Value>, std::collections::HashMap<i64, i64>) {
    let mut sections = vec![serde_json::json!({
        "title": "正片", "episodes": info["episodes"].clone()
    })];
    if let Some(extra) = info.get("section").and_then(Value::as_array) {
        sections.extend(extra.iter().cloned());
    }

    // Episode numbers skip trailer badges mixed into the mainline when
    // logged out.
    let mut number_map = std::collections::HashMap::new();
    let mut number = 0;
    for episode in info["episodes"].as_array().cloned().unwrap_or_default() {
        if text(&episode, "badge") != "预告" {
            number += 1;
            number_map.insert(int(&episode, "cid"), number);
        }
    }

    sections.retain(|section| {
        section["episodes"]
            .as_array()
            .map(|eps| {
                eps.iter()
                    .all(|e| e.get("bvid").is_some() && e.get("cid").is_some())
            })
            .unwrap_or(false)
    });

    (sections, number_map)
}

/// `BangumiEpisodeParser.parse` → node plus current locator.
pub fn parse_bangumi(episode_data: &EpisodeData, episode_id: &str, info: &Value) -> TreeItem {
    episode_data_parser(episode_data, episode_id, info);

    let (sections, number_map) = build_sections(info);
    let season_title = text(info, "season_title");

    let mut root = TreeItem::root(Number::Text(bangumi_episode_type_default()), &season_title);
    root.set_attribute(Attribute::TREE_NODE_BIT);

    let mut number = 0;
    for section in &sections {
        let section_title = text(section, "title");
        let mut section_node = TreeItem::root(Number::Text("章节".to_owned()), &section_title);
        section_node.set_attribute(Attribute::TREE_NODE_BIT);

        for episode in section["episodes"].as_array().cloned().unwrap_or_default() {
            number += 1;
            let title = bangumi_title(&episode);
            let mut item = TreeItem::empty();
            item.aid = int(&episode, "aid");
            item.bvid = episode
                .get("bvid")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            item.cid = int(&episode, "cid");
            item.cover = text(&episode, "cover");
            item.duration = episode_duration(&episode) / 1000;
            item.ep_id = int(&episode, "ep_id");
            item.number = Number::Int(number);
            item.episode_number = number_map.get(&item.cid).copied().unwrap_or(0);
            item.pubtime = int(&episode, "pub_time");
            item.title = title.clone();
            item.badge = text(&episode, "badge");
            item.url = text(&episode, "link");
            item.episode_id = episode_id.to_owned();
            item.related_titles
                .insert("season_title".into(), Value::String(season_title.clone()));
            item.related_titles
                .insert("section_title".into(), Value::String(section_title.clone()));
            item.episode_plot = format!("《{season_title}》{title}");
            item.set_attribute(Attribute::BANGUMI_BIT);
            section_node.add_child(item);
        }
        root.add_child(section_node);
    }
    root
}

fn bangumi_episode_type_default() -> String {
    "番剧".to_owned()
}

fn bangumi_title(episode: &Value) -> String {
    let show = text(episode, "show_title");
    if !show.is_empty() {
        show
    } else {
        text(episode, "title")
    }
}

/// `episode_data_parser`: series/season metadata + rating + uploader.
pub fn episode_data_parser(episode_data: &EpisodeData, episode_id: &str, info: &Value) {
    let mut map = Map::new();
    map.insert(
        "series_title".into(),
        Value::String(text(&info["series"], "series_title")),
    );
    map.insert("season_id".into(), Value::from(int(info, "season_id")));
    map.insert("media_id".into(), Value::from(int(info, "media_id")));
    map.insert(
        "season_number".into(),
        Value::from(determine_season_number(info)),
    );
    map.insert(
        "areas".into(),
        Value::Array(
            info["areas"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|a| Value::String(text(a, "name")))
                .collect(),
        ),
    );
    map.insert(
        "premiered".into(),
        Value::from(timestamp_from_string(&text(&info["publish"], "pub_time"))),
    );
    map.insert("description".into(), Value::String(text(info, "evaluate")));
    map.insert(
        "styles".into(),
        Value::Array(
            info["styles"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|s| Value::String(s.as_str().unwrap_or_default().to_owned()))
                .chain(std::iter::once(Value::String(format!(
                    "Bilibili {}",
                    "番剧"
                ))))
                .collect(),
        ),
    );
    map.insert("poster".into(), Value::String(text(info, "cover")));
    map.insert("actors".into(), Value::String(text(info, "actors")));
    if info.get("rating").is_some_and(|r| !r.is_null()) {
        map.insert("rating".into(), Value::from(int(&info["rating"], "score")));
        map.insert(
            "rating_votes".into(),
            Value::from(int(&info["rating"], "count")),
        );
    }
    if info.get("up_info").is_some_and(|u| !u.is_null()) {
        map.insert(
            "uploader".into(),
            Value::String(text(&info["up_info"], "uname")),
        );
        map.insert(
            "uploader_uid".into(),
            Value::from(int(&info["up_info"], "mid")),
        );
        map.insert(
            "uploader_face".into(),
            Value::String(text(&info["up_info"], "avatar")),
        );
    }
    episode_data.set_episode_data(episode_id, Value::Object(map));
}

/// `determine_season_number`: 1-based index of this season in
/// `seasons`.
pub fn determine_season_number(info: &Value) -> i64 {
    let season_id = int(info, "season_id");
    info["seasons"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .enumerate()
        .find(|(_, s)| int(s, "season_id") == season_id)
        .map(|(i, _)| i as i64 + 1)
        .unwrap_or(0)
}

/// `get_ep_id`: `current_ep_id` wins; else the first mainline episode.
pub fn get_ep_id(info: &Value) -> Value {
    if let Some(current) = info.get("current_ep_id") {
        return current.clone();
    }
    info["episodes"]
        .as_array()
        .and_then(|eps| eps.first())
        .and_then(|e| e.get("ep_id"))
        .cloned()
        .unwrap_or(Value::String(String::new()))
}
