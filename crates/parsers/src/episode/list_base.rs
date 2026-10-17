//! Flat-list episode parsers (ticket 09): port of
//! `src/util/parse/episode/{list_base,space,watch_later,favlist,history,
//! popular,list}.py`.
//!
//! All six families share one skeleton (`ListEpisodeParserBase`): build a
//! structural root carrying the type label, walk the flat episode list,
//! map each entry to a `TreeItem` and set its attribute bits, and write
//! the parse's shared side data to [`crate::tree::EpisodeData`].

use serde_json::{Map, Value};

use crate::tree::{Attribute, EpisodeData, Number, TreeItem};

/// `Translator.EPISODE_TYPE` labels the six families use (node `number`).
pub mod episode_type {
    pub const PROFILE: &str = "User Uploads";
    pub const WATCH_LATER: &str = "Watch Later";
    pub const FAVORITES: &str = "Favorites";
    pub const HISTORY: &str = "History";
    pub const WEEKLY: &str = "Weekly Picks";
    pub const COLLECTION_LIST: &str = "Collection";
}

/// `Translator.TIP_MESSAGES("EXPIRED")`.
pub const TIP_EXPIRED: &str = "Expired";

/// `Translator.TIP_MESSAGES("SEARCH_KEYWORD")` format.
pub const TIP_SEARCH_KEYWORD: &str = "Search: {keyword}";

/// `Units.build_video_url`.
pub fn build_video_url(bvid: &str) -> String {
    format!("https://www.bilibili.com/video/{bvid}")
}

/// `EpisodeParserBase.with_search_keyword`: append the search label to a
/// node title; an empty title keeps only the label.
pub fn with_search_keyword(info_data: &Value, title: &str) -> String {
    let keyword = info_data["_search_keyword"].as_str().unwrap_or_default();
    if keyword.is_empty() {
        return title.to_owned();
    }
    let label = TIP_SEARCH_KEYWORD.replace("{keyword}", keyword);
    if title.is_empty() {
        label
    } else {
        format!("{title} - {label}")
    }
}

/// `EpisodeParserBase.get_episode_duration`.
pub fn episode_duration(episode: &Value) -> i64 {
    if let Some(d) = episode.get("duration") {
        return d.as_i64().unwrap_or(0);
    }
    if let Some(arc) = episode.get("arc") {
        return arc["duration"].as_i64().unwrap_or(0);
    }
    if let Some(length) = episode.get("length").and_then(Value::as_str) {
        return unformat_episode_duration(length);
    }
    0
}

/// `Units.unformat_episode_duration`: "H:M:S" → seconds.
pub fn unformat_episode_duration(text: &str) -> i64 {
    let mut total = 0i64;
    for part in text.split(':') {
        total = total * 60 + part.parse::<i64>().unwrap_or(0);
    }
    total
}

/// Shared JSON field readers: string key or "", integer key or 0.
pub(crate) fn text(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or_default().to_owned()
}

pub(crate) fn int(v: &Value, key: &str) -> i64 {
    v[key].as_i64().unwrap_or(0)
}

/// Result of one flat-list parse: the emitted node (pre-wrapper) plus the
/// `(key, value)` locating info the parse list uses to find the linked
/// entry.
pub struct ListParseResult {
    pub node: TreeItem,
    pub current_episode_data: Option<(&'static str, Value)>,
}

/// Shared skeleton (`ListEpisodeParserBase.build_node`): root node with
/// the type label, then one child per episode built by `build_item`.
pub fn build_list_node<F, G>(
    node_type_name: &str,
    node_title: &str,
    episodes: Option<&Vec<Value>>,
    mut build_item: F,
    mut set_attribute: G,
) -> TreeItem
where
    F: FnMut(&Value, usize) -> TreeItem,
    G: FnMut(&Value, &mut TreeItem),
{
    let mut root = TreeItem::root(Number::Text(node_type_name.to_owned()), node_title);
    root.set_attribute(Attribute::TREE_NODE_BIT);

    for (index, episode) in episodes.unwrap_or(&Vec::new()).iter().enumerate() {
        let mut item = build_item(episode, index + 1);
        set_attribute(episode, &mut item);
        root.add_child(item);
    }
    root
}

/// `update_episode_list` wrapper + title fallback: the outer invisible
/// wrapper node, and the title the parse list shows.
pub fn wrap_node(node: TreeItem) -> (String, TreeItem) {
    let mut title = node.title.clone();
    if title.is_empty() && node.count() > 0 {
        let child = node.child(0);
        if child.has_attribute(Attribute::VIDEO_BIT) || child.has_attribute(Attribute::AUDIO_BIT) {
            title = child.title.clone();
        }
        if child.has_attribute(Attribute::HISTORY_BIT)
            || child.has_attribute(Attribute::WATCH_LATER_BIT)
        {
            title = match &node.number {
                Number::Text(text) => text.clone(),
                Number::Int(n) => n.to_string(),
                Number::Empty => String::new(),
            };
        }
    }

    let mut wrapper = TreeItem::empty();
    wrapper.add_child(node);
    (title, wrapper)
}

/// Side-data write helpers shared by the list parsers.
pub fn episode_data_of(episode_data: &EpisodeData, episode_id: &str) -> Value {
    episode_data.get_episode_data(episode_id)
}

/// Merge `patch` into an episode's side data (`episode_data.update(...)`
/// on the live map reference).
pub fn patch_episode_data(episode_data: &EpisodeData, episode_id: &str, patch: Map<String, Value>) {
    let mut data = episode_data.get_episode_data(episode_id);
    if let Value::Object(map) = &mut data {
        for (key, value) in patch {
            map.insert(key, value);
        }
    }
    episode_data.set_episode_data(episode_id, data);
}
