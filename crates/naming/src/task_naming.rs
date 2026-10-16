//! Task-naming snapshot flow (Python `download/task/manager.py` freeze /
//! update episode / update file-name blocks, plus `EpisodeData` from
//! `tree.py`).
//!
//! The snapshot is the contract with the resume path: freezing the rule at
//! task creation keeps a rule's meaning stable across config edits —
//! renaming presets must never retroactively move files already on disk.

use crate::formatter::{
    get_type_id_from_attribute, get_variable_data_from_task_info, FileNameFormatter,
};
use crate::naming_rules::RuleStore;
use database::info::TaskInfo;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// Naming-related runtime state (Python `NamingInfo` snapshot inputs, from
/// `runtime.py`).
#[derive(Debug, Clone, Default)]
pub struct NamingState {
    pub target_rule_ids: HashMap<i64, String>,
    pub global_starting_number: i64,
    pub current_starting_number: Option<i64>,
    pub rules_reset: bool,
}

impl NamingState {
    pub fn new() -> Self {
        Self::default()
    }
}

/// `download_starting_number` / `naming_rule_ids` style option carrier.
/// `Some` wins over the runtime state even when its map is empty: an empty
/// map says "user chose nothing for this batch", which must override a
/// leftover session-wide selection.
#[derive(Debug, Clone, Default)]
pub struct NamingOptions {
    pub naming_rule_ids: HashMap<i64, String>,
}

/// Python `pick_option`: options Some → options map, else runtime state.
fn pick_option<'a>(
    options: Option<&'a NamingOptions>,
    fallback: &'a HashMap<i64, String>,
) -> &'a HashMap<i64, String> {
    match options {
        Some(o) => &o.naming_rule_ids,
        None => fallback,
    }
}

/// Python `freeze_naming_rule`: pin the naming rule onto the task at
/// creation time (rule id → rule template → fallback to the type default).
pub fn freeze_naming_rule(
    task_info: &mut TaskInfo,
    options: Option<&NamingOptions>,
    store: &RuleStore,
    naming: &NamingState,
) {
    let type_id = get_type_id_from_attribute(task_info.Episode.attribute);

    let rule_map = pick_option(options, &naming.target_rule_ids);
    let rule_id = type_id.and_then(|id| rule_map.get(&id).cloned());

    let rule = match rule_id.as_deref() {
        Some(id) if !id.is_empty() => store.get_rule_by_id(id),
        _ => None,
    };

    let rule = rule.or_else(|| type_id.and_then(|id| store.get_rule_from_config(Some(id))));

    task_info.Naming.rule_id = rule_id.filter(|id| !id.is_empty());
    task_info.Naming.rule = rule;
    task_info.Naming.type_id = type_id;
}

/// Python `update_file_name_info`: set variable data, render, split into
/// folder/name. Any failure → Err carrying the offending rule for the UI.
pub fn update_file_name_info(task_info: &mut TaskInfo, store: &RuleStore) -> Result<(), String> {
    let variable_data = get_variable_data_from_task_info(task_info);
    let formatter = FileNameFormatter::new(variable_data, store);

    let frozen_rule = task_info.Naming.rule.clone().filter(|r| !r.is_empty());

    let rendered = match frozen_rule {
        Some(rule) => {
            formatter.format(task_info.Naming.type_id, task_info.Episode.attribute, &rule)
        }
        None => None,
    };

    let Some(rendered) = rendered else {
        let shown_rule = task_info.Naming.rule.clone().unwrap_or_default();
        return Err(format!("命名规则无法生成文件名：'{shown_rule}'"));
    };

    match rendered.rsplit_once('/') {
        Some((folder, name)) => {
            task_info.File.folder = folder.to_string();
            task_info.File.name = name.to_string();
        }
        None => {
            task_info.File.folder = ".".to_string();
            task_info.File.name = rendered;
        }
    }

    Ok(())
}

/// Episode-side variable data keyed by episode id (Python `EpisodeData`):
/// source-list titles, related titles and uploader info attach here before
/// the formatter ever sees the task.
static EPISODE_DATA: LazyLock<Mutex<EpisodeMap>> = LazyLock::new(|| Mutex::new(HashMap::new()));

type EpisodeMap = HashMap<String, serde_json::Map<String, serde_json::Value>>;

/// `EpisodeData` accessors used by the naming flow.
pub struct EpisodeData;

impl EpisodeData {
    fn map() -> std::sync::MutexGuard<'static, EpisodeMap> {
        EPISODE_DATA.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn add_episode() -> String {
        let id = uuid::Uuid::new_v4().to_string();
        Self::map().insert(id.clone(), serde_json::Map::new());
        id
    }

    pub fn set_episode_data(id: &str, data: serde_json::Map<String, serde_json::Value>) {
        Self::map().insert(id.to_string(), data);
    }

    pub fn remove(id: &str) {
        Self::map().remove(id);
    }

    pub fn get_episode_data(id: &str) -> serde_json::Map<String, serde_json::Value> {
        Self::map().get(id).cloned().unwrap_or_default()
    }
}

/// Attribute bits used by `update_episode_info`.
const ATTR_BANGUMI: i64 = 1 << 1;
const ATTR_CHEESE: i64 = 1 << 2;
const ATTR_LESSON: i64 = 1 << 18;

/// Keys that get filename sanitization (Python `update_episode_info`
/// filter; Windows-illegal set only, no control chars).
const SANITIZED_KEYS: &[&str] = &[
    "leaf_title",
    "parent_title",
    "source_title",
    "section_title",
    "collection_title",
    "series_title",
    "season_title",
    "episode_title",
    "favorites_owner",
    "space_owner",
];

/// Python `update_episode_info`: merge episode info with extra data and the
/// sequence number into one variable set.
///
/// Merge order matters: `number` last, so an explicit sequence number wins
/// over anything stale in extra data.
pub fn update_episode_info(
    episode_info: &mut serde_json::Map<String, serde_json::Value>,
    number: &serde_json::Value,
) -> serde_json::Map<String, serde_json::Value> {
    let episode_id = episode_info
        .get("episode_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let extra_data = EpisodeData::get_episode_data(&episode_id);

    let title = episode_info
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let attribute = episode_info
        .get("attribute")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    // Python mutates episode_info before spreading: leaf_title (and
    // episode_title on media-kind entries) always overwrite stale inputs.
    episode_info.insert(
        "leaf_title".into(),
        serde_json::Value::String(title.clone()),
    );
    if attribute & (ATTR_BANGUMI | ATTR_CHEESE | ATTR_LESSON) != 0 {
        episode_info.insert("episode_title".into(), serde_json::Value::String(title));
    }

    let mut merged = episode_info.clone();

    for (key, value) in extra_data {
        merged.insert(key, value);
    }

    // related_titles / uploader_info are flattened into top-level keys and
    // override both the episode payload and the source-level extra data.
    for subkey in ["related_titles", "uploader_info"] {
        if let Some(serde_json::Value::Object(sub)) = episode_info.get(subkey) {
            for (key, value) in sub {
                merged.insert(key.clone(), value.clone());
            }
        }
    }

    merged.insert("number".to_string(), number.clone());

    sanitize_keys(&mut merged, SANITIZED_KEYS);

    merged
}

fn sanitize_keys(merged: &mut serde_json::Map<String, serde_json::Value>, keys: &[&str]) {
    for key in keys {
        if let Some(serde_json::Value::String(s)) = merged.get_mut(*key) {
            *s = s
                .chars()
                .map(|c| {
                    if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                        '_'
                    } else {
                        c
                    }
                })
                .collect();
        }
    }
}
