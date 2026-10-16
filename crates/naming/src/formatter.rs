//! Filename formatting and path safety (Python `util/format/file_name.py`),
//! plus the attribute → convention-type mapping from `tree.py`.

use std::collections::HashMap;

use database::info::TaskInfo;

use crate::format::{vformat, Value};
use crate::naming_rules::RuleStore;
use crate::template::{compile_rule, is_blank};

/// Rule of last resort when nothing else matched: a bare leaf title is
/// always present on a real task.
pub const FALLBACK_RULE: &str = "{leaf_title}";

/// Attribute bits (Python `util/common/enum.py Attribute`).
pub mod attribute {
    pub const VIDEO: i64 = 1 << 0;
    pub const BANGUMI: i64 = 1 << 1;
    pub const CHEESE: i64 = 1 << 2;
    pub const WEEKLY: i64 = 1 << 3;
    pub const COLLECTION_LIST: i64 = 1 << 4;
    pub const SPACE: i64 = 1 << 5;
    pub const FAVLIST: i64 = 1 << 6;
    pub const NEED_PARSE: i64 = 1 << 7;
    pub const NORMAL: i64 = 1 << 8;
    pub const PART: i64 = 1 << 9;
    pub const COLLECTION: i64 = 1 << 10;
    pub const INTERACTIVE: i64 = 1 << 11;
    pub const DOWNLOAD_AS_SINGLE_VIDEO: i64 = 1 << 12;
    pub const WATCH_LATER: i64 = 1 << 13;
    pub const HISTORY: i64 = 1 << 14;
    pub const TREE_NODE: i64 = 1 << 15;
    pub const AUDIO: i64 = 1 << 16;
    pub const FAVORITE_WITH_MULTI_PART_VIDEO: i64 = 1 << 17;
    pub const LESSON: i64 = 1 << 18;
}

/// Convention type ids (Python `ConventionType`).
pub mod convention_type {
    pub const NORMAL: i64 = 11;
    pub const PART: i64 = 12;
    pub const COLLECTION: i64 = 13;
    pub const INTERACTIVE_VIDEO: i64 = 14;
    pub const BANGUMI: i64 = 20;
    pub const CHEESE: i64 = 30;
    pub const FAVORITE: i64 = 40;
    pub const SPACE: i64 = 50;
    pub const HISTORY: i64 = 60;
    pub const WATCH_LATER: i64 = 70;
    pub const WEEKLY: i64 = 80;
    pub const AUDIO: i64 = 90;
}

/// Numbering modes (Python `NumberingType`).
pub mod numbering_type {
    pub const FROM_SPECIFIED: i64 = 0;
    pub const USE_PARSE_LIST: i64 = 1;
    pub const CONTINUOUS: i64 = 2;
}

/// Python `Time.from_timestamp`: ms timestamps get divided down; local time
/// so preview and download agree.
pub fn time_from_timestamp(ts: i64) -> Value {
    let seconds = if ts.abs() >= 10_000_000_000 { ts / 1000 } else { ts };
    let naive = chrono::DateTime::from_timestamp(seconds, 0)
        .map(|dt| dt.with_timezone(&chrono::Local).naive_local())
        .unwrap_or_default();
    Value::DateTime(naive)
}

/// Filename formatter bound to a task's variable data and naming rule.
pub struct FileNameFormatter<'a> {
    variable_data: HashMap<String, Value>,
    store: &'a RuleStore,
}

impl<'a> FileNameFormatter<'a> {
    pub fn new(variable_data: HashMap<String, Value>, store: &'a RuleStore) -> Self {
        Self { variable_data, store }
    }

    /// `FileNameFormatter.format()`: sanitize, render, normalize; any
    /// failure returns None — the caller shows a generic "cannot name this
    /// file" error rather than a template traceback.
    pub fn format(&self, type_id: Option<i64>, attribute: i64, rule: &str) -> Option<String> {
        let mut rule = rule.to_string();

        if rule.is_empty() {
            rule = self.store.get_rule_from_config(type_id)?;
        }

        if attribute != 0 {
            rule = get_special_rule(&rule, attribute)?;
        }

        if rule.is_empty() {
            rule = FALLBACK_RULE.to_string();
        }

        // Sanitize: rule literals must never smuggle path separators into a
        // filename. String values only — numbers with formatting are legit.
        let mut data = self.variable_data.clone();
        for value in data.values_mut() {
            if let Value::Text(s) = value {
                *s = sanitize_text(s);
            }
        }

        let rendered = compile_rule(&rule, false).ok()?.render(&data).ok()?;

        Some(normalize_path(&rendered))
    }

    pub fn variable_data(&self) -> &HashMap<String, Value> {
        &self.variable_data
    }
}

/// Special rule overrides keyed on attribute bits (Python
/// `get_special_rule`): only one case — "download multi-part as a single
/// video" names by leaf title alone.
fn get_special_rule(current_rule: &str, attribute: i64) -> Option<String> {
    if attribute & attribute::DOWNLOAD_AS_SINGLE_VIDEO != 0 {
        return Some("{leaf_title}".to_string());
    }
    Some(current_rule.to_string())
}

/// Filename-illegal chars → `_` (Python `sanitize_filename` regex
/// `[<>:"/\\|?*\x00-\x1f]`).
fn sanitize_text(s: &str) -> String {
    s.chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
                || (c as u32) <= 0x1f
            {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// Collapses directory components and strips junk the OS would reject
/// (Python `normalize_path`): leading separators, trailing dots/spaces per
/// component, `.`, `..` and empty components.
pub fn normalize_path(path: &str) -> String {
    if path.is_empty() {
        return "_".to_string();
    }

    let stripped = path.trim_start_matches(['/', '\\']);
    let parts: Vec<String> = stripped
        .split('/')
        .map(|part| {
            part.trim_start_matches(['/', '\\'])
                .trim_matches(['.', ' '])
                .to_string()
        })
        .filter(|part| !part.is_empty())
        .collect();

    if parts.is_empty() {
        return "_".to_string();
    }

    parts.join("/")
}

/// Attribute → convention type (Python `get_type_id_from_attribute`).
/// The table is **ordered**; first bit hit wins. Three tiers:
/// 1. media-kind bits (bangumi/cheese own their variable sets; lesson merged
///    into cheese),
/// 2. source-list bits (they swallow normal/part/collection members — shape
///    differences are absorbed inside the source type's optional segments),
/// 3. structure bits (collection first: reparse adds NORMAL/PART but
///    collection membership doesn't change), then VIDEO as the fallback.
pub fn get_type_id_from_attribute(attribute: i64) -> Option<i64> {
    let ordered = [
        (attribute::BANGUMI, convention_type::BANGUMI),
        (attribute::CHEESE, convention_type::CHEESE),
        (attribute::LESSON, convention_type::CHEESE),
        (attribute::FAVLIST, convention_type::FAVORITE),
        (attribute::SPACE, convention_type::SPACE),
        (attribute::HISTORY, convention_type::HISTORY),
        (attribute::WATCH_LATER, convention_type::WATCH_LATER),
        (attribute::WEEKLY, convention_type::WEEKLY),
        (attribute::AUDIO, convention_type::AUDIO),
        (attribute::COLLECTION, convention_type::COLLECTION),
        (attribute::NORMAL, convention_type::NORMAL),
        (attribute::PART, convention_type::PART),
        (attribute::INTERACTIVE, convention_type::INTERACTIVE_VIDEO),
        (attribute::VIDEO, convention_type::NORMAL),
    ];

    ordered
        .into_iter()
        .find(|(bit, _)| attribute & bit != 0)
        .map(|(_, type_id)| type_id)
}

/// The variable data the formatter sees for a task (Python
/// `get_variable_data_from_task_info`): 40 keys, times as datetimes,
/// timestamps raw, `number` numeric when parseable.
pub fn get_variable_data_from_task_info(task_info: &TaskInfo) -> HashMap<String, Value> {
    let e = &task_info.Episode;
    let b = &task_info.Basic;

    let s = |v: &str| Value::Text(v.to_string());
    let n = |v: i64| Value::Int(v);

    [
        ("pub_time", time_from_timestamp(e.pubtime)),
        ("pub_ts", n(e.pubtime)),
        ("create_time", time_from_timestamp(b.created_time)),
        ("create_ts", n(b.created_time)),
        ("fav_time", time_from_timestamp(e.favtime)),
        ("fav_ts", n(e.favtime)),
        ("last_watched_time", time_from_timestamp(e.viewtime)),
        ("last_watched_ts", n(e.viewtime)),
        ("number", number_value(&e.number)),
        ("uploader", s(&e.uploader)),
        ("uploader_uid", n(e.uploader_uid)),
        ("video_quality", s(&e.video_quality)),
        ("audio_quality", s(&e.audio_quality)),
        ("video_codec", s(&e.video_codec)),
        ("aid", n(e.aid)),
        ("bvid", s(&e.bvid)),
        ("cid", n(e.cid)),
        ("ep_id", n(e.ep_id)),
        ("season_id", n(e.season_id)),
        ("course_id", n(e.course_id)),
        ("lesson_id", n(e.lesson_id)),
        ("item_id", n(e.item_id)),
        ("section_id", n(e.section_id)),
        ("leaf_title", s(&e.leaf_title)),
        ("parent_title", s(&e.parent_title)),
        ("source_title", s(&e.source_title)),
        ("section_title", s(&e.section_title)),
        ("collection_title", s(&e.collection_title)),
        ("series_title", s(&e.series_title)),
        ("season_title", s(&e.season_title)),
        ("episode_title", s(&e.episode_title)),
        ("season_number", n(e.season_number)),
        ("episode_number", n(e.episode_number)),
        ("p", n(e.part_number)),
        ("favorites_name", s(&e.favorites_name)),
        ("favorites_id", n(e.favorites_id)),
        ("favorites_owner", s(&e.favorites_owner)),
        ("favorites_owner_id", n(e.favorites_owner_id)),
        ("space_owner", s(&e.space_owner)),
        ("space_owner_id", n(e.space_owner_id)),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

fn number_value(number: &str) -> Value {
    number.parse::<i64>().map(Value::Int).unwrap_or_else(|_| Value::Text(number.to_string()))
}

/// Whether a naming variable would render as empty/trivial (used by the
/// task-naming flow to drop optional segments).
pub fn value_is_blank(value: &Value) -> bool {
    is_blank(value)
}

/// Convenience wrapper for tests and callers that render one rule against
/// task data.
pub fn render_rule(rule: &str, data: &HashMap<String, Value>) -> Option<String> {
    compile_rule(rule, false).ok()?.render(data).ok().map(|s| normalize_path(&s))
}

/// Re-export so callers can feed `vformat` output back through the same
/// engine (used by conventions tests).
pub fn python_format(template: &str, data: &HashMap<String, Value>) -> Option<String> {
    vformat(template, data).ok()
}

/// Field names of a compiled rule, sorted and deduped.
pub fn rule_field_names(rule: &str) -> Vec<String> {
    compile_rule(rule, false)
        .map(|t| t.field_names())
        .unwrap_or_default()
}

