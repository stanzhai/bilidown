//! Episode → TaskInfo assembly (Python `__episode_info_to_task_info`,
//! `__update_episode_info`, `__filter_illegal_characters`,
//! `__determine_download_type`, `__trim_download_type`,
//! `__freeze_naming_rule`, cover id, created time).
//!
//! Task Info is frozen at creation: quality ids, download type, options
//! snapshot, download path and naming snapshot all come from the moment
//! `create()` ran, never re-read later.

use crate::options::{apply_snapshot, pick_option, snapshot};
use crate::status::{download_status, download_type};
use config::ConfigStore;
use database::info::TaskInfo;
use md5::{Digest, Md5};
use parking_lot::Mutex;
use parsers::EpisodeData;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

/// `cover_manager._calc_cover_id`: md5 of the cover URL.
pub fn calc_cover_id(cover_url: &str) -> String {
    format!("{:x}", Md5::digest(cover_url.as_bytes()))
}

/// Module-global monotonic millisecond clock (Python `get_timestamp_ms`:
/// strictly increasing even when the wall clock stalls).
static LAST_TIMESTAMP_MS: Mutex<i64> = Mutex::new(0);

pub fn get_timestamp_ms() -> i64 {
    let mut last = LAST_TIMESTAMP_MS.lock();
    let mut timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    if timestamp <= *last {
        timestamp = *last + 1;
    }
    *last = timestamp;
    timestamp
}

/// `Attribute` bits used here (mirrors `parsers::tree`, spelled locally
/// so raw episode dicts need no Attribute wrapper).
const BANGUMI_BIT: i64 = 1 << 1;
const CHEESE_BIT: i64 = 1 << 2;
const NEED_PARSE_BIT: i64 = 1 << 7;
const LESSON_BIT: i64 = 1 << 18;

/// Filesystem-illegal characters replaced with `_` (Python
/// `re.sub(r'[\/\\\:\*\?\"\<\>\|]', '_', ...)`). No regex needed: a
/// char-set membership test.
pub fn filter_illegal_characters(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}

/// Episode side-data store + config inputs frozen for one `create()` call.
pub struct CreateInputs<'a> {
    pub config: &'a ConfigStore,
    pub episode_data: &'a EpisodeData,
    /// Runtime download defaults (Python `runtime.download`): the quality
    /// ids used when the options dict does not override them.
    pub video_quality_id: i64,
    pub audio_quality_id: i64,
    pub video_codec_id: i64,
}

/// Inputs for the naming step. The naming engine is a later ticket, so
/// the caller supplies the already-formatted relative path (folder +
/// file name) plus the rule snapshot to freeze verbatim.
#[derive(Debug, Clone, Default)]
pub struct NamingInput {
    /// `naming_rule_ids` map from options (type_id -> rule_id).
    pub rule_id: Option<String>,
    /// Rule template frozen verbatim from options (`naming_rule`).
    pub rule: Option<String>,
    /// Caller-resolved ConventionType for the episode, if known.
    pub type_id: Option<i64>,
    /// Formatted relative path (`folder/file.ext`), naming engine output.
    pub formatted_path: Option<String>,
}

impl Default for CreateInputs<'_> {
    fn default() -> Self {
        unreachable!("CreateInputs requires config + episode data")
    }
}

/// `__update_episode_info`: merged episode dict fed into `EpisodeInfo`.
/// Merge order: episode dict ← extra side data ← related_titles ←
/// uploader_info ← `number`; then the illegal-char filter on the title
/// keys that are present.
pub fn update_episode_info(
    episode: &serde_json::Map<String, Value>,
    episode_data: &EpisodeData,
    number: &str,
) -> serde_json::Map<String, Value> {
    let mut data = episode.clone();
    let title = data
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let attribute = data.get("attribute").and_then(Value::as_i64).unwrap_or(0);

    // 任何类型都存一份 leaf_title 备用
    data.insert("leaf_title".into(), Value::String(title.clone()));

    // 剧集与课程：episode_title 表示剧集名/课程名
    if attribute & (BANGUMI_BIT | CHEESE_BIT | LESSON_BIT) != 0 {
        data.insert("episode_title".into(), Value::String(title));
    }

    let episode_id = data
        .get("episode_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let extra = episode_data.get_episode_data(&episode_id);
    if let Some(map) = extra.as_object() {
        for (k, v) in map {
            data.insert(k.clone(), v.clone());
        }
    }
    for source in ["related_titles", "uploader_info"] {
        let overlay = data
            .get(source)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for (k, v) in overlay {
            data.insert(k, v);
        }
    }
    data.insert("number".into(), Value::String(number.to_string()));

    for key in [
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
    ] {
        if let Some(v) = data.get(key).and_then(Value::as_str) {
            data.insert(key.to_string(), Value::String(filter_illegal_characters(v)));
        }
    }
    data
}

/// `__determine_download_type`: bits from per-call options with config
/// fallbacks (`Download` group for the two stream toggles, `Additional`
/// for the rest).
pub fn determine_download_type(
    config: &ConfigStore,
    options: Option<&serde_json::Map<String, Value>>,
) -> i64 {
    let flag = |option_key: &str, group: &str| -> bool {
        let fallback = config.get(group, option_key).unwrap_or(Value::Bool(false));
        pick_option(options, option_key, &fallback)
            .as_bool()
            .unwrap_or(false)
    };
    let mut bits = 0i64;
    if flag("download_video_stream", "Download") {
        bits |= download_type::VIDEO;
    }
    if flag("download_audio_stream", "Download") {
        bits |= download_type::AUDIO;
    }
    if flag("download_danmaku", "Additional") {
        bits |= download_type::DANMAKU;
    }
    if flag("download_subtitle", "Additional") {
        bits |= download_type::SUBTITLE;
    }
    if flag("download_cover", "Additional") {
        bits |= download_type::COVER;
    }
    if flag("download_metadata", "Additional") {
        bits |= download_type::METADATA;
    }
    if flag("embed_chapter", "Additional") {
        bits |= download_type::CHAPTER;
    }
    bits
}

/// `__trim_download_type`: LESSON entries carry no aid/cid/cover.
pub fn trim_download_type(task_info: &mut TaskInfo) {
    if task_info.Episode.attribute & LESSON_BIT != 0 {
        task_info.Episode.attribute &= !0; // no-op, keep attribute intact
        task_info.Download.type_ &= !(download_type::DANMAKU
            | download_type::SUBTITLE
            | download_type::CHAPTER
            | download_type::COVER);
    }
}

/// `__freeze_naming_rule` + `__update_file_name_info` fused: store the
/// rule snapshot verbatim and the caller-supplied formatted path. With no
/// formatted path the rule is frozen from config and rendered through the
/// naming engine (Python always names at creation).
pub fn apply_naming(
    task_info: &mut TaskInfo,
    naming: &NamingInput,
    config: &ConfigStore,
) -> anyhow::Result<()> {
    task_info.Naming.rule_id = naming.rule_id.clone();
    task_info.Naming.rule = naming.rule.clone();
    task_info.Naming.type_id = naming.type_id;

    if let Some(path) = &naming.formatted_path {
        // Path::new(path).name / parent semantics on a normalized string.
        let (folder, name) = match path.rfind('/') {
            Some(i) => (&path[..i], &path[i + 1..]),
            None => ("", path.as_str()),
        };
        task_info.File.name = name.to_string();
        task_info.File.folder = folder.to_string();
    } else {
        // Python always freezes + formats at creation
        // (`__freeze_naming_rule` → `__update_file_name_info`). Callers
        // that supply no formatted path (the UI default) get the same
        // treatment here from the config's rule store.
        let store = naming::naming_rules::RuleStore::from_config_value(
            config.get("File Naming", "naming_rule_list").as_ref(),
        );
        if task_info.Naming.rule.as_deref().unwrap_or("").is_empty() {
            naming::task_naming::freeze_naming_rule(task_info, None, &store, &Default::default());
        }
        if let Err(e) = naming::task_naming::update_file_name_info(task_info, &store) {
            return Err(anyhow::anyhow!("{e}"));
        }
    }
    Ok(())
}

fn bool_config(config: &ConfigStore, key: &str) -> Value {
    config.get("Download", key).unwrap_or(Value::Bool(false))
}

/// `__episode_info_to_task_info`: one fully-frozen TaskInfo.
#[allow(clippy::too_many_arguments)]
pub fn episode_to_task_info(
    inputs: &CreateInputs<'_>,
    episode: &serde_json::Map<String, Value>,
    number: &str,
    options: Option<&serde_json::Map<String, Value>>,
    naming: &NamingInput,
) -> anyhow::Result<TaskInfo> {
    let config = inputs.config;
    let mut task_info = TaskInfo::default();

    // BasicInfo
    task_info.Basic.task_id = uuid::Uuid::new_v4().to_string();
    task_info.Basic.cover_id = calc_cover_id(
        episode
            .get("cover")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    task_info.Basic.show_title = episode
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    task_info.Basic.created_time = get_timestamp_ms();

    // DownloadInfo
    task_info.Download.status = download_status::QUEUED;
    task_info.Download.type_ = determine_download_type(config, options);
    task_info.Download.video_quality_id = pick_option(
        options,
        "video_quality_id",
        &Value::from(inputs.video_quality_id),
    )
    .as_i64()
    .unwrap_or(inputs.video_quality_id);
    task_info.Download.audio_quality_id = pick_option(
        options,
        "audio_quality_id",
        &Value::from(inputs.audio_quality_id),
    )
    .as_i64()
    .unwrap_or(inputs.audio_quality_id);
    task_info.Download.video_codec_id = pick_option(
        options,
        "video_codec_id",
        &Value::from(inputs.video_codec_id),
    )
    .as_i64()
    .unwrap_or(inputs.video_codec_id);
    task_info.Download.merge_video_audio = pick_option(
        options,
        "merge_video_audio",
        &bool_config(config, "merge_video_audio"),
    )
    .as_bool()
    .unwrap_or(true);
    task_info.Download.keep_original_files = pick_option(
        options,
        "keep_original_files",
        &bool_config(config, "keep_original_files"),
    )
    .as_bool()
    .unwrap_or(false);

    // EpisodeInfo: start from serialized defaults so required fields the
    // parse dict omits fall back, overlay the merged parse values, and
    // drop keys EpisodeInfo does not model (it denies unknown fields).
    let merged = update_episode_info(episode, inputs.episode_data, number);
    let mut object = serde_json::to_value(TaskInfo::default().Episode)
        .ok()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    for (key, value) in &merged {
        if object.contains_key(key) {
            object.insert(key.clone(), value.clone());
        }
    }
    task_info.Episode = serde_json::from_value(Value::Object(object))?;
    trim_download_type(&mut task_info);

    // OptionsInfo
    apply_snapshot(&mut task_info, &snapshot(config, options));

    // FileInfo: download path frozen at creation.
    task_info.File.download_path = config
        .get("Download", "download_path")
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();

    // NamingInfo
    apply_naming(&mut task_info, naming, config)?;

    Ok(task_info)
}

/// True when the episode needs a second parse pass (NEED_PARSE_BIT);
/// reported back to the caller — this crate has no reparse worker.
pub fn needs_reparse(episode: &serde_json::Map<String, Value>) -> bool {
    episode
        .get("attribute")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        & NEED_PARSE_BIT
        != 0
}
