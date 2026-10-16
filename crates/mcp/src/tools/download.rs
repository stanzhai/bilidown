//! Download tools (`src/util/mcp/tools/download.py`): create_download,
//! pause/resume/cancel.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use serde_json::{json, Map, Value};

use parsers::playurl::{AUDIO_QUALITY_MAP, VIDEO_CODEC_MAP, VIDEO_QUALITY_MAP};
use parsers::tree::Attribute;
use tasks::status::download_status as status;

use crate::protocol::{ToolOutput, ToolSchema};
use crate::state::McpState;
use crate::tools;

const MAX_EPISODES_PER_CALL: usize = 200;

/// Models do not copy enum spellings verbatim (hi_res / Hi-Res / HIRES all
/// appear); fold before comparing so one underscore isn't an error.
fn fold(value: &str) -> String {
    value.trim().to_uppercase().replace([' ', '-', '_'], "")
}

/// `AVC/H.264` → each half recognized separately (model writes one half).
static VIDEO_CODEC_LOOKUP: LazyLock<HashMap<String, i64>> = LazyLock::new(|| {
    let mut lookup: HashMap<String, i64> = VIDEO_CODEC_MAP
        .iter()
        .map(|(name, value)| (fold(name), *value))
        .collect();
    for (name, value) in [
        ("AVC", 7),
        ("H.264", 7),
        ("HEVC", 12),
        ("H.265", 12),
        ("AV1", 13),
    ] {
        lookup.insert(fold(name), value);
    }
    lookup
});

/// The model-visible names for each table, in declaration order (error
/// messages list them exactly).
const VIDEO_QUALITY_NAMES: &[&str] = &[
    "auto",
    "8K",
    "DOLBY_VISION",
    "HDR",
    "4K_SDR",
    "4K",
    "1080P60",
    "1080P+",
    "AI",
    "1080P",
    "720P",
    "480P",
    "360P",
];
const AUDIO_QUALITY_NAMES: &[&str] = &["auto", "HI_RES", "DOLBY_ATMOS", "192K", "132K", "64K"];
const VIDEO_CODEC_NAMES: &[&str] = &["auto", "AVC/H.264", "HEVC/H.265", "AV1"];
const VIDEO_CONTAINERS: &[&str] = &["mp4", "mkv"];
const DANMAKU_TYPES: &[&str] = &["xml", "ass", "json"];
const SUBTITLE_TYPES: &[&str] = &["srt", "lrc", "txt", "ass", "json"];
const COVER_TYPES: &[&str] = &["jpg", "png", "avif", "webp"];
const METADATA_TYPES: &[&str] = &["nfo", "json"];

/// The media-stream取舍 (audio-only for listening/transcription is common).
const MEDIA_CHOICES: &str = "video+audio, video, audio";

/// Model-visible name → TaskManager key. This mapping is also the
/// whitelist: thread counts, download dir, naming rules stay out of reach.
const FLAG_OPTIONS: &[(&str, &str)] = &[
    ("danmaku", "download_danmaku"),
    ("subtitle", "download_subtitle"),
    ("cover", "download_cover"),
    ("metadata", "download_metadata"),
    ("chapter", "embed_chapter"),
    ("embed_danmaku", "embed_danmaku"),
    ("embed_subtitle", "embed_subtitle"),
    ("attach_cover", "attach_cover"),
];

fn csv(names: &[&str]) -> String {
    names.join(", ")
}

fn pick_option<'a>(
    options: &'a Map<String, Value>,
    key: &str,
    fallback: Option<&'a Value>,
) -> Option<&'a Value> {
    options.get(key).or(fallback)
}

/// Translate model-facing options into TaskManager's options.
/// Bad values error instead of silently defaulting — the model believes it
/// asked for 4K and would have no way to notice the fallback.
fn normalize_options(
    state: &McpState,
    raw: Option<&Value>,
) -> Result<Option<Map<String, Value>>, String> {
    let Some(raw) = raw else { return Ok(None) };
    let Some(raw_map) = raw.as_object() else {
        return Err("The 'options' argument must be an object.".to_owned());
    };

    let mut options = Map::new();

    // (name, key, error-value list, resolver) — tables are `&'static str`
    // keyed and small; a folded linear scan beats a second map.
    for (name, key, choices, table) in [
        (
            "video_quality",
            "video_quality_id",
            VIDEO_QUALITY_NAMES,
            Some(&*VIDEO_QUALITY_MAP),
        ),
        (
            "audio_quality",
            "audio_quality_id",
            AUDIO_QUALITY_NAMES,
            Some(&*AUDIO_QUALITY_MAP),
        ),
        ("video_codec", "video_codec_id", VIDEO_CODEC_NAMES, None),
    ] {
        let Some(value) = raw_map.get(name) else {
            continue;
        };
        let Some(text) = value.as_str() else {
            return Err(format!("'{name}' must be a string."));
        };
        let resolved =
            table.map_or_else(|| resolve_codec(text), |table| resolve_quality(table, text));
        let Some(resolved) = resolved else {
            return Err(format!(
                "'{text}' is not a valid {name}. Valid values are: {}.",
                csv(choices)
            ));
        };
        options.insert(key.to_owned(), json!(resolved));
    }

    for (name, key) in FLAG_OPTIONS {
        let Some(value) = raw_map.get(*name) else {
            continue;
        };
        let Some(flag) = value.as_bool() else {
            return Err(format!("'{name}' must be a boolean."));
        };
        options.insert((*key).to_owned(), json!(flag));
    }

    if let Some(media) = raw_map.get("media") {
        let Some(text) = media.as_str().map(str::trim) else {
            return Err(format!("'media' must be one of: {MEDIA_CHOICES}."));
        };
        let (video, audio) = match text.to_lowercase().as_str() {
            "video+audio" => (true, true),
            "video" => (true, false),
            "audio" => (false, true),
            _ => return Err(format!("'media' must be one of: {MEDIA_CHOICES}.")),
        };
        options.insert("download_video_stream".to_owned(), json!(video));
        options.insert("download_audio_stream".to_owned(), json!(audio));
    }

    for (name, key, choices) in [
        ("danmaku_format", "danmaku_type", DANMAKU_TYPES),
        ("subtitle_format", "subtitle_type", SUBTITLE_TYPES),
        ("cover_format", "cover_type", COVER_TYPES),
        ("metadata_format", "metadata_type", METADATA_TYPES),
        ("container", "video_container", VIDEO_CONTAINERS),
    ] {
        let Some(value) = raw_map.get(name) else {
            continue;
        };
        let Some(text) = value.as_str() else {
            return Err(format!("'{name}' must be a string."));
        };
        let Some(resolved) = choices
            .iter()
            .find(|choice| fold(choice) == fold(text))
            .copied()
        else {
            return Err(format!(
                "'{text}' is not a valid {name}. Valid values are: {}.",
                csv(choices)
            ));
        };
        options.insert((*key).to_owned(), json!(resolved));
    }

    if let Some(languages) = raw_map.get("subtitle_languages") {
        let Some(list) = languages.as_array() else {
            return Err(
                "'subtitle_languages' must be an array of language code strings.".to_owned(),
            );
        };
        if !list.iter().all(Value::is_string) {
            return Err(
                "'subtitle_languages' must be an array of language code strings.".to_owned(),
            );
        }
        // An empty array means unrestricted — same as the settings page's
        // "download all languages".
        options.insert(
            "subtitle_language".to_owned(),
            json!({
                "download_specified": !list.is_empty(),
                "specified_language": list,
            }),
        );
    }

    if let Some(error) = embed_conflict(state, &options) {
        return Err(error);
    }

    Ok((!options.is_empty()).then_some(options))
}

/// Quality name → id via the parser tables (folded comparison).
fn resolve_quality(table: &'static HashMap<&'static str, i64>, name: &str) -> Option<i64> {
    let folded = fold(name);
    table
        .iter()
        .find(|(key, _)| fold(key) == folded)
        .map(|(_, value)| *value)
}

fn resolve_codec(name: &str) -> Option<i64> {
    VIDEO_CODEC_LOOKUP.get(&fold(name)).copied()
}
/// Embedding danmaku/subtitles silently no-ops when any prerequisite is
/// missing (ASS format, MKV container, an actual merge step). Intercept
/// and name the missing piece — otherwise the model believes it embedded.
fn embed_conflict(state: &McpState, options: &Map<String, Value>) -> Option<String> {
    let config = state.config.lock();
    for (switch, format_key, ass_value, label) in [
        ("embed_danmaku", "danmaku_type", "ass", "danmaku"),
        ("embed_subtitle", "subtitle_type", "ass", "subtitle"),
    ] {
        if !options
            .get(switch)
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            continue;
        }

        // Unspecified options fall back to the user's current settings;
        // already ASS + MKV must not be an error.
        let format_fallback = config.get("Advanced", format_key).cloned();
        let container_fallback = config.get("Advanced", "video_container").cloned();

        let file_format = pick_option(options, format_key, format_fallback.as_ref())
            .and_then(Value::as_str)
            .unwrap_or_default();
        let container = pick_option(options, "video_container", container_fallback.as_ref())
            .and_then(Value::as_str)
            .unwrap_or_default();

        let mut missing = Vec::new();
        if file_format != ass_value {
            missing.push(format!(
                "{label}_format must be 'ass' (currently '{file_format}')"
            ));
        }
        if container != "mkv" {
            missing.push(format!("container must be 'mkv' (currently '{container}')"));
        }
        // Audio-only has no merge step, nothing to embed into.
        if options.get("download_video_stream") == Some(&json!(false)) {
            missing.push("media must include video".to_owned());
        }
        if !missing.is_empty() {
            return Some(format!(
                "Cannot embed the {label}: {}. Either set those options too, or drop \
                 embed_{label} and the {label} will be saved as a separate file.",
                missing.join("; ")
            ));
        }
    }
    None
}

/// (episodes, found ids, missing ids, needs-reparse ids).
type EpisodeMatch = (
    Vec<Map<String, Value>>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
);

/// Resolve position ids against the current parse list.
fn collect_episode_info(state: &McpState, episode_ids: &[String]) -> EpisodeMatch {
    let view = state.view.lock();
    let items = view.get_all_items();

    let mut found = Vec::new();
    let mut found_ids = Vec::new();
    let mut missing = Vec::new();
    let mut needs_reparse = Vec::new();

    for episode_id in episode_ids {
        let Ok(index) = episode_id.parse::<usize>() else {
            missing.push(episode_id.clone());
            continue;
        };
        let Some(item) = index.checked_sub(1).and_then(|i| items.get(i)) else {
            missing.push(episode_id.clone());
            continue;
        };

        // These items (favorites/space videos) have no cid yet; creating
        // tasks for them would fail.
        if item.attribute.0 & Attribute::NEED_PARSE_BIT.0 != 0 {
            needs_reparse.push(episode_id.clone());
            continue;
        }

        let mut dict = match item.to_dict() {
            Value::Object(map) => map,
            _ => continue,
        };
        // `to_dict` reads the item's episode uuid; tasks key off it, so
        // keep it, but surface the position id the model addressed.
        dict.insert("title".to_owned(), json!(item.title));
        found.push(dict);
        found_ids.push(episode_id.clone());
    }

    (found, found_ids, missing, needs_reparse)
}

fn tool_create_download(state: &McpState, arguments: &Map<String, Value>) -> ToolOutput {
    let Some(_guard) = state
        .create_guard
        .try_lock_for(std::time::Duration::from_secs(5))
    else {
        return ToolOutput::error("Another download request is being processed. Retry shortly.");
    };

    let Some(ids) = arguments.get("episode_ids").and_then(Value::as_array) else {
        return ToolOutput::error(
            "The 'episode_ids' argument must be a non-empty array of episode_id strings.",
        );
    };
    if ids.is_empty() {
        return ToolOutput::error(
            "The 'episode_ids' argument must be a non-empty array of episode_id strings.",
        );
    }
    if !ids.iter().all(Value::is_string) {
        return ToolOutput::error("Every entry in 'episode_ids' must be a string.");
    }
    let episode_ids: Vec<String> = ids
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();

    if episode_ids.len() > MAX_EPISODES_PER_CALL {
        return ToolOutput::error(format!(
            "Too many episodes in one call ({}); the limit is {MAX_EPISODES_PER_CALL}.",
            episode_ids.len()
        ));
    }

    let mut options = match normalize_options(state, arguments.get("options")) {
        Ok(options) => options.unwrap_or_default(),
        Err(error) => return ToolOutput::error(error),
    };

    let redownload = arguments.get("redownload").and_then(Value::as_bool);
    if arguments.get("redownload").is_some() && redownload.is_none() {
        return ToolOutput::error("The 'redownload' argument must be a boolean.");
    }
    let redownload = redownload.unwrap_or(false);

    // Validate ids before the media-info gate: a wrong id that reports
    // "media unavailable" sends the model off to re-parse instead.
    let (mut found, found_ids, missing, needs_reparse) = collect_episode_info(state, &episode_ids);

    if found.is_empty() {
        let mut detail = Vec::new();
        if !missing.is_empty() {
            detail.push(format!(
                "{} id(s) were not found in the parse list",
                missing.len()
            ));
        }
        if !needs_reparse.is_empty() {
            detail.push(format!(
                "{} id(s) need to be parsed individually first",
                needs_reparse.len()
            ));
        }
        return ToolOutput::error(if !detail.is_empty() {
            format!("No downloadable episodes matched. {}.", detail.join("; "))
        } else {
            "No downloadable episodes matched. Call get_episodes to see the available episode_id values.".to_owned()
        });
    }

    if let Some(reason) = crate::tools::parse::media_info_error(&state.preview.lock().clone()) {
        return ToolOutput::error(format!(
            "Cannot start a download: {reason}. Try parsing the link again; if it keeps failing, \
             the content may require a login or be region-restricted."
        ));
    }

    // Duplicate handling must be decided here: ALWAYS_ASK would block
    // forever on this headless chain.
    options.insert(
        "duplicate_resolution".to_owned(),
        json!(if redownload { 0 } else { 1 }),
    );

    let mut duplicates: Vec<String> = Vec::new();
    if !redownload {
        // Pre-check instead of letting TaskManager skip silently: skipped
        // items never appear in `created` and the model would never learn
        // which ones were duplicates.
        let mut fresh = Vec::new();
        let mut fresh_ids = Vec::new();
        for (episode, episode_id) in found.into_iter().zip(found_ids.iter()) {
            match state.manager.is_duplicate(&episode) {
                Ok(true) => duplicates.push(
                    episode
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                ),
                Ok(false) => {
                    fresh.push(episode);
                    fresh_ids.push(episode_id.clone());
                }
                // DB read failed → treat as fresh; TaskManager re-checks
                // anyway, better than locking episodes out.
                Err(_) => {
                    fresh.push(episode);
                    fresh_ids.push(episode_id.clone());
                }
            }
        }
        found = fresh;
        if found.is_empty() {
            return ToolOutput::with_structured(
                format!(
                    "Nothing to download: all {} episode(s) have already been downloaded. \
                     Duplicates are matched by video id alone, so quality and format are not \
                     taken into account. Pass redownload=true to download them again.",
                    duplicates.len()
                ),
                json!({"created": 0, "duplicates": duplicates}),
            );
        }
    }

    let created = {
        let config = state.config.lock();
        state
            .manager
            .create(&config, &state.episode_data, &found, Some(&options), |_| {
                tasks::info::NamingInput::default()
            })
    };
    let Ok(result) = created else {
        return ToolOutput::error(
            "Task creation failed; the parse data may be stale. Try parse_url again.",
        );
    };

    // The "downloaded" badge — position-exact: multi-P parts share a bvid,
    // marking by bvid would flag all parts when only one was downloaded.
    state.downloaded.lock().extend(found_ids.iter().cloned());

    let mut notes = Vec::new();
    if !missing.is_empty() {
        let sample: Vec<&str> = missing.iter().take(5).map(String::as_str).collect();
        notes.push(format!(
            "{} id(s) were not found: {}",
            missing.len(),
            sample.join(", ")
        ));
    }
    if !needs_reparse.is_empty() {
        notes.push(format!(
            "{} id(s) need to be parsed individually before downloading",
            needs_reparse.len()
        ));
    }
    if !duplicates.is_empty() {
        notes.push(format!(
            "{} episode(s) were skipped as already downloaded (pass redownload=true to \
             download them again)",
            duplicates.len()
        ));
    }
    for (title, error) in &result.failed {
        notes.push(format!("failed to create '{title}': {error}"));
    }

    let tasks: Vec<Value> = result
        .created
        .iter()
        .map(|t| json!({"task_id": t.Basic.task_id, "title": t.Basic.show_title}))
        .collect();

    let mut summary = format!("Created {} download task(s).", tasks.len());
    if !notes.is_empty() {
        summary.push_str(&format!(" {}.", notes.join("; ")));
    }

    let mut structured = json!({
        "created": tasks.len(),
        "tasks": tasks,
        "notes": notes,
    });
    if !duplicates.is_empty() {
        structured["duplicates"] = json!(duplicates);
    }

    ToolOutput::with_structured(summary, structured)
}

/// pause/resume/cancel over the scheduler (`DownloadingListModel`
/// togglePauseResume / cancelDownload without the GUI).
fn control_task(state: &McpState, task_id: &str, action: &str) -> Option<String> {
    let Ok(Some(task)) = state.manager.query_by_id(task_id) else {
        return Some(format!(
            "No active task with id '{task_id}'. Only tasks that are still in the download \
             queue can be controlled."
        ));
    };

    let status_value = task.Download.status;
    match action {
        "pause" => {
            if status_value != status::DOWNLOADING {
                return Some(format!(
                    "Task is not downloading (current status: {}).",
                    status_label(status_value)
                ));
            }
            state.manager.scheduler.pause(task_id);
            let mut task = task;
            task.Download.status = status::PAUSED;
            state.manager.update_async(&task);
        }
        "resume" => {
            if status_value != status::PAUSED {
                return Some(format!(
                    "Task is not paused (current status: {}).",
                    status_label(status_value)
                ));
            }
            let mut task = task;
            task.Download.status = status::QUEUED;
            state.manager.update_async(&task);
            state.manager.scheduler.auto_manage(&state.manager);
        }
        "cancel" => {
            // Merging/converting tasks cannot be cancelled — same rule as
            // the GUI; everything else goes through cancelDownload.
            if matches!(status_value, s if s == status::MERGING || s == status::CONVERTING) {
                return Some(
                    "Task is being merged or converted and cannot be cancelled. Wait for it \
                     to finish."
                        .to_owned(),
                );
            }
            state.manager.scheduler.cancel(task_id);
            state.manager.cancel_many_async(&[task]);
        }
        _ => return Some(format!("Unknown action: {action}")),
    }
    None
}

fn status_label(value: i64) -> String {
    let name = match value {
        status::QUEUED => "queued",
        status::PARSING => "parsing",
        status::DOWNLOADING => "downloading",
        status::PAUSED => "paused",
        status::COMPLETED => "completed",
        status::FFMPEG_QUEUED => "ffmpeg_queued",
        status::MERGING => "merging",
        status::CONVERTING => "converting",
        status::ADDITIONAL_PROCESSING => "additional_processing",
        status::FAILED => "failed",
        status::FFMPEG_FAILED => "ffmpeg_failed",
        _ => return format!("unknown({value})"),
    };
    name.to_owned()
}

/// Past-tense for receipts — no `f"{action}d"` concatenation (that would
/// produce "canceld").
const ACTION_DONE: &[(&str, &str)] = &[
    ("pause", "paused"),
    ("resume", "resumed"),
    ("cancel", "cancelled"),
];

fn control_handler(state: &McpState, arguments: &Map<String, Value>, action: &str) -> ToolOutput {
    let task_id = tools::str_arg(arguments, "task_id")
        .unwrap_or_default()
        .trim();
    if task_id.is_empty() {
        return ToolOutput::error("The 'task_id' argument is required.");
    }

    if let Some(reason) = control_task(state, task_id, action) {
        return ToolOutput::error(reason);
    }

    let done = ACTION_DONE
        .iter()
        .find(|(name, _)| *name == action)
        .map(|(_, done)| *done)
        .unwrap_or(action);

    ToolOutput::with_structured(
        format!("Task {task_id} {done}."),
        json!({"task_id": task_id, "action": action}),
    )
}

pub fn download_schemas() -> Vec<ToolSchema> {
    vec![
        ToolSchema {
            name: "create_download",
            title: "Create Download Tasks",
            description: concat!(
                "Create download tasks for episodes currently in the parse list. Call parse_url ",
                "first, then pass the episode_id values you want. Anything left out of 'options' ",
                "follows the user's own settings, and 'options' applies only to the tasks created ",
                "by this call. The output folder and file naming are always the user's and cannot ",
                "be changed here. Episodes that were already downloaded are skipped unless ",
                "redownload is set."
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "episode_ids": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "episode_id values from parse_url or get_episodes.",
                        "minItems": 1,
                        "maxItems": MAX_EPISODES_PER_CALL,
                    },
                    "options": {
                        "type": "object",
                        "description": concat!(
                            "Per-task download settings. Omitted fields follow the user's ",
                            "settings. Check the 'available' field from parse_url first: ",
                            "requesting a quality the content does not offer silently falls ",
                            "back to the closest available one, so the download will not ",
                            "match what was asked for."
                        ),
                        "properties": {
                            "video_quality": {
                                "type": "string",
                                "enum": VIDEO_QUALITY_NAMES,
                                "description": "Video quality; 'auto' follows the user's priority list.",
                            },
                            "audio_quality": {
                                "type": "string",
                                "enum": AUDIO_QUALITY_NAMES,
                                "description": "Audio quality; 'auto' follows the user's priority list.",
                            },
                            "video_codec": {
                                "type": "string",
                                "enum": VIDEO_CODEC_NAMES,
                                "description": "Video codec; 'auto' follows the user's priority list.",
                            },
                            "media": {
                                "type": "string",
                                "enum": ["video+audio", "video", "audio"],
                                "description": "Which streams to download. Use 'audio' for audio-only.",
                            },
                            "container": {
                                "type": "string",
                                "enum": VIDEO_CONTAINERS,
                                "description": "Output container for the merged file.",
                            },
                            "danmaku": {"type": "boolean", "description": "Download the danmaku (bullet comments) file."},
                            "danmaku_format": {
                                "type": "string",
                                "enum": DANMAKU_TYPES,
                                "description": "Format of the danmaku file.",
                            },
                            "embed_danmaku": {
                                "type": "boolean",
                                "description": concat!(
                                    "Embed the danmaku into the video as a subtitle track. ",
                                    "Requires danmaku_format 'ass' and container 'mkv'."
                                ),
                            },
                            "subtitle": {"type": "boolean", "description": "Download the subtitle file."},
                            "subtitle_format": {
                                "type": "string",
                                "enum": SUBTITLE_TYPES,
                                "description": "Format of the subtitle file.",
                            },
                            "subtitle_languages": {
                                "type": "array",
                                "items": {"type": "string"},
                                "description": concat!(
                                    "Only download subtitles in these languages, using Bilibili's ",
                                    "own codes (zh-CN, zh-Hant, en-US, ai-zh ...). An empty array ",
                                    "downloads every available language. Codes the video does not ",
                                    "provide simply yield no subtitle file."
                                ),
                            },
                            "embed_subtitle": {
                                "type": "boolean",
                                "description": concat!(
                                    "Embed the subtitle into the video as a subtitle track. ",
                                    "Requires subtitle_format 'ass' and container 'mkv'."
                                ),
                            },
                            "cover": {"type": "boolean", "description": "Download the cover image."},
                            "cover_format": {
                                "type": "string",
                                "enum": COVER_TYPES,
                                "description": "Format of the cover image file.",
                            },
                            "attach_cover": {
                                "type": "boolean",
                                "description": "Attach the cover to the output file as embedded artwork.",
                            },
                            "metadata": {"type": "boolean", "description": "Write a metadata file."},
                            "metadata_format": {
                                "type": "string",
                                "enum": METADATA_TYPES,
                                "description": "Format of the metadata file.",
                            },
                            "chapter": {"type": "boolean", "description": "Embed chapter markers into the output file."},
                        },
                        "additionalProperties": false,
                    },
                    "redownload": {
                        "type": "boolean",
                        "description": concat!(
                            "Download episodes even if they were downloaded before. Duplicates are ",
                            "matched by video id alone, so an episode already downloaded at a ",
                            "different quality still counts as a duplicate; set this to download it ",
                            "again at another quality."
                        ),
                    },
                },
                "required": ["episode_ids"],
                "additionalProperties": false,
            }),
        },
        ToolSchema {
            name: "pause_task",
            title: "Pause Download",
            description: "Pause a download task that is currently downloading.",
            input_schema: task_id_schema(),
        },
        ToolSchema {
            name: "resume_task",
            title: "Resume Download",
            description: "Resume a paused download task.",
            input_schema: task_id_schema(),
        },
        ToolSchema {
            name: "cancel_task",
            title: "Cancel Download",
            description: concat!(
                "Cancel a download task and remove it from the queue. Partial files are deleted. ",
                "A task that is already merging or converting cannot be cancelled."
            ),
            input_schema: task_id_schema(),
        },
    ]
}

fn task_id_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "task_id": {
                "type": "string",
                "description": "The task_id returned by list_tasks or create_download.",
            },
        },
        "required": ["task_id"],
        "additionalProperties": false,
    })
}

pub fn dispatch(state: Arc<McpState>, name: &str, arguments: &Map<String, Value>) -> ToolOutput {
    match name {
        "create_download" => tool_create_download(&state, arguments),
        "pause_task" => control_handler(&state, arguments, "pause"),
        "resume_task" => control_handler(&state, arguments, "resume"),
        "cancel_task" => control_handler(&state, arguments, "cancel"),
        _ => ToolOutput::error(format!("Unknown download tool: {name}")),
    }
}
