//! Cheese (课程) and mall-lesson episode parsers (ticket 09): ports of
//! `src/util/parse/episode/{cheese,lesson}.py`.

use serde_json::{Map, Value};

use crate::episode::list_base::{int, text};
use crate::tree::{Attribute, EpisodeData, Number, TreeItem};

// ---------------------------------------------------------------------------
// Cheese
// ---------------------------------------------------------------------------

/// `CheeseEpisodeParser.parse`.
pub fn parse_cheese(episode_data: &EpisodeData, episode_id: &str, info: &Value) -> TreeItem {
    cheese_episode_data(episode_data, episode_id, info);

    let cheese_title = text(info, "title");
    let mut root = TreeItem::root(Number::Text("课程".to_owned()), &cheese_title);
    root.set_attribute(Attribute::TREE_NODE_BIT);

    let mut number = 0;
    for section in info["sections"].as_array().cloned().unwrap_or_default() {
        let episodes = section["episodes"].as_array().cloned().unwrap_or_default();
        if episodes.is_empty() {
            continue;
        }
        let section_title = text(&section, "title");
        let mut section_node = TreeItem::root(Number::Text("章节".to_owned()), &section_title);
        section_node.set_attribute(Attribute::TREE_NODE_BIT);

        for episode in episodes {
            number += 1;
            let mut item = TreeItem::empty();
            item.aid = int(&episode, "aid");
            item.cid = int(&episode, "cid");
            item.cover = text(&episode, "cover");
            item.duration = int(&episode, "duration");
            item.ep_id = int(&episode, "id");
            item.number = Number::Int(number);
            item.episode_number = number;
            item.pubtime = int(&episode, "release_date");
            item.title = text(&episode, "title");
            item.badge = cheese_badge(&episode);
            item.url = format!("https://www.bilibili.com/cheese/play/{}", item.ep_id);
            item.episode_id = episode_id.to_owned();
            item.episode_plot = format!(
                "{} · {}",
                text(&episode, "play_way_subtitle"),
                text(&episode, "subtitle")
            );
            item.related_titles
                .insert("series_title".into(), Value::String(cheese_title.clone()));
            item.related_titles
                .insert("section_title".into(), Value::String(section_title.clone()));
            item.set_attribute(Attribute::CHEESE_BIT);
            section_node.add_child(item);
        }
        root.add_child(section_node);
    }
    root
}

/// `get_episode_badge`: `label` key wins, else the status map.
fn cheese_badge(episode: &Value) -> String {
    if let Some(label) = episode.get("label").and_then(Value::as_str) {
        if !label.is_empty() {
            return label.to_owned();
        }
    }
    match int(episode, "status") {
        1 => "全集试看".to_owned(),
        2 => "付费".to_owned(),
        3 => "部分试看".to_owned(),
        _ => String::new(),
    }
}

/// `episode_data_parser`.
fn cheese_episode_data(episode_data: &EpisodeData, episode_id: &str, info: &Value) {
    let mut map = Map::new();
    map.insert("poster".into(), Value::String(text(info, "cover")));
    map.insert("description".into(), Value::String(text(info, "subtitle")));
    map.insert(
        "styles".into(),
        Value::Array(vec![Value::String("Bilibili 课堂".to_owned())]),
    );
    map.insert("premiered".into(), Value::from(cheese_premiered(info)));
    map.insert("season_id".into(), Value::from(int(info, "season_id")));
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
    episode_data.set_episode_data(episode_id, Value::Object(map));
}

/// `get_premiered`: first `release_date` in the serialized payload —
/// JSON ints must not be confused with quoted ones, so match the raw
/// text like Python does.
fn cheese_premiered(info: &Value) -> i64 {
    static RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r#""release_date"\s*:\s*(\d+)"#).unwrap()
    });
    // ponytail: re-serializing the Value is O(payload); enough until
    // profiles show it (int-vs-quoted needs the raw JSON text).
    let text = serde_json::to_string(info).unwrap_or_default();
    RE.captures(&text)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().parse().unwrap_or(0))
        .unwrap_or(0)
}

/// `get_ep_id`: `current_ep_id` wins, else the first episode's id.
pub fn cheese_get_ep_id(info: &Value) -> Value {
    if let Some(current) = info.get("current_ep_id") {
        return current.clone();
    }
    info["sections"]
        .as_array()
        .and_then(|sections| sections.first())
        .and_then(|s| s["episodes"].as_array())
        .and_then(|eps| eps.first())
        .and_then(|e| e.get("id").cloned())
        .unwrap_or(Value::String(String::new()))
}

// ---------------------------------------------------------------------------
// Mall lesson
// ---------------------------------------------------------------------------

/// `LessonEpisodeParser.parse`.
pub fn parse_lesson(episode_data: &EpisodeData, episode_id: &str, info: &Value) -> TreeItem {
    let mut map = Map::new();
    map.insert(
        "styles".into(),
        Value::Array(vec![Value::String("Bilibili 课堂".to_owned())]),
    );
    map.insert("season_id".into(), Value::from(int(info, "lessonId")));
    episode_data.set_episode_data(episode_id, Value::Object(map));

    let course_title = lesson_course_title(info);
    let mut root = TreeItem::root(Number::Text("课程".to_owned()), &course_title);
    root.set_attribute(Attribute::TREE_NODE_BIT);

    for chapter in info["chapterList"].as_array().cloned().unwrap_or_default() {
        let sections = chapter["sectionList"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if sections.is_empty() {
            continue;
        }
        let chapter_title = text(&chapter, "chapterName").trim().to_owned();

        // chapterMode 0 courses carry a single blank-named chapter; its
        // sections hang directly off the course node.
        let chapter_wrapped = !chapter_title.is_empty();
        let mut chapter_node = TreeItem::root(Number::Text("章节".to_owned()), &chapter_title);
        chapter_node.set_attribute(Attribute::TREE_NODE_BIT);

        for section in sections {
            let Some(item) =
                lesson_section(info, &section, &course_title, &chapter_title, episode_id)
            else {
                continue;
            };
            if chapter_wrapped {
                chapter_node.add_child(item);
            } else {
                root.add_child(item);
            }
        }
        if chapter_wrapped && chapter_node.count() > 0 {
            root.add_child(chapter_node);
        }
    }
    root
}

/// `get_course_title`: lessonName (course) over itemsName (product).
pub fn lesson_course_title(info: &Value) -> String {
    let name = text(info, "lessonName");
    if !name.is_empty() {
        return name;
    }
    text(info, "itemsName")
}

fn lesson_section(
    info: &Value,
    section: &Value,
    course_title: &str,
    chapter_title: &str,
    episode_id: &str,
) -> Option<TreeItem> {
    let section_id = section.get("sectionId").and_then(Value::as_i64)?;
    if section
        .get("videoTime")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        == 0
    {
        // Unpublished section: the play API would fail for it anyway.
        return None;
    }

    let number = section
        .get("sectionIndex")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let mut item = TreeItem::empty();
    item.badge = lesson_badge(section);
    item.duration = int(section, "videoTime") / 1000;
    item.number = if number > 0 {
        Number::Int(number)
    } else {
        Number::Empty
    };
    item.episode_number = number;
    item.title = text(section, "sectionName");
    item.episode_plot = chapter_title.to_owned();
    item.episode_id = episode_id.to_owned();
    item.section_id = section_id;
    item.lesson_id = section
        .get("lessonId")
        .and_then(Value::as_i64)
        .filter(|id| *id != 0)
        .unwrap_or_else(|| int(info, "lessonId"));
    item.url = format!(
        "https://mall.bilibili.com/lesson/play?courseId={}&lessonId={}&itemId={}",
        int(info, "courseId"),
        int(info, "lessonId"),
        int(info, "itemId")
    );
    item.related_titles.insert(
        "series_title".into(),
        Value::String(course_title.to_owned()),
    );
    item.related_titles.insert(
        "season_title".into(),
        Value::String(course_title.to_owned()),
    );
    item.related_titles.insert(
        "section_title".into(),
        Value::String(if chapter_title.is_empty() {
            course_title.to_owned()
        } else {
            chapter_title.to_owned()
        }),
    );
    item.set_attribute(Attribute::LESSON_BIT);
    Some(item)
}

/// `get_section_badge`.
fn lesson_badge(section: &Value) -> String {
    if section["hasWatchRight"].as_bool().unwrap_or(false) {
        return String::new();
    }
    if section["couldPreview"].as_bool().unwrap_or(false) {
        return "试看".to_owned();
    }
    "付费".to_owned()
}

/// `get_section_id`: last-watched section, else the first playable one.
pub fn lesson_get_section_id(info: &Value) -> Value {
    if let Some(id) = info["locationInfo"]["sectionId"].as_i64() {
        return Value::from(id);
    }
    for chapter in info["chapterList"].as_array().cloned().unwrap_or_default() {
        for section in chapter["sectionList"]
            .as_array()
            .cloned()
            .unwrap_or_default()
        {
            if section
                .get("videoTime")
                .and_then(Value::as_i64)
                .unwrap_or(0)
                > 0
            {
                return section
                    .get("sectionId")
                    .cloned()
                    .unwrap_or(Value::String(String::new()));
            }
        }
    }
    Value::String(String::new())
}
