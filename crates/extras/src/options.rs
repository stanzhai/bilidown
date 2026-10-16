//! The `resolve()` half of `src/util/download/task/options.py` for the keys
//! the extras parsers read. Per-task `Options` snapshot wins; `None` (or an
//! enum value the build no longer knows) falls back to the global config.

use config::ConfigStore;
use database::TaskInfo;

/// Resolve `key` from the task snapshot or the global `Additional` group.
/// Returns the raw value (enum members as their string value, dictionaries
/// and booleans as JSON); unknown enum strings fall back to the global
/// value exactly like Python's `ValueError` branch.
pub fn resolve(task: &TaskInfo, store: &ConfigStore, key: &str) -> serde_json::Value {
    let pinned = match key {
        "subtitle_type" => task
            .Options
            .subtitle_type
            .clone()
            .map(serde_json::Value::String),
        "embed_subtitle" => task.Options.embed_subtitle.map(serde_json::Value::Bool),
        "delete_subtitle_after_embed" => task
            .Options
            .delete_subtitle_after_embed
            .map(serde_json::Value::Bool),
        "subtitle_language" => task
            .Options
            .subtitle_language
            .clone()
            .map(serde_json::Value::Object),
        "cover_type" => task
            .Options
            .cover_type
            .clone()
            .map(serde_json::Value::String),
        "attach_cover" => task.Options.attach_cover.map(serde_json::Value::Bool),
        "delete_cover_after_attach" => task
            .Options
            .delete_cover_after_attach
            .map(serde_json::Value::Bool),
        "metadata_type" => task
            .Options
            .metadata_type
            .clone()
            .map(serde_json::Value::String),
        _ => None,
    };

    let Some(value) = pinned else {
        return global(store, key);
    };

    // Enum-typed keys: an unknown string must not fail the download.
    if matches!(
        key,
        "subtitle_type" | "cover_type" | "metadata_type" | "danmaku_type"
    ) {
        let known = matches!(
            (key, value.as_str()),
            (
                "subtitle_type",
                Some("srt" | "lrc" | "txt" | "ass" | "json")
            ) | ("cover_type", Some("jpg" | "png" | "avif" | "webp"))
                | ("metadata_type", Some("nfo" | "json"))
                | ("danmaku_type", Some("xml" | "ass" | "json"))
        );
        if !known {
            return global(store, key);
        }
    }

    value
}

fn global(store: &ConfigStore, key: &str) -> serde_json::Value {
    store
        .get("Additional", key)
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

/// `resolve(...).as_bool()` with Python's implicit-None error message.
pub fn resolve_bool(task: &TaskInfo, store: &ConfigStore, key: &str) -> bool {
    resolve(task, store, key).as_bool().unwrap_or(false)
}
