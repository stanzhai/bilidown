//! `SubtitlesParser` (`additional/subtitles.py`): subtitle list selection,
//! per-format export and MKV subtitle-track registration.

use serde_json::Value;

use database::TaskInfo;
use network::HttpClient;

use crate::ass::{self, SubtitleStyle};
use crate::options::{resolve, resolve_bool};
use crate::writer::{self, Contents};
use crate::Labels;
use config::ConfigStore;

/// B 站 AI 生成字幕的语言码前缀，如 ai-zh、ai-es
pub const AI_LANGUAGE_PREFIX: &str = "ai-";

/// MKV 的 Language 元素期望 ISO 639-2 三字母码。
pub const LANGUAGE_CODE_MAP: &[(&str, &str)] = &[
    ("zh", "chi"),
    ("zh-cn", "chi"),
    ("zh-hans", "chi"),
    ("zh-hant", "chi"),
    ("zh-hk", "chi"),
    ("zh-tw", "chi"),
    ("en", "eng"),
    ("en-us", "eng"),
    ("en-gb", "eng"),
    ("ja", "jpn"),
    ("jp", "jpn"),
    ("ko", "kor"),
    ("es", "spa"),
    ("ar", "ara"),
    ("pt", "por"),
    ("fr", "fre"),
    ("de", "ger"),
    ("ru", "rus"),
    ("it", "ita"),
    ("vi", "vie"),
    ("th", "tha"),
    ("id", "ind"),
    ("ms", "may"),
    ("hi", "hin"),
    ("tr", "tur"),
    ("pl", "pol"),
    ("nl", "dut"),
    ("tl", "tgl"),
];

pub fn to_iso_639_2(language: &str) -> String {
    let key = language.to_lowercase();
    if let Some((_, code)) = LANGUAGE_CODE_MAP.iter().find(|(k, _)| *k == key) {
        return (*code).to_owned();
    }
    // AI 字幕的语言码形如 ai-zh、ai-es，剥掉前缀后再查一次
    if let Some(rest) = key.strip_prefix(AI_LANGUAGE_PREFIX) {
        if let Some((_, code)) = LANGUAGE_CODE_MAP.iter().find(|(k, _)| *k == rest) {
            return (*code).to_owned();
        }
    }
    // 表中没有的原样传给 FFmpeg，不用 und 兜底，避免丢掉已知的语言信息
    language.to_owned()
}

/// One fetched subtitle entry: `language` (the `lan` code),
/// `language_doc` (readable name, used as the embedded track title) and the
/// fetched document body.
#[derive(Debug, Clone)]
pub struct SubtitleEntry {
    pub language: String,
    pub language_doc: String,
    pub data: Value,
}

/// `_get_subtitles_data_list`: filter by the configured language allow-list
/// and fetch each subtitle document. Entries whose fetch returns a falsy
/// body are dropped, mirroring Python's `if data:`.
pub async fn get_subtitles_data_list(
    client: &HttpClient,
    player_data: &Value,
    language_config: &Value,
) -> anyhow::Result<Vec<SubtitleEntry>> {
    let mut list = Vec::new();
    let subtitles = player_data
        .get("subtitle")
        .and_then(|s| s.get("subtitles"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let download_specified = language_config
        .get("download_specified")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let specified: Vec<String> = language_config
        .get("specified_language")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();

    for entry in subtitles {
        let language = entry
            .get("lan")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if download_specified && !specified.iter().any(|s| s == &language) {
            continue;
        }
        // Protocol-relative URLs from the API become https.
        let raw = entry
            .get("subtitle_url")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let url = if raw.starts_with("http") {
            raw.to_owned()
        } else {
            format!("https:{raw}")
        };
        let body = client.get(&url, &[]).await?;
        let data: Value = serde_json::from_str(&body)?;
        if crate::python_truthy(&data) {
            list.push(SubtitleEntry {
                language_doc: entry
                    .get("lan_doc")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&language)
                    .to_owned(),
                language,
                data,
            });
        }
    }
    Ok(list)
}

/// `_to_srt`: numbered cues with `H:MM:SS,mmm` timestamps, a blank separator
/// after each content line, then the outer `strip()`.
pub fn to_srt(data: &Value) -> String {
    let mut lines = Vec::new();
    if let Some(body) = data.get("body").and_then(Value::as_array) {
        for (i, item) in body.iter().enumerate() {
            let from = item.get("from").and_then(Value::as_f64).unwrap_or(0.0);
            let to = item.get("to").and_then(Value::as_f64).unwrap_or(0.0);
            let content = item.get("content").and_then(Value::as_str).unwrap_or("");
            lines.push(format!("{}", i + 1));
            lines.push(format!(
                "{} --> {}",
                crate::time::format_srt_time(from),
                crate::time::format_srt_time(to)
            ));
            lines.push(format!("{content}\n"));
        }
    }
    lines.join("\n").trim().to_owned()
}

/// `_to_lrc`: `[MM:SS.ss]content` with a 5-wide 2-decimal seconds field.
pub fn to_lrc(data: &Value) -> String {
    let mut lines = Vec::new();
    if let Some(body) = data.get("body").and_then(Value::as_array) {
        for item in body {
            let start = item.get("from").and_then(Value::as_f64).unwrap_or(0.0);
            let content = item.get("content").and_then(Value::as_str).unwrap_or("");
            let m = (start / 60.0) as i64;
            let s = start % 60.0;
            lines.push(format!("[{m:02}:{s:05.2}]{content}"));
        }
    }
    lines.join("\n").trim().to_owned()
}

/// `_to_txt`: contents joined by newlines, stripped.
pub fn to_txt(data: &Value) -> String {
    let mut lines = Vec::new();
    if let Some(body) = data.get("body").and_then(Value::as_array) {
        for item in body {
            lines.push(
                item.get("content")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            );
        }
    }
    lines.join("\n").trim().to_owned()
}

/// `_to_json`: pretty JSON with 2-space indent, raw UTF-8.
pub fn to_json(data: &Value) -> String {
    serde_json::to_string_pretty(data).expect("subtitle data is JSON-serializable")
}

/// `parse`: export every selected subtitle in the configured format and
/// register ASS tracks for MKV embedding.
pub async fn parse(
    client: &HttpClient,
    task: &mut TaskInfo,
    store: &ConfigStore,
    labels: &Labels,
    player_data: &Value,
) -> anyhow::Result<()> {
    let language_config = resolve(task, store, "subtitle_language");
    let list = get_subtitles_data_list(client, player_data, &language_config).await?;
    let subtitle_type = resolve(task, store, "subtitle_type")
        .as_str()
        .unwrap_or("ass")
        .to_owned();
    let style = SubtitleStyle::from_config(
        &store
            .get("Additional", "subtitle_style")
            .unwrap_or(Value::Null),
    );

    for entry in &list {
        let (contents, suffix) = match subtitle_type.as_str() {
            "srt" => (to_srt(&entry.data), "srt"),
            "lrc" => (to_lrc(&entry.data), "lrc"),
            "txt" => (to_txt(&entry.data), "txt"),
            "ass" => (
                ass::generate(&entry.data, &task.Basic.show_title, &style),
                "ass",
            ),
            _ => (to_json(&entry.data), "json"),
        };
        let name = task.File.name.clone();
        let file_name = writer::write(
            task,
            Contents::Text(&contents),
            suffix,
            Some(&name),
            &[&labels.subtitles_qualifier, &entry.language],
        )?;
        check_embed_subtitle(task, store, &subtitle_type, &file_name, entry, labels);
    }
    Ok(())
}

fn check_embed_subtitle(
    task: &mut TaskInfo,
    store: &ConfigStore,
    subtitle_type: &str,
    file_name: &str,
    entry: &SubtitleEntry,
    labels: &Labels,
) {
    // 仅 ASS 格式能作为字幕轨嵌入，其余格式即便开着开关也静默跳过
    if subtitle_type != "ass" {
        return;
    }
    if !resolve_bool(task, store, "embed_subtitle") || !writer::is_embed_available(task) {
        return;
    }
    let mut track = serde_json::Map::new();
    track.insert("file".to_owned(), Value::String(file_name.to_owned()));
    track.insert(
        "title".to_owned(),
        Value::String(track_title(entry, labels)),
    );
    track.insert(
        "language".to_owned(),
        Value::String(to_iso_639_2(&entry.language)),
    );
    track.insert("kind".to_owned(), Value::String("subtitle".to_owned()));
    task.File.subtitle_track_list.push(track);
}

fn track_title(entry: &SubtitleEntry, labels: &Labels) -> String {
    // B 站对 AI 字幕的 lan_doc 就是纯语言名，只有 lan 里的 ai- 前缀能区分
    let title = entry.language_doc.clone();
    if entry
        .language
        .to_lowercase()
        .starts_with(AI_LANGUAGE_PREFIX)
    {
        return labels.ai_track_title.replace("{name}", &title);
    }
    title
}
