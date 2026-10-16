//! Per-task option snapshot and resolve (Python `task/options.py`).
//!
//! The 14 freezable keys match `_OPTION_SPEC` one-to-one; the fallback for
//! every key is the config store's same-named item (`_global_value`).
//! `pick_option` uses presence, not truthiness — an explicit `false` wins.

use config::ConfigStore;
use database::info::TaskInfo;
use serde_json::Value;

/// `_OPTION_SPEC` keys, in order. The enum-typed keys map their config
/// value (a string) through validation on resolve; bool/dict keys pass
/// through as-is.
pub const OPTION_KEYS: [&str; 14] = [
    "video_container",
    "danmaku_type",
    "embed_danmaku",
    "delete_danmaku_after_embed",
    "subtitle_type",
    "embed_subtitle",
    "delete_subtitle_after_embed",
    "subtitle_language",
    "cover_type",
    "attach_cover",
    "delete_cover_after_attach",
    "metadata_type",
    "m4a_to_mp3",
    "keep_original_files_type",
];

/// Enum-valued option keys: only these validate on resolve (Python
/// `enum_cls(value)`); everything else returns the frozen value verbatim.
const ENUM_KEYS: [&str; 6] = [
    "video_container",
    "danmaku_type",
    "subtitle_type",
    "cover_type",
    "metadata_type",
    "keep_original_files_type",
];

/// The valid members per enum key; an unrecognized frozen value falls
/// back to the global setting (Python `except ValueError`).
const ENUM_MEMBERS: [(&str, &[&str]); 6] = [
    ("video_container", &["mp4", "mkv"]),
    ("danmaku_type", &["xml", "ass", "json"]),
    ("subtitle_type", &["srt", "lrc", "txt", "ass", "json"]),
    ("cover_type", &["jpg", "png", "avif", "webp"]),
    ("metadata_type", &["nfo", "json"]),
    // OriginalFileType is int-valued: 0/1/2.
    ("keep_original_files_type", &["0", "1", "2"]),
];

/// `pick_option`: per-task override when present (`Some`-ness, not
/// truthiness — explicit `false`/0 wins), else the fallback.
pub fn pick_option(
    options: Option<&serde_json::Map<String, Value>>,
    key: &str,
    fallback: &Value,
) -> Value {
    if let Some(map) = options {
        if let Some(v) = map.get(key) {
            // Python `is not None`: JSON null is treated as missing.
            if !v.is_null() {
                return v.clone();
            }
        }
    }
    fallback.clone()
}

fn global_value(config: &ConfigStore, key: &str) -> Value {
    const GROUPS: [&str; 3] = ["Additional", "Download", "File Naming"];
    for group in GROUPS {
        if let Some(v) = config.get(group, key) {
            return v.clone();
        }
    }
    Value::Null
}

/// `snapshot`: freeze the current global settings (or the explicit
/// overrides) into the task's `Options` section. Returns a map covering
/// every key in [`OPTION_KEYS`]; enum values land as their scalar value.
pub fn snapshot(
    config: &ConfigStore,
    overrides: Option<&serde_json::Map<String, Value>>,
) -> serde_json::Map<String, Value> {
    let mut data = serde_json::Map::new();
    for key in OPTION_KEYS {
        let global = global_value(config, key);
        let value = pick_option(overrides, key, &global);
        data.insert(key.to_string(), value);
    }
    data
}

/// `resolve`: read a task's frozen option; `None`, missing section, or
/// an unrecognized enum member falls back to the current global setting.
/// Returns the raw JSON value (string for enums, int for
/// `keep_original_files_type`, bool/dict otherwise).
pub fn resolve(config: &ConfigStore, task_info: &TaskInfo, key: &str) -> Value {
    let frozen = frozen_value(task_info, key);
    let Some(value) = frozen else {
        return global_value(config, key);
    };
    if ENUM_KEYS.contains(&key) {
        let scalar = match &value {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        };
        let valid = ENUM_MEMBERS
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, members)| members.contains(&scalar.as_deref().unwrap_or("")))
            .unwrap_or(false);
        if !valid {
            return global_value(config, key);
        }
    }
    value
}

fn frozen_value(task_info: &TaskInfo, key: &str) -> Option<Value> {
    let options = task_info.Options.clone();

    match key {
        "video_container" => options.video_container.map(Value::String),
        "danmaku_type" => options.danmaku_type.map(Value::String),
        "embed_danmaku" => options.embed_danmaku.map(Value::Bool),
        "delete_danmaku_after_embed" => options.delete_danmaku_after_embed.map(Value::Bool),
        "subtitle_type" => options.subtitle_type.map(Value::String),
        "embed_subtitle" => options.embed_subtitle.map(Value::Bool),
        "delete_subtitle_after_embed" => options.delete_subtitle_after_embed.map(Value::Bool),
        "subtitle_language" => options
            .subtitle_language
            .map(|m| Value::Object(serde_json::Map::clone(&m))),
        "cover_type" => options.cover_type.map(Value::String),
        "attach_cover" => options.attach_cover.map(Value::Bool),
        "delete_cover_after_attach" => options.delete_cover_after_attach.map(Value::Bool),
        "metadata_type" => options.metadata_type.map(Value::String),
        "m4a_to_mp3" => options.m4a_to_mp3.map(Value::Bool),
        "keep_original_files_type" => options
            .keep_original_files_type
            .map(|v| Value::Number(v.into())),
        _ => None,
    }
}

/// Fill a task's `Options` section from a [`snapshot`] map (TaskInfo
/// assembly path). Unknown keys are ignored.
pub fn apply_snapshot(task_info: &mut TaskInfo, data: &serde_json::Map<String, Value>) {
    let o = &mut task_info.Options;
    for (key, value) in data {
        match key.as_str() {
            "video_container" => o.video_container = value.as_str().map(str::to_owned),
            "danmaku_type" => o.danmaku_type = value.as_str().map(str::to_owned),
            "embed_danmaku" => o.embed_danmaku = value.as_bool(),
            "delete_danmaku_after_embed" => o.delete_danmaku_after_embed = value.as_bool(),
            "subtitle_type" => o.subtitle_type = value.as_str().map(str::to_owned),
            "embed_subtitle" => o.embed_subtitle = value.as_bool(),
            "delete_subtitle_after_embed" => o.delete_subtitle_after_embed = value.as_bool(),
            "subtitle_language" => o.subtitle_language = value.as_object().cloned(),
            "cover_type" => o.cover_type = value.as_str().map(str::to_owned),
            "attach_cover" => o.attach_cover = value.as_bool(),
            "delete_cover_after_attach" => o.delete_cover_after_attach = value.as_bool(),
            "metadata_type" => o.metadata_type = value.as_str().map(str::to_owned),
            "m4a_to_mp3" => o.m4a_to_mp3 = value.as_bool(),
            "keep_original_files_type" => o.keep_original_files_type = value.as_i64(),
            _ => {}
        }
    }
}
