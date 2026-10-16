//! The six flat-list episode parsers (port of
//! `src/util/parse/episode/{space,watch_later,favlist,history,popular,
//! list}.py`). Each takes the response `data` object plus the category
//! name and returns the root node the parse list renders.

use serde_json::{json, Map, Value};

use crate::episode::list_base::{
    build_list_node, build_video_url, episode_duration, episode_type, patch_episode_data,
    with_search_keyword, TIP_EXPIRED,
};
use crate::tree::{Attribute, EpisodeData, Number, TreeItem};

fn text(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or_default().to_owned()
}

fn int(v: &Value, key: &str) -> i64 {
    v[key].as_i64().unwrap_or(0)
}

/// `SpaceEpisodeParser`.
pub fn parse_space(episode_data: &EpisodeData, info_data: &Value) -> TreeItem {
    let episode_id = episode_data.add_episode();
    let name = text(&info_data["info"], "name");
    patch_episode_data(
        episode_data,
        &episode_id,
        Map::from_iter([
            ("space_owner".to_owned(), json!(name)),
            ("space_owner_id".to_owned(), json!(int(&info_data["info"], "mid"))),
        ]),
    );

    let title = with_search_keyword(info_data, &name);
    let vlist = info_data["list"]["vlist"].as_array().cloned();

    build_list_node(
        episode_type::PROFILE,
        &title,
        vlist.as_ref(),
        |episode, count| {
            let mut item = TreeItem::new(
                Number::Int(count as i64),
                &text(episode, "title"),
                &text(episode, "pic"),
                episode_duration(episode),
                0,
                int(episode, "created"),
                &build_video_url(&text(episode, "bvid")),
                "",
                Map::new(),
            );
            item.aid = int(episode, "aid");
            item.bvid = text(episode, "bvid");
            item.ep_id = episode.get("season_id").and_then(Value::as_i64).unwrap_or(0);
            item.episode_id = episode_id.clone();
            item.badge = space_badge(episode);
            item
        },
        |episode, item| {
            if episode["is_lesson_video"].as_bool().unwrap_or(false) {
                item.set_attribute(Attribute::CHEESE_BIT);
            } else {
                item.set_attribute(Attribute::VIDEO_BIT);
            }
            item.set_attribute(Attribute::SPACE_BIT | Attribute::NEED_PARSE_BIT);
        },
    )
}

fn space_badge(episode: &Value) -> String {
    if episode["is_charging_arc"].as_bool().unwrap_or(false) {
        return "充电专属".to_owned();
    }
    if episode["is_lesson_video"].as_bool().unwrap_or(false) {
        return "课程".to_owned();
    }
    if episode["is_union_video"].as_bool().unwrap_or(false) {
        return "合作".to_owned();
    }
    String::new()
}

/// `WatchLaterEpisodeParser`.
pub fn parse_watch_later(episode_data: &EpisodeData, info_data: &Value) -> TreeItem {
    let episode_id = episode_data.add_episode();
    patch_episode_data(
        episode_data,
        &episode_id,
        Map::from_iter([(
            "source_title".to_owned(),
            json!(episode_type::WATCH_LATER),
        )]),
    );

    let title = with_search_keyword(info_data, "");
    let list = info_data.get("list").and_then(Value::as_array).cloned();

    build_list_node(
        episode_type::WATCH_LATER,
        &title,
        list.as_ref(),
        |episode, count| {
            let bangumi = episode.get("bangumi").filter(|b| !b.is_null());
            let mut item = TreeItem::new(
                Number::Int(count as i64),
                &text(episode, "title"),
                &text(episode, "pic"),
                episode_duration(episode),
                0,
                int(episode, "pubdate"),
                &build_video_url(&text(episode, "bvid")),
                "",
                Map::new(),
            );
            item.aid = int(episode, "aid");
            item.bvid = text(episode, "bvid");
            item.cid = int(episode, "cid");
            item.ep_id = bangumi.map(|b| int(b, "ep_id")).unwrap_or_default();
            item.favtime = int(episode, "add_at");
            item.episode_id = episode_id.clone();
            item.badge = bangumi
                .map(|_| text(episode, "pgc_label"))
                .unwrap_or_default();
            item
        },
        |episode, item| {
            if episode.get("bangumi").filter(|b| !b.is_null()).is_some() {
                item.set_attribute(Attribute::BANGUMI_BIT);
            } else {
                item.set_attribute(Attribute::VIDEO_BIT);
            }
            item.set_attribute(Attribute::WATCH_LATER_BIT | Attribute::NEED_PARSE_BIT);
        },
    )
}

/// `FavlistEpisodeParser`.
pub fn parse_favlist(episode_data: &EpisodeData, info_data: &Value) -> TreeItem {
    let episode_id = episode_data.add_episode();
    let info = &info_data["info"];
    patch_episode_data(
        episode_data,
        &episode_id,
        Map::from_iter([
            ("favorites_name".to_owned(), json!(text(info, "title"))),
            ("favorites_id".to_owned(), json!(int(info, "id"))),
            ("favorites_owner".to_owned(), json!(text(&info["upper"], "name"))),
            (
                "favorites_owner_id".to_owned(),
                json!(int(&info["upper"], "mid")),
            ),
        ]),
    );

    let title = with_search_keyword(info_data, &text(info, "title"));
    let medias = info_data.get("medias").and_then(Value::as_array).cloned();

    build_list_node(
        episode_type::FAVORITES,
        &title,
        medias.as_ref(),
        |episode, count| {
            let ogv = episode.get("ogv").filter(|o| !o.is_null());
            let mut item = TreeItem::new(
                Number::Int(count as i64),
                &favlist_title(episode),
                &text(episode, "cover"),
                episode_duration(episode),
                0,
                int(episode, "pubtime"),
                &build_video_url(&text(episode, "bvid")),
                "",
                Map::new(),
            );
            item.bvid = text(episode, "bvid");
            item.ep_id = int(episode, "id");
            item.favtime = int(episode, "fav_time");
            item.episode_id = episode_id.clone();
            item.badge = if ogv.is_some() {
                text(&episode["ogv"], "type_name")
            } else if episode.get("page").and_then(Value::as_i64).unwrap_or(0) > 1 {
                "分P".to_owned()
            } else {
                String::new()
            };
            item
        },
        |episode, item| {
            if episode.get("ogv").filter(|o| !o.is_null()).is_some() {
                item.set_attribute(Attribute::BANGUMI_BIT);
            } else {
                item.set_attribute(Attribute::VIDEO_BIT);
            }
            if episode.get("page").and_then(Value::as_i64).unwrap_or(0) > 1 {
                item.set_attribute(Attribute::FAVORITE_WITH_MULTI_PART_VIDEO_BIT);
            }
            item.set_attribute(Attribute::FAVLIST_BIT | Attribute::NEED_PARSE_BIT);
        },
    )
}

fn favlist_title(episode: &Value) -> String {
    if episode.get("ogv").filter(|o| !o.is_null()).is_some() {
        format!("{} - {}", text(episode, "title"), text(episode, "intro"))
    } else {
        text(episode, "title")
    }
}

/// `HistoryEpisodeParser`.
pub fn parse_history(episode_data: &EpisodeData, info_data: &Value) -> TreeItem {
    let episode_id = episode_data.add_episode();
    patch_episode_data(
        episode_data,
        &episode_id,
        Map::from_iter([(
            "source_title".to_owned(),
            json!(episode_type::HISTORY),
        )]),
    );

    let title = with_search_keyword(info_data, "");
    let list = info_data.get("list").and_then(Value::as_array).cloned();

    build_list_node(
        episode_type::HISTORY,
        &title,
        list.as_ref(),
        |episode, count| {
            let history = &episode["history"];
            let mut item = TreeItem::new(
                Number::Int(count as i64),
                &history_title(episode),
                &text(episode, "cover"),
                episode_duration(episode),
                0,
                0,
                &history_url(episode),
                "",
                Map::new(),
            );
            item.bvid = text(history, "bvid");
            item.cid = int(history, "cid");
            item.ep_id = int(history, "epid");
            item.viewtime = int(episode, "view_at");
            item.episode_id = episode_id.clone();
            item.expired = episode["duration"].as_i64().unwrap_or(0) == 0;
            item.badge = if item.expired {
                TIP_EXPIRED.to_owned()
            } else {
                text(episode, "badge")
            };
            item
        },
        |episode, item| {
            match episode["history"]["business"].as_str().unwrap_or_default() {
                "pgc" => item.set_attribute(Attribute::BANGUMI_BIT),
                "cheese" => item.set_attribute(Attribute::CHEESE_BIT),
                // archive and every unseen business: treat as a video so
                // the NEED_PARSE re-parse always has a shape bit to walk.
                _ => item.set_attribute(Attribute::VIDEO_BIT),
            }
            item.set_attribute(Attribute::HISTORY_BIT | Attribute::NEED_PARSE_BIT);
        },
    )
}

fn history_title(episode: &Value) -> String {
    if episode["history"]["business"].as_str() == Some("pgc") {
        let show_title = episode.get("show_title").and_then(Value::as_str).unwrap_or("");
        if !show_title.is_empty() {
            return format!("{} - {}", text(episode, "title"), show_title);
        }
    }
    text(episode, "title")
}

fn history_url(episode: &Value) -> String {
    let uri = text(episode, "uri");
    if !uri.is_empty() {
        return uri;
    }
    build_video_url(&text(&episode["history"], "bvid"))
}

/// `PopularEpisodeParser` (每周必看).
pub fn parse_popular(episode_data: &EpisodeData, info_data: &Value) -> TreeItem {
    let episode_id = episode_data.add_episode();

    let label = text(&info_data["config"], "label");
    let list = info_data["list"].as_array().cloned();

    build_list_node(
        episode_type::WEEKLY,
        &label,
        list.as_ref(),
        |episode, count| {
            let mut item = TreeItem::new(
                Number::Int(count as i64),
                &text(episode, "title"),
                &text(episode, "pic"),
                episode_duration(episode),
                0,
                int(episode, "pubdate"),
                &build_video_url(&text(episode, "bvid")),
                "",
                Map::from_iter([(
                    "source_title".to_owned(),
                    Value::String(label.clone()),
                )]),
            );
            item.aid = int(episode, "aid");
            item.bvid = text(episode, "bvid");
            item.cid = int(episode, "cid");
            item.episode_id = episode_id.clone();
            item
        },
        |_, item| {
            item.set_attribute(Attribute::VIDEO_BIT | Attribute::WEEKLY_BIT);
        },
    )
}

/// `ListEpisodeParser` (合集列表).
pub fn parse_list(episode_data: &EpisodeData, info_data: &Value) -> TreeItem {
    let episode_id = episode_data.add_episode();
    let meta = &info_data["meta"];
    let node_title = if meta.get("title").is_some() {
        text(meta, "title")
    } else {
        text(meta, "name")
    };
    patch_episode_data(
        episode_data,
        &episode_id,
        Map::from_iter([(
            "collection_title".to_owned(),
            json!(node_title),
        )]),
    );

    let archives = info_data["archives"].as_array().cloned();

    build_list_node(
        episode_type::COLLECTION_LIST,
        &node_title,
        archives.as_ref(),
        |episode, count| {
            let mut item = TreeItem::new(
                Number::Int(count as i64),
                &text(episode, "title"),
                &text(episode, "pic"),
                episode_duration(episode),
                0,
                int(episode, "pubdate"),
                &build_video_url(&text(episode, "bvid")),
                "",
                Map::new(),
            );
            item.aid = int(episode, "aid");
            item.bvid = text(episode, "bvid");
            item.episode_id = episode_id.clone();
            item
        },
        |_, item| {
            item.set_attribute(
                Attribute::COLLECTION_LIST_BIT
                    | Attribute::COLLECTION_BIT
                    | Attribute::VIDEO_BIT
                    | Attribute::NEED_PARSE_BIT,
            );
        },
    )
}

/// Current-entry locator for the collection list (`_current_bvid`).
pub fn list_current_episode_data(info_data: &Value) -> Option<(&'static str, Value)> {
    info_data["_current_bvid"]
        .as_str()
        .filter(|b| !b.is_empty())
        .map(|b| ("bvid", json!(b)))
}

/// Re-export of the wrapper for the parser side.
pub use crate::episode::list_base::wrap_node as wrap_episode_node;
