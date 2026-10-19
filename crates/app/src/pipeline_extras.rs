//! Additional-files stage (ticket 31, F8): port of
//! `src/util/parse/additional/worker.py.__parse` — danmaku → one player-info
//! fetch → subtitles → chapter → cover → metadata, gated by `Download.type_`
//! bits. Chapter has no Rust home yet, so its ffmetadata writer lives here.

use std::path::PathBuf;

use config::ConfigStore;
use database::TaskInfo;
use network::HttpClient;
use serde_json::Value;

use danmaku::{DanmakuFormat, DanmakuStyle};
use extras::writer::{self, Contents};
use extras::Labels;

use tasks::status::download_type;

/// Active i18n catalog from `Interface.language` (auto → system).
fn catalog(cfg: &ConfigStore) -> &'static services::i18n::Catalog {
    services::i18n::catalog(
        services::i18n::Language::from_config(
            &cfg.get("Interface", "language")
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_else(|| "Auto".into()),
        )
        .resolve(),
    )
}

/// Localized qualifier labels (Python `Translator.ADDITIONAL_FILES_QUALIFIER`
/// / `SUBTITLE_TRACK_TITLE`), resolved from the config's UI language.
pub fn default_labels(cfg: &ConfigStore) -> Labels {
    let c = catalog(cfg);
    Labels {
        subtitles_qualifier: services::i18n::additional_files_qualifier(c, "SUBTITLES")
            .unwrap_or_else(|| "Subtitles".into()),
        metadata_qualifier: services::i18n::additional_files_qualifier(c, "METADATA")
            .unwrap_or_else(|| "Metadata".into()),
        ai_track_title: services::i18n::subtitle_track_title(c, "AI_GENERATED")
            .unwrap_or_else(|| "{name} (AI Generated)".into()),
    }
}

/// `ChapterParser.get_file_name`.
fn chapter_file_name(task_id: &str) -> String {
    format!("chapter_{task_id}.txt")
}

/// `ChapterParser.is_available`: chapters ride the merge, so only a task
/// with a merge step needs the file.
pub fn chapter_available(task: &TaskInfo) -> bool {
    task.Download.type_ & download_type::CHAPTER != 0
        && (task.Download.merge_video_audio || task.Download.video_parts_count > 0)
}

/// `ChapterParser._escape`: ffmetadata specials.
fn escape_ffmetadata(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    for char in content.chars() {
        if char == '=' || char == ';' || char == '#' || char == '\\' || char == '\n' {
            out.push('\\');
        }
        out.push(char);
    }
    out
}

/// `ChapterParser._to_ffmetadata`.
pub fn to_ffmetadata(view_points: &[Value], duration: i64) -> String {
    let mut lines = vec![";FFMETADATA1".to_string(), String::new()];
    for (index, entry) in view_points.iter().enumerate() {
        let start = entry.get("from").and_then(Value::as_i64).unwrap_or(0);
        let mut end = entry.get("to").and_then(Value::as_i64).unwrap_or(0);
        if end <= start {
            end = view_points
                .get(index + 1)
                .and_then(|next| next.get("from"))
                .and_then(Value::as_i64)
                .unwrap_or(duration);
            if end <= start {
                continue;
            }
        }
        let title = entry
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        lines.push("[CHAPTER]".into());
        lines.push("TIMEBASE=1/1000".into());
        lines.push(format!("START={}", start * 1000));
        lines.push(format!("END={}", end * 1000));
        lines.push(format!("title={}", escape_ffmetadata(title)));
        lines.push(String::new());
    }
    lines.join("\n")
}

/// `ChapterParser.parse`: write `chapter_{task_id}.txt` in the task folder
/// (the merger finds and deletes it); no file when there are no view points.
pub fn write_chapter_file(task: &TaskInfo, player_data: &Value) -> anyhow::Result<()> {
    let Some(points) = player_data.get("view_points").and_then(Value::as_array) else {
        return Ok(());
    };
    if points.is_empty() {
        return Ok(());
    }
    let contents = to_ffmetadata(points, task.Episode.duration);
    let path = PathBuf::from(&task.File.download_path)
        .join(&task.File.folder)
        .join(chapter_file_name(&task.Basic.task_id));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, contents)?;
    Ok(())
}

/// The whole additional stage. Errors abort the task (Python surfaces the
/// worker's `error` signal the same way).
pub async fn run_additional(
    client: &HttpClient,
    cfg: &ConfigStore,
    task: &mut TaskInfo,
) -> Result<(), String> {
    let labels = default_labels(cfg);
    let attr = task.Download.type_;

    // Resume/retry re-runs this stage; clear first so an ASS file is not
    // registered for embedding twice.
    task.File.subtitle_track_list.clear();

    let need_subtitle = attr & download_type::SUBTITLE != 0;
    let need_chapter = chapter_available(task);

    if attr & download_type::DANMAKU != 0 {
        let format = match extras::options::resolve(task, cfg, "danmaku_type")
            .as_str()
            .unwrap_or("xml")
        {
            "ass" => DanmakuFormat::Ass,
            "json" => DanmakuFormat::Json,
            _ => DanmakuFormat::Xml,
        };
        let entries = danmaku::fetch_segments(client, "", task.Episode.cid, task.Episode.duration)
            .await
            .map_err(|e| e.to_string())?;
        let style = DanmakuStyle::default();
        let contents = danmaku::render(
            &entries,
            format,
            task.Episode.cid,
            &task.Basic.show_title,
            &style,
        );
        let suffix = match format {
            DanmakuFormat::Xml => "xml",
            DanmakuFormat::Ass => "ass",
            DanmakuFormat::Json => "json",
        };
        let danmaku_qualifier = services::i18n::additional_files_qualifier(catalog(cfg), "DANMAKU")
            .unwrap_or_else(|| "Danmaku".into());
        let name = writer::write(
            task,
            Contents::Text(&contents),
            suffix,
            Some(&task.File.name.clone()),
            &[&danmaku_qualifier],
        )
        .map_err(|e| e.to_string())?;

        // Embed registration: ass + embed flag + mkv output only.
        if format == DanmakuFormat::Ass
            && extras::options::resolve_bool(task, cfg, "embed_danmaku")
            && writer::is_embed_available(task)
        {
            let mut track = serde_json::Map::new();
            track.insert("file".into(), serde_json::json!(name));
            track.insert("title".into(), serde_json::json!(danmaku_qualifier));
            track.insert("kind".into(), serde_json::json!("danmaku"));
            task.File.subtitle_track_list.push(track);
        }
    }

    // Subtitles and chapter share one player-info request.
    let player_data = if need_subtitle || need_chapter {
        extras::player::get_data(client, "", task)
            .await
            .map_err(|e| e.to_string())?
    } else {
        Value::Null
    };

    if need_subtitle {
        extras::subtitles::parse(client, task, cfg, &labels, &player_data)
            .await
            .map_err(|e| e.to_string())?;
    }

    if need_chapter {
        write_chapter_file(task, &player_data).map_err(|e| e.to_string())?;
    }

    if attr & download_type::COVER != 0 {
        let cover_db = PathBuf::from(&task.File.download_path).join("cover.db");
        extras::cover::parse(client, &cover_db, task, cfg)
            .await
            .map_err(|e| e.to_string())?;
    }

    if attr & download_type::METADATA != 0 {
        extras::nfo::parse(client, "", task, cfg, &labels)
            .await
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}
