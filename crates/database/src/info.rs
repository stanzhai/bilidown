//! Task Info JSON model (Python `TaskInfo` sections) persisted inside
//! `task.db` `data` columns. Field order matches the Python dataclass field
//! order so serialized rows byte-match what a Python build writes, and
//! unknown keys survive round-trips (old rows keep their extra fields).
//!
//! (spec: drop-in user-data compatibility, ticket 03)

use serde::{Deserialize, Serialize};

/// Basic section (`BasicInfo`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BasicInfo {
    pub task_id: String,
    pub cover_id: String,
    pub show_title: String,
    pub created_time: i64,
    pub completed_time: i64,
}

/// File section (`FileInfo`). `subtitle_track_list` items carry keys chosen
/// at parse time (`file`/`title`/`language`/`kind`); preserved verbatim.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FileInfo {
    pub name: String,
    pub download_path: String,
    pub folder: String,
    pub video_file_ext: String,
    pub audio_file_ext: String,
    pub merge_file_ext: String,
    #[serde(default)]
    pub relative_files: Vec<String>,
    #[serde(default)]
    pub subtitle_track_list: Vec<serde_json::Map<String, serde_json::Value>>,
}

/// Episode section (`EpisodeInfo`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpisodeInfo {
    pub attribute: i64,
    pub aid: i64,
    pub bvid: String,
    pub cid: i64,
    pub sid: i64,
    pub cover: String,
    pub ep_id: i64,
    pub pubtime: i64,
    pub favtime: i64,
    pub viewtime: i64,
    pub number: String,
    pub part_number: i64,
    pub episode_number: i64,
    pub leaf_title: String,
    pub parent_title: String,
    pub source_title: String,
    pub section_title: String,
    pub collection_title: String,
    pub series_title: String,
    pub season_title: String,
    pub episode_title: String,
    #[serde(default)]
    pub areas: Vec<String>,
    pub actors: String,
    pub description: String,
    pub episode_plot: String,
    pub uploader: String,
    pub uploader_uid: i64,
    pub uploader_face: String,
    pub premiered: i64,
    pub poster: String,
    pub season_id: i64,
    pub season_number: i64,
    #[serde(default)]
    pub styles: Vec<String>,
    pub rating: f64,
    pub rating_votes: i64,
    pub tid: i64,
    pub tid_v2: i64,
    pub url: String,
    pub duration: i64,
    #[serde(default)]
    pub tags: Vec<String>,
    pub new_ep_status: bool,
    pub favorites_name: String,
    pub favorites_id: i64,
    pub favorites_owner: String,
    pub favorites_owner_id: i64,
    pub space_owner: String,
    pub space_owner_id: i64,
    pub course_id: i64,
    pub lesson_id: i64,
    pub item_id: i64,
    pub section_id: i64,
    pub video_quality: String,
    pub audio_quality: String,
    pub video_codec: String,
}

/// Download section (`DownloadInfo`). `files` is an opaque map kept verbatim.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DownloadInfo {
    #[serde(rename = "type")]
    pub type_: i64,
    pub media_type: i64,
    pub speed: i64,
    pub progress: i64,
    pub total_size: i64,
    pub downloaded_size: i64,
    pub status: i64,
    pub video_quality_id: i64,
    pub audio_quality_id: i64,
    pub video_codec_id: i64,
    #[serde(default)]
    pub queue: Vec<String>,
    #[serde(default)]
    pub files: serde_json::Map<String, serde_json::Value>,
    pub merge_video_audio: bool,
    pub keep_original_files: bool,
    pub video_parts_count: i64,
    pub info_label: String,
    pub status_label: String,
}

/// Options section (`OptionsInfo`): per-task snapshot; `None` = not pinned,
/// falls back to global settings. Never give these concrete defaults.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionsInfo {
    pub video_container: Option<String>,
    pub danmaku_type: Option<String>,
    pub embed_danmaku: Option<bool>,
    pub delete_danmaku_after_embed: Option<bool>,
    pub subtitle_type: Option<String>,
    pub embed_subtitle: Option<bool>,
    pub delete_subtitle_after_embed: Option<bool>,
    pub subtitle_language: Option<serde_json::Map<String, serde_json::Value>>,
    pub cover_type: Option<String>,
    pub attach_cover: Option<bool>,
    pub delete_cover_after_attach: Option<bool>,
    pub metadata_type: Option<String>,
    pub m4a_to_mp3: Option<bool>,
    pub keep_original_files_type: Option<i64>,
}

/// Naming section (`NamingInfo`): rule snapshot; `None` = falls back to the
/// default rule for `type_id`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamingInfo {
    pub rule_id: Option<String>,
    pub rule: Option<String>,
    pub type_id: Option<i64>,
}

/// Whole task record: the six Python sections under their original names.
/// Section names match the Python dataclass attribute names (JSON keys).
#[allow(non_snake_case)]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskInfo {
    #[serde(default)]
    pub Basic: BasicInfo,
    #[serde(default)]
    pub File: FileInfo,
    #[serde(default)]
    pub Episode: EpisodeInfo,
    #[serde(default)]
    pub Download: DownloadInfo,
    #[serde(default)]
    pub Options: OptionsInfo,
    #[serde(default)]
    pub Naming: NamingInfo,
    /// Sections/keys an older or newer build wrote; preserved verbatim so
    /// round-trips through this build never lose data.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl TaskInfo {
    /// Serializes exactly like Python `dumps(task_info.to_dict())`:
    /// compact separators, raw UTF-8, insertion-ordered maps.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("TaskInfo is JSON-serializable")
    }

    pub fn from_json(s: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(s)?)
    }
}
