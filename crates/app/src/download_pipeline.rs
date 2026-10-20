//! Download orchestrator (ticket 31, Finding F8): the end-to-end pipeline
//! behind a QUEUED task row — port of `downloader.py`'s start →
//! parse_worker → download → additional worker → merger chain, collapsed
//! into one async function per task.
//!
//! Stages, in Python's order:
//! 1. **Parse** (`parse_worker.py`): fetch the playurl payload by content
//!    type (`parsers::playurl::fetch_media_info`), resolve quality/codec by
//!    priority with fallback + supplement, resolve download URLs and sizes
//!    (`network::probe::resolve_download_url`, min 1024), build the
//!    `FileSpec` list filtered by any resumed `Download.queue`.
//! 2. **Download** (`downloader.py start_download`): `Scheduler::start_engine`
//!    + `engine.run()` to completion.
//! 3. **Additional** (`additional/worker.py`): danmaku → one player-info
//!    fetch → subtitles → chapter → cover → metadata, gated by
//!    `Download.type_` bits; clears `File.subtitle_track_list` first so a
//!    retried task does not embed duplicate tracks.
//! 4. **Merge** (`merger.py`): FFMPEG_QUEUED → `mux::Merger::run` →
//!    terminal status; `mark_as_completed` on COMPLETED.
//!
//! Pure async, no gpui — headless-testable against wiremock.

use std::path::PathBuf;
use std::sync::Arc;

use config::ConfigStore;
use database::TaskInfo;
use network::HttpClient;
use serde_json::Value;

use naming::naming_rules::RuleStore;
use naming::task_naming::{freeze_naming_rule, update_file_name_info};
use parsers::playurl::{self as playurl_mod, MediaType, PlayurlApis};
use parsers::preview::{Preview, Priorities};
use parsers::tree::Attribute;

use crate::pipeline_extras;

/// Outcome of one task's pipeline run.
#[derive(Debug)]
pub struct PipelineOutcome {
    pub task_id: String,
    pub status: i64,
    /// Short error message on failure; empty otherwise.
    pub message: String,
}

impl PipelineOutcome {
    fn ok(task_id: &str, status: i64) -> Self {
        Self {
            task_id: task_id.to_owned(),
            status,
            message: String::new(),
        }
    }
    fn err(task_id: &str, status: i64, message: impl Into<String>) -> Self {
        Self {
            task_id: task_id.to_owned(),
            status,
            message: message.into(),
        }
    }
}

/// Read the CDN candidate config (`Advanced` group) into a `CdnConfig`.
/// Missing/unset values fall back to the shipped defaults; `area` unset
/// means no host replacement (Python's unset `config.area`).
pub fn cdn_from_config(cfg: &ConfigStore) -> network::cdn::CdnConfig {
    use network::cdn::{Area, CdnConfig, CdnServer, PreferProvider};

    let area = match cfg
        .get("Advanced", "area")
        .and_then(|v| v.as_str().map(str::to_owned))
        .as_deref()
    {
        Some("cn") => Some(Area::Cn),
        Some("ov") => Some(Area::Ov),
        _ => None,
    };
    let prefer = cfg
        .get("Advanced", "prefer_cdn_server_provider_")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let parse_list = |key: &str| -> Option<Vec<CdnServer>> {
        cfg.get("Advanced", key)?.as_array().map(|list| {
            list.iter()
                .filter_map(|entry| {
                    let host = entry.get("host").and_then(Value::as_str)?;
                    let provider = entry.get("provider").and_then(Value::as_str)?;
                    Some(CdnServer::new(host, provider))
                })
                .collect()
        })
    };

    CdnConfig {
        area,
        prefer_provider: PreferProvider(prefer),
        cn_servers: parse_list("cn_cdn_server_list")
            .filter(|l| !l.is_empty())
            .unwrap_or_else(CdnConfig::default_cn_servers),
        ov_servers: parse_list("ov_cdn_server_list")
            .filter(|l| !l.is_empty())
            .unwrap_or_else(CdnConfig::default_ov_servers),
    }
}

/// Quality/codec priority lists from `Download` (`get_*_by_priority`).
pub fn priorities_from_config(cfg: &ConfigStore) -> Priorities {
    let ids = |key: &str| -> Vec<i64> {
        cfg.get("Download", key)
            .and_then(|v| v.as_array().cloned())
            .map(|a| a.iter().filter_map(Value::as_i64).collect())
            .unwrap_or_default()
    };
    let mut p = Priorities::default();
    let v = ids("video_quality_priority");
    if !v.is_empty() {
        p.video_quality = v;
    }
    let a = ids("audio_quality_priority");
    if !a.is_empty() {
        p.audio_quality = a;
    }
    let c = ids("video_codec_priority");
    if !c.is_empty() {
        p.video_codec = c;
    }
    p
}

/// `mux::GlobalOptions` snapshot from `Additional` (task-pinned values were
/// already frozen into `task.Options` at creation; the merger reads those
/// through its own resolution — this is the global fallback layer).
pub fn global_options_from_config(cfg: &ConfigStore) -> mux::GlobalOptions {
    let b = |key: &str| -> bool {
        cfg.get("Additional", key)
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    };
    mux::GlobalOptions {
        m4a_to_mp3: b("m4a_to_mp3"),
        attach_cover: b("attach_cover"),
        delete_cover_after_attach: b("delete_cover_after_attach"),
        delete_danmaku_after_embed: b("delete_danmaku_after_embed"),
        delete_subtitle_after_embed: b("delete_subtitle_after_embed"),
        keep_original_files_type: cfg
            .get("Additional", "keep_original_files_type")
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
        cover_type: cfg
            .get("Additional", "cover_type")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "jpg".into()),
    }
}

/// `ffmpeg` selection from config (`FFmpeg/` group keys, per `init_ffmpeg`).
pub fn ffmpeg_selection_from_config(
    cfg: &ConfigStore,
    install_root: PathBuf,
) -> ffmpeg::locate::FFmpegSelection {
    let source = ffmpeg::locate::FFmpegSource::from_config(
        &cfg.get("Advanced", "ffmpeg_source")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "bundled".into()),
    )
    .unwrap_or(ffmpeg::locate::FFmpegSource::Bundled);
    ffmpeg::locate::FFmpegSelection {
        source,
        custom_path: cfg
            .get("Advanced", "custom_ffmpeg_path")
            .and_then(|v| v.as_str().map(str::to_owned))
            .map(PathBuf::from)
            .unwrap_or_default(),
        install_root,
    }
}

/// One resolved download entry (Python `download_list` value).
struct FileEntry {
    file_key: String,
    file_name: String,
    url: String,
    file_size: u64,
}

/// Stage 1: parse the download info for `task` (`parse_worker.py`).
/// Returns the file entries (queue-filtered) and mutates the task
/// (quality/codec ids actually used, media type, file exts, merge flags,
/// relative files, parts count).
async fn parse_download(
    client: &HttpClient,
    apis: &PlayurlApis,
    cdn: &network::cdn::CdnConfig,
    priorities: &Priorities,
    cfg: &ConfigStore,
    task: &mut TaskInfo,
) -> Result<Vec<FileEntry>, String> {
    use tasks::status::download_type;

    let attr = Attribute(task.Episode.attribute as u32);
    let item = tree_item_from_task(task);
    let fetched = playurl_mod::fetch_media_info(client, apis, &item, Some(task_qn(task, attr)))
        .await
        .map_err(|e| e.to_string())?;

    let media_type = fetched.media_type;
    task.Download.media_type = media_type as i64;

    // Resolve video + audio selections through the preview machinery
    // (priority, fallback, supplement, size probing).
    let mut preview = Preview::new(priorities.clone());
    preview.adopt(fetched).map_err(|e| e.to_string())?;

    let mut entries: Vec<FileEntry> = Vec::new();
    let wants_video = task.Download.type_ & download_type::VIDEO != 0;
    let wants_audio = task.Download.type_ & download_type::AUDIO != 0;

    if wants_video {
        let video = preview
            .query_video(
                client,
                apis,
                task.Download.video_quality_id,
                task.Download.video_codec_id,
            )
            .await
            .map_err(|e| e.to_string())?;
        if let Some(info) = video {
            task.Download.video_quality_id = info.quality_id;
            task.Download.video_codec_id = info.codec_id;
            task.File.video_file_ext = playurl_mod::stream_file_ext(media_type).to_owned();
            match media_type {
                MediaType::Dash => {
                    let name = playurl_mod::video_file_name(&task.Basic.task_id, media_type);
                    push_relative(task, &name);
                    let node = preview.video_node(info.quality_id, info.codec_id);
                    let (url, file_size) = resolve_entry(client, &node, cdn).await?;
                    entries.push(FileEntry {
                        file_key: "video".into(),
                        file_name: name,
                        url,
                        file_size,
                    });
                }
                MediaType::Mp4 | MediaType::Flv => {
                    // durl parts: index-ordered, one file per part.
                    let parts = preview.durl_parts(info.quality_id);
                    task.Download.video_parts_count = parts.len() as i64;
                    for (index, part) in parts.iter().enumerate() {
                        let name =
                            playurl_mod::video_part_name(&task.Basic.task_id, index, media_type);
                        push_relative(task, &name);
                        let (url, file_size) = resolve_entry(client, part, cdn).await?;
                        entries.push(FileEntry {
                            file_key: format!("video_part_{index}"),
                            file_name: name,
                            url,
                            file_size,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    if wants_audio {
        if let Some(info) = preview.query_audio(task.Download.audio_quality_id) {
            task.Download.audio_quality_id = info.quality_id;
            task.File.audio_file_ext = playurl_mod::audio_file_ext(info.quality_id).to_owned();
            let name = playurl_mod::audio_file_name(&task.Basic.task_id, info.quality_id);
            push_relative(task, &name);
            let node = preview.audio_node(info.quality_id);
            let (url, file_size) = resolve_entry(client, &node, cdn).await?;
            entries.push(FileEntry {
                file_key: "audio".into(),
                file_name: name,
                url,
                file_size,
            });
        }
    }

    // `get_output_file_ext`: without both streams there is nothing to
    // merge; with merging on (or video parts), the output extension is
    // the resolved container.
    let got_video = entries.iter().any(|e| e.file_key.starts_with("video"));
    let got_audio = entries.iter().any(|e| e.file_key == "audio");
    if !got_video || !got_audio {
        task.Download.merge_video_audio = false;
        task.Download.keep_original_files = false;
    }
    if task.Download.merge_video_audio || task.Download.video_parts_count > 0 {
        let container = tasks::options::resolve(cfg, task, "video_container");
        task.File.merge_file_ext = container.as_str().unwrap_or("mp4").to_string();
    }

    // `_update_media_info`: re-render the frozen rule now that quality /
    // codec variables are known — also re-names tasks resumed with an
    // empty file name (rows created before the naming engine existed).
    if task.File.name.is_empty() {
        let store =
            RuleStore::from_config_value(cfg.get("File Naming", "naming_rule_list").as_ref());
        if task.Naming.rule.as_deref().unwrap_or("").is_empty() {
            freeze_naming_rule(task, None, &store, &Default::default());
        }
        let _ = update_file_name_info(task, &store);
    }

    // `filter_download_list`: on resume, only the queue's keys download.
    if !task.Download.queue.is_empty() {
        entries.retain(|e| task.Download.queue.iter().any(|q| q == &e.file_key));
    }

    task.Download.total_size = entries.iter().map(|e| e.file_size).sum::<u64>() as i64;
    Ok(entries)
}

fn push_relative(task: &mut TaskInfo, name: &str) {
    if !task.File.relative_files.iter().any(|f| f == name) {
        task.File.relative_files.push(name.to_owned());
    }
}

fn task_qn(task: &TaskInfo, attr: Attribute) -> i64 {
    // Python requests 127 when auto so the account's real max surfaces.
    if attr.contains(Attribute::VIDEO_BIT)
        && task.Download.video_quality_id == playurl_mod::QUALITY_AUTO
    {
        127
    } else {
        task.Download.video_quality_id
    }
}

/// Resolve one stream node to `(url, size)` (`QueryWorker`).
async fn resolve_entry(
    client: &HttpClient,
    node: &Value,
    cdn: &network::cdn::CdnConfig,
) -> Result<(String, u64), String> {
    let urls = playurl_mod::extract_stream_urls(node);
    network::probe::resolve_download_url(client, &urls, cdn, 1024)
        .await
        .map(|r| (r.url, r.file_size))
        .map_err(|e| e.to_string())
}

/// The whole pipeline for one task. `manager` persists status transitions;
/// the caller spawns this per admitted task.
pub async fn run_task(
    manager: &Arc<tasks::TaskManager>,
    cfg: &Arc<ConfigStore>,
    client: &HttpClient,
    apis: &PlayurlApis,
    install_root: PathBuf,
    task: TaskInfo,
) -> PipelineOutcome {
    use tasks::status::download_status;

    let task_id = task.Basic.task_id.clone();
    let mut task = task;
    let cdn = cdn_from_config(cfg);
    let priorities = priorities_from_config(cfg);

    // ---- Stage 1: parse ----
    task.Download.status = download_status::PARSING;
    manager.update_async(&task);
    let entries = match parse_download(client, apis, &cdn, &priorities, cfg, &mut task).await {
        Ok(e) => e,
        Err(message) => {
            task.Download.status = download_status::FAILED;
            manager.update_async(&task);
            return PipelineOutcome::err(&task_id, download_status::FAILED, message);
        }
    };
    if entries.is_empty() {
        // Nothing left (audio-only with no stream / fully resumed): jump to
        // the merge stage so extras still run (Python `on_download_completed`
        // when the queue is empty is skipped — an empty parse is an error
        // upstream; here we treat it as complete-with-nothing).
        task.Download.status = download_status::DOWNLOADING;
    }

    // ---- Stage 2: download ----
    if !entries.is_empty() {
        task.Download.status = download_status::DOWNLOADING;
        task.Download.queue = entries.iter().map(|e| e.file_key.clone()).collect();
        // `update_info`: seed per-file progress on first parse.
        if task.Download.files.is_empty() {
            let mut files = serde_json::Map::new();
            for e in &entries {
                files.insert(
                    e.file_key.clone(),
                    serde_json::json!({
                        "chunks_list": [],
                        "total_chunks": 0,
                        "finished_chunks": 0,
                        "chunk_offsets": {},
                        "file_size": e.file_size,
                    }),
                );
            }
            task.Download.files = files;
        }
        manager.update_async(&task);

        let specs: Vec<downloader::FileSpec> = entries
            .iter()
            .map(|e| downloader::FileSpec {
                file_key: e.file_key.clone(),
                url: e.url.clone(),
                file_size: e.file_size,
                path: PathBuf::from(&task.File.download_path)
                    .join(&task.File.folder)
                    .join(&e.file_name),
            })
            .collect();
        let dir = PathBuf::from(&task.File.download_path).join(&task.File.folder);
        let _ = std::fs::create_dir_all(&dir);

        // Resume seeding: fold the freshest persisted offset tables into
        // the new engine so a resumed run skips confirmed chunks instead
        // of re-downloading from zero (files absent/empty → fresh start).
        let resume: std::collections::BTreeMap<String, downloader::OffsetTable> = {
            let files = manager.current_files(&task_id);
            files
                .iter()
                .filter_map(|(key, file)| {
                    let table =
                        downloader::OffsetTable::from_json(file.get("chunk_offsets")?.clone());
                    if table == downloader::OffsetTable::default() {
                        None
                    } else {
                        Some((key.clone(), table))
                    }
                })
                .collect()
        };
        let Ok((handle, control)) = manager.scheduler.spawn_engine_resumed(
            manager,
            &task_id,
            client.clone(),
            specs,
            if resume.is_empty() {
                None
            } else {
                Some(resume)
            },
        ) else {
            task.Download.status = download_status::FAILED;
            manager.update_async(&task);
            return PipelineOutcome::err(&task_id, download_status::FAILED, "engine start failed");
        };
        // Live progress: sample the engine while it runs and persist
        // progress/speed/downloaded_size (the engine itself never
        // computes speed — it's a display concern of this sampler).
        let sampler = tokio::spawn({
            let manager = manager.clone();
            let task = task.clone();
            let control = control.clone();
            async move {
                let mut last_bytes = control.snapshot().downloaded_size;
                let mut smooth: f64 = 0.0;
                let mut tick = tokio::time::interval(std::time::Duration::from_millis(500));
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tick.tick().await;
                    let snap = control.snapshot();
                    if snap.downloaded_size == last_bytes {
                        continue;
                    }
                    let inst = snap.downloaded_size.saturating_sub(last_bytes) as f64 * 2.0; // 0.5s window
                    last_bytes = snap.downloaded_size;
                    // EMA (Python 每秒采样直接显示; 0.5s 窗口的瞬时差分
                    // 噪声大, 平滑掉显示层的数字跳动, 不影响任何决策路径)
                    smooth = if smooth == 0.0 {
                        inst
                    } else {
                        smooth * 0.6 + inst * 0.4
                    };
                    let mut t = task.clone();
                    t.Download.downloaded_size = snap.downloaded_size as i64;
                    t.Download.progress = snap.progress() as i64;
                    t.Download.speed = smooth as i64;
                    manager.update_async(&t);
                }
            }
        });
        let state = match handle.await {
            Ok(state) => state,
            Err(e) => {
                sampler.abort();
                manager.scheduler.release(&task_id);
                task.Download.status = download_status::FAILED;
                manager.update_async(&task);
                return PipelineOutcome::err(&task_id, download_status::FAILED, e.to_string());
            }
        };
        sampler.abort();
        // Fold the engine's final offset tables into the task row: every
        // subsequent update/mark_as_completed persists them, so a resumed
        // run seeds from bytes already confirmed on disk.
        for (key, table) in control.offset_tables() {
            if let Some(file) = task.Download.files.get_mut(&key) {
                if let Some(map) = file.as_object_mut() {
                    map.insert("chunk_offsets".into(), table.to_json());
                }
            }
        }
        match state.status {
            downloader::TaskStatus::Completed => {}
            downloader::TaskStatus::Paused => {
                manager.scheduler.release(&task_id);
                task.Download.status = download_status::PAUSED;
                manager.update_async(&task);
                return PipelineOutcome::ok(&task_id, download_status::PAUSED);
            }
            _status => {
                manager.scheduler.release(&task_id);
                task.Download.status = download_status::FAILED;
                manager.update_async(&task);
                return PipelineOutcome::err(&task_id, download_status::FAILED, "download failed");
            }
        }
        task.Download.downloaded_size = state.downloaded_size as i64;
        task.Download.progress = 100;
        task.Download.speed = 0;
        manager.scheduler.release(&task_id);
    }

    // ---- Stage 3: additional files ----
    if let Err(message) = pipeline_extras::run_additional(client, cfg, &mut task).await {
        task.Download.status = download_status::FAILED;
        manager.update_async(&task);
        return PipelineOutcome::err(&task_id, download_status::FAILED, message);
    }

    // ---- Stage 4: merge ----
    task.Download.status = download_status::FFMPEG_QUEUED;
    manager.update_async(&task);
    let globals = global_options_from_config(cfg);
    let exe = match ffmpeg_selection_from_config(cfg, install_root).resolve() {
        Ok(r) => r.path,
        Err(e) => {
            task.Download.status = download_status::FFMPEG_FAILED;
            manager.update_async(&task);
            return PipelineOutcome::err(&task_id, download_status::FFMPEG_FAILED, e.to_string());
        }
    };
    task.Download.status = download_status::MERGING;
    manager.update_async(&task);
    let outcome = {
        let manager = manager.clone();
        let mut last_persisted = -1i64;
        // Live merge progress: persist every 2% step (the callback fires
        // per percent; a write per step is enough for a moving bar). The
        // sink runs while `task` is mutably borrowed, so it carries its
        // own snapshot and only swaps the progress field.
        let base = {
            let mut t = task.clone();
            t.Download.speed = 0;
            t
        };
        let mut merger = mux::Merger::new(&mut task, &globals, exe);
        merger.set_progress_sink(move |p| {
            if p / 2 == last_persisted / 2 {
                return;
            }
            last_persisted = p;
            let mut t = base.clone();
            t.Download.progress = p;
            manager.update_async(&t);
        });
        merger.run()
    };
    manager.update_async(&task);
    if outcome.status == download_status::COMPLETED {
        manager.mark_as_completed(&task, false, 5.0);
    }
    PipelineOutcome::ok(&task_id, outcome.status)
}

/// `TreeItemOf` constructor from a task row: only the identity fields
/// `fetch_media_info` reads.
fn tree_item_from_task(task: &TaskInfo) -> parsers::tree::TreeItem {
    use parsers::tree::{CheckState, Number};
    parsers::tree::TreeItem {
        attribute: Attribute(task.Episode.attribute as u32),
        checked: CheckState::Unchecked,
        pubtime: 0,
        favtime: 0,
        viewtime: 0,
        expired: false,
        aid: task.Episode.aid,
        cid: task.Episode.cid,
        sid: task.Episode.sid,
        url: task.Episode.url.clone(),
        bvid: task.Episode.bvid.clone(),
        ep_id: task.Episode.ep_id,
        badge: String::new(),
        cover: task.Episode.cover.clone(),
        title: task.Basic.show_title.clone(),
        author: String::new(),
        number: Number::default(),
        duration: task.Episode.duration,
        episode_id: String::new(),
        episode_plot: String::new(),
        part_number: 0,
        episode_number: 0,
        related_titles: Default::default(),
        uploader: String::new(),
        uploader_uid: 0,
        course_id: task.Episode.course_id,
        lesson_id: task.Episode.lesson_id,
        item_id: task.Episode.item_id,
        section_id: task.Episode.section_id,
        children: Vec::new(),
    }
}

#[cfg(all(test, unix))]
mod tests {
    //! End-to-end pipeline test (ticket 31, F8): parse → download → merge
    //! over wiremock at the HTTP seams (playurl API + CDN HEAD/ranged-GET),
    //! with a fake producing-ffmpeg script like mux's own fixtures.

    use super::*;
    use serde_json::json;
    use std::sync::Arc;
    use tasks::status::{download_status, download_type};
    use tasks::TaskManager;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn wire_client() -> HttpClient {
        network::HttpClient::new(network::ClientOptions {
            user_agent: "bilidown-test".into(),
            proxy: network::ProxyConfig::none(),
            cookies_from_persistence: Vec::new(),
            signer: None,
        })
        .unwrap()
    }

    /// 4 MiB of patterned bytes — comfortably above the 1024-byte probe
    /// floor.
    fn body(seed: u64, len: u64) -> Vec<u8> {
        let mut state = seed;
        let mut out = Vec::with_capacity(len as usize);
        while out.len() < len as usize {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            out.push((state & 0xff) as u8);
        }
        out
    }

    fn producing_ffmpeg(dir: &std::path::Path) -> PathBuf {
        let path = dir.join("ffmpeg");
        std::fs::write(
            &path,
            "#!/bin/sh\nfor arg do\n  out=$arg\ndone\nprintf 'x' > \"$out\"\nprintf 'out_time_us=100000000\\nprogress=end\\n'\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// A DASH playurl payload: one video node (id 80, codec 7) + one
    /// audio node (id 30280), URLs pointing at the wiremock CDN.
    fn dash_payload(base: &str) -> Value {
        json!({
            "code": 0,
            "message": "0",
            "data": {
                "format": "dash",
                "quality": 80,
                "timelength": 100000,
                "accept_quality": [80],
                "dash": {
                    "video": [{
                        "id": 80,
                        "codecid": 7,
                        "baseUrl": format!("{base}/video.m4s"),
                        "backupUrl": [],
                    }],
                    "audio": [{
                        "id": 30280,
                        "codecid": 0,
                        "baseUrl": format!("{base}/audio.m4s"),
                        "backupUrl": [],
                    }]
                }
            }
        })
    }

    /// CDN mock: HEAD returns the size; ranged GET returns the slice.
    async fn cdn_mock(server: &MockServer, url_path: &str, data: Arc<Vec<u8>>) {
        Mock::given(method("HEAD"))
            .and(path(url_path))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "video/mp4")
                    .insert_header("content-length", data.len().to_string()),
            )
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path(url_path))
            .respond_with({
                let data = data.clone();
                move |req: &wiremock::Request| {
                    let range = req
                        .headers
                        .get("range")
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_owned());
                    match range {
                        Some(range) if range.starts_with("bytes=") => {
                            let (s, e) = range[6..].split_once('-').unwrap();
                            let (start, end): (usize, usize) =
                                (s.parse().unwrap(), e.parse().unwrap());
                            ResponseTemplate::new(206)
                                .append_header(
                                    "Content-Range",
                                    format!("bytes {}-{}/{}", start, end, data.len()),
                                )
                                .set_body_bytes(data[start..=end].to_vec())
                        }
                        _ => ResponseTemplate::new(200).set_body_bytes(data.as_ref().clone()),
                    }
                }
            })
            .mount(server)
            .await;
    }

    fn dash_task(dir: &std::path::Path, cid: i64) -> TaskInfo {
        let mut task = TaskInfo::default();
        task.Basic.task_id = "e2e1".to_string();
        task.Basic.show_title = "End to End".to_string();
        task.File.download_path = dir.to_string_lossy().into_owned();
        task.File.folder = String::new();
        task.File.name = "End to End".to_string();
        task.Download.type_ = download_type::VIDEO | download_type::AUDIO;
        task.Download.status = download_status::QUEUED;
        task.Download.video_quality_id = 200; // auto
        task.Download.audio_quality_id = 30300; // auto
        task.Download.video_codec_id = 20; // auto
        task.Download.merge_video_audio = true;
        task.Episode.attribute = 1; // Attribute::VIDEO_BIT
        task.Episode.bvid = "BV1xx411c7mD".to_string();
        task.Episode.cid = cid;
        task.Episode.aid = 42;
        task.Episode.duration = 100;
        task
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dash_task_downloads_merges_and_completes_end_to_end() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store =
            ConfigStore::with_paths(tmp.path().join("config"), tmp.path().join("downloads"));
        let ffmpeg = producing_ffmpeg(tmp.path());
        store.set("Advanced", "ffmpeg_source", json!("custom"));
        store.set(
            "Advanced",
            "custom_ffmpeg_path",
            json!(ffmpeg.to_string_lossy().to_string()),
        );
        let server = MockServer::start().await;
        // Point the CDN replacement at the wiremock host so every
        // rewritten candidate URL still routes to the mock.
        let netloc = server.uri().trim_start_matches("http://").to_owned();
        store.set(
            "Advanced",
            "cn_cdn_server_list",
            json!([{ "host": netloc, "provider": "TEST" }]),
        );
        store.save().unwrap();
        let store = Arc::new(store);
        let manager =
            Arc::new(TaskManager::open(tmp.path().join("task.db"), store.clone()).unwrap());

        let apis = PlayurlApis::with_base(&server.uri());

        Mock::given(method("GET"))
            .and(path("/x/player/wbi/playurl"))
            .respond_with(ResponseTemplate::new(200).set_body_json(dash_payload(&server.uri())))
            .mount(&server)
            .await;

        let video = Arc::new(body(0x5eed, 4 * 1024 * 1024));
        let audio = Arc::new(body(0xa11ce, 2 * 1024 * 1024));
        cdn_mock(&server, "/video.m4s", video.clone()).await;
        cdn_mock(&server, "/audio.m4s", audio.clone()).await;

        let mut task = dash_task(tmp.path(), 1234);
        task.File.merge_file_ext = "mp4".to_string();

        // Seed the task row so the manager can update it.
        {
            let mut db = manager.reader().unwrap();
            db.add_tasks(std::slice::from_ref(&task), false).unwrap();
        }

        let client = wire_client();
        let outcome = run_task(
            &manager,
            &store,
            &client,
            &apis,
            tmp.path().to_path_buf(),
            task.clone(),
        )
        .await;

        assert_eq!(
            outcome.status,
            download_status::COMPLETED,
            "{}",
            outcome.message
        );
        assert_eq!(outcome.task_id, "e2e1");

        // The merged output exists (fake ffmpeg wrote it, merger renamed it).
        let delivered = tmp.path().join("End to End.mp4");
        assert!(delivered.exists(), "merged output missing");

        // The task row reached COMPLETED in the DB.
        manager.shutdown(5.0);
        let db = database::TaskDatabase::open(tmp.path().join("task.db")).unwrap();
        let row = db
            .query_task_by_id("e2e1", true)
            .unwrap()
            .expect("task moved to completed");
        assert_eq!(row.Download.status, download_status::COMPLETED);
    }

    /// Regression (downloads live progress + resume): a paused task's
    /// persisted offset tables must seed the resumed engine, so the
    /// resumed run never re-requests byte ranges already on disk. The
    /// CDN mock counts ranged requests; a from-zero restart would ask
    /// for `bytes 0-…` again and the count would double.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn resumed_run_skips_already_downloaded_ranges() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store =
            ConfigStore::with_paths(tmp.path().join("config"), tmp.path().join("downloads"));
        let ffmpeg = producing_ffmpeg(tmp.path());
        store.set("Advanced", "ffmpeg_source", json!("custom"));
        store.set(
            "Advanced",
            "custom_ffmpeg_path",
            json!(ffmpeg.to_string_lossy().to_string()),
        );
        let server = MockServer::start().await;
        let netloc = server.uri().trim_start_matches("http://").to_owned();
        store.set(
            "Advanced",
            "cn_cdn_server_list",
            json!([{ "host": netloc, "provider": "TEST" }]),
        );
        store.save().unwrap();
        let store = Arc::new(store);
        let manager =
            Arc::new(TaskManager::open(tmp.path().join("task.db"), store.clone()).unwrap());
        let apis = PlayurlApis::with_base(&server.uri());

        Mock::given(method("GET"))
            .and(path("/x/player/wbi/playurl"))
            .respond_with(ResponseTemplate::new(200).set_body_json(dash_payload(&server.uri())))
            .mount(&server)
            .await;

        // One 4 MiB video file only (audio keeps the range math simple).
        let video = Arc::new(body(0x5eed, 4 * 1024 * 1024));
        let ranges = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        {
            let ranges = ranges.clone();
            Mock::given(method("HEAD"))
                .and(path("/video.m4s"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .insert_header("content-type", "video/mp4")
                        .insert_header("content-length", video.len().to_string()),
                )
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/video.m4s"))
                .respond_with(move |req: &wiremock::Request| {
                    let is_range = req
                        .headers
                        .get("range")
                        .and_then(|v| v.to_str().ok())
                        .is_some_and(|r| r.starts_with("bytes="));
                    if is_range {
                        ranges.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    }
                    let data = video.clone();
                    match req.headers.get("range").and_then(|v| v.to_str().ok()) {
                        Some(range) if range.starts_with("bytes=") => {
                            let (s, e) = range[6..].split_once('-').unwrap();
                            let (start, end): (usize, usize) =
                                (s.parse().unwrap(), e.parse().unwrap());
                            ResponseTemplate::new(206)
                                .append_header(
                                    "Content-Range",
                                    format!("bytes {}-{}/{}", start, end, data.len()),
                                )
                                .set_body_bytes(data[start..=end].to_vec())
                        }
                        _ => ResponseTemplate::new(200).set_body_bytes(data.as_ref().clone()),
                    }
                })
                .mount(&server)
                .await;
        }

        // Run 1: video-only task completes fully.
        let mut task = dash_task(tmp.path(), 5678);
        task.Download.type_ = download_type::VIDEO;
        task.Download.merge_video_audio = false;
        task.File.merge_file_ext = "mp4".into();
        {
            let mut db = manager.reader().unwrap();
            db.add_tasks(std::slice::from_ref(&task), false).unwrap();
        }
        let client = wire_client();
        let outcome = run_task(
            &manager,
            &store,
            &client,
            &apis,
            tmp.path().to_path_buf(),
            task.clone(),
        )
        .await;
        assert_eq!(
            outcome.status,
            download_status::COMPLETED,
            "{}",
            outcome.message
        );
        manager.shutdown(5.0);
        let ranges_after_first = ranges.load(std::sync::atomic::Ordering::SeqCst);
        assert!(ranges_after_first > 0, "first run must use ranged requests");

        // A second run over the same task (files map already carries the
        // completed chunk_offsets — the resume path's input) must not
        // re-request any range: every chunk is confirmed on disk.
        let ranges_before_second = ranges_after_first;
        let manager2 = TaskManager::open(tmp.path().join("task.db"), store.clone()).unwrap();
        let manager2 = Arc::new(manager2);
        let mut resumed = manager2
            .query_by_id("e2e1")
            .unwrap()
            .expect("row persisted");
        resumed.Download.status = download_status::QUEUED;
        resumed.Download.progress = 50;
        {
            let mut db = manager2.reader().unwrap();
            db.add_tasks(std::slice::from_ref(&resumed), false).unwrap();
        }
        let outcome2 = run_task(
            &manager2,
            &store,
            &client,
            &apis,
            tmp.path().to_path_buf(),
            resumed,
        )
        .await;
        assert_eq!(
            outcome2.status,
            download_status::COMPLETED,
            "{}",
            outcome2.message
        );
        manager2.shutdown(5.0);
        let ranges_after_second = ranges.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            ranges_after_second, ranges_before_second,
            "resumed run re-downloaded confirmed chunks"
        );
    }
}
