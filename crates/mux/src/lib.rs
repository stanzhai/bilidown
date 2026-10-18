//! Post-download FFmpeg pipeline (ticket 15). Port of
//! `src/util/download/downloader/merger.py`: DASH video+audio merge, legacy
//! FLV multi-part concat, m4a→mp3 conversion, standalone audio remux, cover
//! `attached_pic` embedding, ffmetadata chapter injection, and the temp→final
//! rename dance — with FFmpeg progress and watchdog outcomes flowing into
//! the task state.
//!
//! The Python `Merger` is a Qt signal machine; this port is a synchronous
//! state machine over [`Merger::run`]. The task manager (ticket 16) drives
//! it after the chunked engine finishes, passing the resolved ffmpeg
//! executable plus a global-options snapshot, then persists the mutated
//! [`TaskInfo`] (the Python `add_file`/`task_manager.update` calls collapse
//! into that single save).
//!
//! Option resolution mirrors `resolve()` in `util/download/task/options.py`:
//! a task-pinned option wins; a missing or unrecognized one falls back to
//! the caller's snapshot of the global settings. This crate deliberately
//! does not depend on `config`, keeping the ffmpeg-crate precedent.

use std::path::{Path, PathBuf};

use database::TaskInfo;
use ffmpeg::command::{FFmpegCommand, SubtitleEntry};
use ffmpeg::runner::{FFmpegOutcome, FFmpegRunner, WatchdogConfig};
use parking_lot::Mutex;

/// Standalone DASH audio stream extensions (Python `DASH_AUDIO_STREAM_EXTS`):
/// all three ship fragmented-MP4 payloads that strict players reject until
/// remuxed into the container their extension promises.
pub const DASH_AUDIO_STREAM_EXTS: [&str; 3] = ["m4a", "flac", "ec3"];

/// Task download statuses this pipeline transitions between (Python
/// `DownloadStatus` codes persisted in task.db; the download engine owns
/// 0-3).
pub mod download_status {
    pub const COMPLETED: i64 = 4;
    pub const FFMPEG_QUEUED: i64 = 5;
    pub const MERGING: i64 = 6;
    pub const CONVERTING: i64 = 7;
    pub const FFMPEG_FAILED: i64 = 101;
}

/// `DownloadType` bits deciding which files get delivered.
pub mod download_type {
    pub const VIDEO: i64 = 1 << 0;
    pub const AUDIO: i64 = 1 << 1;
}

/// Error messages surfaced on the failure path, ported verbatim from the
/// Python translator's ERROR_MESSAGES entries (the zh/en translation layer
/// is a later UI ticket; keys stay stable).
pub mod error_messages {
    pub const DOWNLOAD_FAILED: &str = "Download failed";
    pub const RENAME_FAILED: &str = "Failed to rename file";
    pub const FFMPEG_STILL_RUNNING: &str =
        "The previous FFmpeg task has not exited yet. Please try again later.";
    pub const FILE_NOT_FOUND_DETAIL: &str =
        "The file may have been moved or deleted. Please download it again.";
    pub const M4A_NOT_FOUND: &str = "M4A audio file not found for conversion";
    pub const INSUFFICIENT_SPACE: &str =
        "Insufficient disk space, please free up enough space and try again.";
    pub const PERMISSION_DENIED: &str = "Permission denied: cannot write to file";
    pub const CORRUPTED_FILE: &str = "Downloaded file is corrupted, please try downloading again.";
    pub const COULD_NOT_OPEN: &str = "Failed to open file";
    pub const FILE_IS_BUSY: &str = "File is in use by another process";
    pub const CANNOT_CREATE: &str = "Could not create output file";
}

/// stderr substring → message, first match wins (Python `on_merge_error`).
const ERROR_MAP: [(&str, &str); 7] = [
    (
        "No space left on device",
        error_messages::INSUFFICIENT_SPACE,
    ),
    ("Permission denied", error_messages::PERMISSION_DENIED),
    (
        "Invalid data found when processing input",
        error_messages::CORRUPTED_FILE,
    ),
    (
        "No such file or directory",
        error_messages::FILE_NOT_FOUND_DETAIL,
    ),
    ("Could not open file", error_messages::COULD_NOT_OPEN),
    ("Device or resource busy", error_messages::FILE_IS_BUSY),
    (
        "Could not create output file",
        error_messages::CANNOT_CREATE,
    ),
];

/// Chapter sidecar name; the chapter parser (ticket 18) writes it, the
/// merger finds it by this name and deletes it after a successful merge.
pub fn chapter_file_name(task_id: &str) -> String {
    format!("chapter_{task_id}.txt")
}

/// How a rename target conflict resolves (Python `FileConflictResolution`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileConflictResolution {
    /// Keep both: the incoming file becomes `name (n).ext`.
    #[default]
    AutoRename,
    /// Replace the existing target.
    Overwrite,
}

impl FileConflictResolution {
    pub fn from_code(code: i64) -> Self {
        match code {
            2 => Self::Overwrite,
            _ => Self::AutoRename,
        }
    }
}

/// `OriginalFileType`: which stream is delivered alongside the merge when
/// originals are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OriginalFileType {
    #[default]
    Both = 0,
    Video = 1,
    Audio = 2,
}

impl OriginalFileType {
    /// Unknown/renamed members degrade to Both, matching the Python
    /// `except ValueError` fallback.
    pub fn from_code(code: i64) -> Self {
        match code {
            1 => Self::Video,
            2 => Self::Audio,
            _ => Self::Both,
        }
    }
}

/// Global fallbacks for the per-task download options, snapshotted by the
/// caller from the config store (`snapshot()` in `task/options.py`).
#[derive(Debug, Clone)]
pub struct GlobalOptions {
    pub m4a_to_mp3: bool,
    pub attach_cover: bool,
    pub delete_cover_after_attach: bool,
    pub delete_danmaku_after_embed: bool,
    pub delete_subtitle_after_embed: bool,
    pub keep_original_files_type: i64,
    /// `Additional.cover_type` (jpg/png/avif/webp).
    pub cover_type: String,
}

impl Default for GlobalOptions {
    /// Mirrors `defaults.json`: every toggle off, cover type jpg.
    fn default() -> Self {
        Self {
            m4a_to_mp3: false,
            attach_cover: false,
            delete_cover_after_attach: false,
            delete_danmaku_after_embed: false,
            delete_subtitle_after_embed: false,
            keep_original_files_type: 0,
            cover_type: "jpg".to_string(),
        }
    }
}

/// Serializes conflict resolution + rename across tasks finishing into the
/// same folder (Python `_rename_lock`).
static RENAME_LOCK: Mutex<()> = Mutex::new(());

/// Delete files under `cwd`; a missing file is fine, anything else fails
/// with the error text (Python `safe_remove` re-raises).
fn safe_remove(cwd: &Path, file_names: &[String]) -> Result<(), String> {
    for name in file_names {
        let path = cwd.join(name);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{path:?}: {e}")),
        }
    }
    Ok(())
}

/// Rename under `cwd`, resolving a target conflict per `conflict`; returns
/// the delivered file name (Python `safe_rename`).
fn safe_rename(
    cwd: &Path,
    original: &str,
    new: &str,
    conflict: FileConflictResolution,
) -> Result<String, String> {
    let original_path = cwd.join(original);
    let new_path = cwd.join(new);

    if !original_path.exists() {
        return Err(format!("原始文件不存在: {}", original_path.display()));
    }

    let delivered = {
        let _guard = RENAME_LOCK.lock();
        let target = if !new_path.exists() {
            new_path.clone()
        } else {
            match conflict {
                FileConflictResolution::Overwrite => {
                    std::fs::remove_file(&new_path).map_err(|e| format!("{:?}: {e}", new_path))?;
                    new_path.clone()
                }
                FileConflictResolution::AutoRename => {
                    let stem = new_path.file_stem().and_then(|s| s.to_str()).unwrap_or(new);
                    let ext = match new_path.extension().and_then(|s| s.to_str()) {
                        Some(e) => format!(".{e}"),
                        None => String::new(),
                    };
                    let mut n = 1;
                    loop {
                        let candidate = cwd.join(format!("{stem} ({n}){ext}"));
                        if !candidate.exists() {
                            break candidate;
                        }
                        n += 1;
                    }
                }
            }
        };
        std::fs::rename(&original_path, &target)
            .map_err(|e| format!("{:?} -> {:?}: {e}", original_path, target))?;
        target
    };
    Ok(delivered
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| new.to_string()))
}

/// Terminal result of one pipeline run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeOutcome {
    /// `Download.status` code: COMPLETED or FFMPEG_FAILED.
    pub status: i64,
    /// Short toast message; empty on success.
    pub short_message: String,
    /// Long description; FFmpeg failures append the stderr tail.
    pub description: String,
}

impl MergeOutcome {
    fn failed(short: &str, description: impl Into<String>) -> Self {
        Self {
            status: download_status::FFMPEG_FAILED,
            short_message: short.to_string(),
            description: description.into(),
        }
    }
}

/// The one-FFmpeg-at-a-time mux pipeline for one downloaded task.
pub struct Merger<'a> {
    task: &'a mut TaskInfo,
    globals: &'a GlobalOptions,
    exe: PathBuf,
    conflict: FileConflictResolution,
    watchdog: Option<WatchdogConfig>,
    /// Cover selected at check time: (file name, delete after embedding).
    embedded_cover: Option<(String, bool)>,
    /// Embedded subtitle/danmaku tracks as (file, kind) for post-merge
    /// deletion.
    embedded_subtitles: Vec<(String, String)>,
    /// Audio artifact to deliver: a remux/convert output, else the raw
    /// stream (Python `output_audio_file_name`).
    output_audio_file: Option<String>,
    /// Ext written back to `File.audio_file_ext` once conversion succeeds.
    converted_audio_ext: Option<&'static str>,
    /// Live progress sink: called on every ffmpeg percentage so the
    /// pipeline can persist the merge bar without waiting for the run
    /// to finish.
    on_progress: Option<Box<dyn FnMut(i64) + 'a>>,
}

impl<'a> Merger<'a> {
    /// The caller has already set `Download.status = MERGING` (Python
    /// `start()` contract) and provides the resolved ffmpeg executable.
    pub fn new(
        task: &'a mut TaskInfo,
        globals: &'a GlobalOptions,
        exe: impl Into<PathBuf>,
    ) -> Self {
        Self {
            task,
            globals,
            exe: exe.into(),
            conflict: FileConflictResolution::default(),
            watchdog: None,
            embedded_cover: None,
            embedded_subtitles: Vec::new(),
            output_audio_file: None,
            converted_audio_ext: None,
            on_progress: None,
        }
    }

    /// Attach a live progress sink (each ffmpeg percentage step).
    pub fn set_progress_sink(&mut self, sink: impl FnMut(i64) + 'a) -> &mut Self {
        self.on_progress = Some(Box::new(sink));
        self
    }

    /// `Behavior.file_conflict_resolution` for temp→final renames.
    pub fn set_conflict_resolution(&mut self, conflict: FileConflictResolution) -> &mut Self {
        self.conflict = conflict;
        self
    }

    /// Overrides the production watchdog (300s/5s); tests run second-scale.
    pub fn set_watchdog(&mut self, watchdog: WatchdogConfig) -> &mut Self {
        self.watchdog = Some(watchdog);
        self
    }

    /// Run the pipeline to a terminal state, mutating the task (status,
    /// progress, audio ext, relative files, completed time) along the way.
    pub fn run(&mut self) -> MergeOutcome {
        match self.start() {
            Ok(()) => MergeOutcome {
                status: self.task.Download.status,
                short_message: String::new(),
                description: String::new(),
            },
            Err(outcome) => {
                self.task.Download.status = outcome.status;
                outcome
            }
        }
    }

    /// `Err` = terminal failure with the outcome already composed.
    fn start(&mut self) -> Result<(), MergeOutcome> {
        if self.task.Download.merge_video_audio {
            if self.should_remux_kept_audio() {
                // Kept audio is both a deliverable and a merge input; remux
                // it first, then merge off the still-present original.
                self.remux_audio()?;
                self.task.Download.status = download_status::MERGING;
            }
            self.merge_video_audio()
        } else if self.task.Download.video_parts_count > 0 {
            self.merge_video_parts()
        } else if DASH_AUDIO_STREAM_EXTS.contains(&self.task.File.audio_file_ext.as_str()) {
            if self.task.File.audio_file_ext == "m4a" && self.resolve_m4a_to_mp3() {
                // Re-encoding rewrites the container; no remux needed.
                self.m4a_to_mp3()
            } else {
                self.remux_audio()?;
                self.on_convert_completed()
            }
        } else {
            // Video-only download (no independent audio stream): nothing to
            // merge, deliver by rename.
            self.rename_output_file()
        }
    }

    fn cwd(&self) -> PathBuf {
        Path::new(&self.task.File.download_path).join(&self.task.File.folder)
    }

    fn task_id(&self) -> &str {
        &self.task.Basic.task_id
    }

    fn temp_video_file_name(&self) -> String {
        format!("video_{}.{}", self.task_id(), self.task.File.video_file_ext)
    }

    fn temp_audio_file_name(&self) -> String {
        format!("audio_{}.{}", self.task_id(), self.task.File.audio_file_ext)
    }

    /// Deliberately not the `output_` prefix: `temp_output_file_name` is
    /// `output_{task_id}.{merge_ext}`, and the no-collision property must
    /// not depend on VideoContainer having exactly two members.
    fn temp_remux_audio_file_name(&self) -> String {
        format!("remux_{}.{}", self.task_id(), self.task.File.audio_file_ext)
    }

    fn temp_output_file_name(&self) -> String {
        format!(
            "output_{}.{}",
            self.task_id(),
            self.task.File.merge_file_ext
        )
    }

    fn output_audio_file_name(&self) -> String {
        self.output_audio_file
            .clone()
            .unwrap_or_else(|| self.temp_audio_file_name())
    }

    fn final_output_file_name(&self) -> String {
        format!("{}.{}", self.task.File.name, self.task.File.merge_file_ext)
    }

    fn final_video_file_name(&self) -> String {
        format!("{}.{}", self.task.File.name, self.task.File.video_file_ext)
    }

    fn final_mp4_video_file_name(&self) -> String {
        format!("{}.mp4", self.task.File.name)
    }

    fn final_audio_file_name(&self) -> String {
        format!("{}.{}", self.task.File.name, self.task.File.audio_file_ext)
    }

    fn cover_file_name(&self) -> String {
        format!("{}.{}", self.task.File.name, self.resolve_cover_type())
    }

    // ---- option resolution (task pin wins, global snapshot backs off) --

    fn resolve_bool(&self, pinned: Option<bool>, global: bool) -> bool {
        pinned.unwrap_or(global)
    }

    fn resolve_m4a_to_mp3(&self) -> bool {
        self.resolve_bool(self.task.Options.m4a_to_mp3, self.globals.m4a_to_mp3)
    }

    fn resolve_cover_type(&self) -> String {
        const VALID: [&str; 4] = ["jpg", "png", "avif", "webp"];
        match self.task.Options.cover_type.as_deref() {
            Some(pinned) if VALID.contains(&pinned) => pinned.to_string(),
            // Unrecognized pinned value falls back to global, matching the
            // Python ValueError path.
            _ => self.globals.cover_type.clone(),
        }
    }

    fn keep_original_file_type(&self) -> OriginalFileType {
        OriginalFileType::from_code(
            self.task
                .Options
                .keep_original_files_type
                .unwrap_or(self.globals.keep_original_files_type),
        )
    }

    fn should_remux_kept_audio(&self) -> bool {
        self.task.Download.keep_original_files
            && DASH_AUDIO_STREAM_EXTS.contains(&self.task.File.audio_file_ext.as_str())
            && matches!(
                self.keep_original_file_type(),
                OriginalFileType::Both | OriginalFileType::Audio
            )
    }

    // ---- pipeline stages ----------------------------------------------

    /// DASH merge. A surviving previous `output_` temp counts as already
    /// merged (retry after a crash between FFmpeg exit and the rename).
    fn merge_video_audio(&mut self) -> Result<(), MergeOutcome> {
        let cwd = self.cwd();
        let v = cwd.join(self.temp_video_file_name()).exists();
        let a = cwd.join(self.temp_audio_file_name()).exists();
        let o = cwd.join(self.temp_output_file_name()).exists();

        if v && a {
            let cover = self.check_attach_cover();
            let chapter = self.check_attach_chapter();
            let subtitles = self.check_embed_subtitles();
            let cmd = FFmpegCommand::merge_video_audio(
                Path::new(&self.temp_video_file_name()),
                Path::new(&self.temp_audio_file_name()),
                Path::new(&self.temp_output_file_name()),
                cover.as_deref().map(Path::new),
                chapter.as_deref().map(Path::new),
                &subtitles,
            );
            self.run_ffmpeg(cmd)?;
            self.on_merge_completed()
        } else if o && !v && !a {
            self.on_merge_completed()
        } else {
            Err(MergeOutcome::failed(
                error_messages::DOWNLOAD_FAILED,
                error_messages::FILE_NOT_FOUND_DETAIL,
            ))
        }
    }

    /// Legacy FLV multi-part concat.
    fn merge_video_parts(&mut self) -> Result<(), MergeOutcome> {
        let lists = self.create_lists_file();
        // Registered so the relative-files cleanup removes it.
        self.add_file(std::slice::from_ref(&lists), false);

        let cover = self.check_attach_cover();
        let chapter = self.check_attach_chapter();
        let subtitles = self.check_embed_subtitles();
        let cmd = FFmpegCommand::merge_video_parts(
            Path::new(&lists),
            Path::new(&self.temp_output_file_name()),
            cover.as_deref().map(Path::new),
            chapter.as_deref().map(Path::new),
            &subtitles,
        );
        self.run_ffmpeg(cmd)?;
        self.on_merge_completed()
    }

    fn m4a_to_mp3(&mut self) -> Result<(), MergeOutcome> {
        if !self.cwd().join(self.temp_audio_file_name()).exists() {
            return Err(MergeOutcome::failed(
                error_messages::DOWNLOAD_FAILED,
                error_messages::M4A_NOT_FOUND,
            ));
        }
        self.task.Download.status = download_status::CONVERTING;

        // The ext must not flip mid-conversion: a crash would leave the
        // retry path renaming an mp3 that does not exist. Convert into an
        // independent temp name, flip only on success.
        let temp_output = format!("output_{}.mp3", self.task_id());
        self.output_audio_file = Some(temp_output.clone());
        self.converted_audio_ext = Some("mp3");

        let cmd = FFmpegCommand::convert_m4a_to_mp3(
            Path::new(&self.temp_audio_file_name()),
            Path::new(&temp_output),
        );
        self.run_ffmpeg(cmd)?;
        self.on_convert_completed()
    }

    /// Remux the standalone audio stream into the container its extension
    /// promises (fragmented MP4 payloads are rejected by strict players).
    fn remux_audio(&mut self) -> Result<(), MergeOutcome> {
        if !self.cwd().join(self.temp_audio_file_name()).exists() {
            return Err(MergeOutcome::failed(
                error_messages::DOWNLOAD_FAILED,
                error_messages::FILE_NOT_FOUND_DETAIL,
            ));
        }
        self.task.Download.status = download_status::CONVERTING;
        self.output_audio_file = Some(self.temp_remux_audio_file_name());

        let cmd = FFmpegCommand::remux_audio(
            Path::new(&self.temp_audio_file_name()),
            Path::new(&self.temp_remux_audio_file_name()),
        );
        self.run_ffmpeg(cmd)
    }

    fn run_ffmpeg(&mut self, cmd: FFmpegCommand) -> Result<(), MergeOutcome> {
        // Download left progress at 100; the merge bar must restart at 0.
        self.task.Download.progress = 0;

        let mut runner = FFmpegRunner::from_command(&self.exe, &cmd);
        runner.set_cwd(self.cwd());
        // FFmpeg's own Duration report is more accurate; this only carries
        // the percentages before it prints one.
        runner.set_duration(self.task.Episode.duration as f64);
        if let Some(watchdog) = self.watchdog {
            runner.set_watchdog(watchdog);
        }
        let mut sink = self.on_progress.take();
        let outcome = runner.run(|p| {
            self.task.Download.progress = i64::from(p);
            if let Some(sink) = sink.as_mut() {
                sink(i64::from(p));
            }
        });
        match outcome {
            FFmpegOutcome::Success { .. } => Ok(()),
            other => {
                let stderr = match &other {
                    FFmpegOutcome::Failed { stderr_tail, .. }
                    | FFmpegOutcome::NoResponse { stderr_tail } => stderr_tail.clone(),
                    FFmpegOutcome::Error { .. } | FFmpegOutcome::Success { .. } => String::new(),
                };
                let message = ERROR_MAP
                    .iter()
                    .find(|(needle, _)| stderr.contains(needle))
                    .map(|(_, message)| (*message).to_string())
                    .unwrap_or_else(|| other.message());
                Err(MergeOutcome::failed(
                    error_messages::DOWNLOAD_FAILED,
                    format!("{message}\n\n\n{stderr}"),
                ))
            }
        }
    }

    /// Deliver the merge output and clean intermediates.
    fn on_merge_completed(&mut self) -> Result<(), MergeOutcome> {
        let cwd = self.cwd();
        let rename = |name: String| -> Result<(), MergeOutcome> {
            Err(MergeOutcome::failed(error_messages::RENAME_FAILED, name))
        };

        let final_output = match safe_rename(
            &cwd,
            &self.temp_output_file_name(),
            &self.final_output_file_name(),
            self.conflict,
        ) {
            Ok(name) => name,
            Err(e) => return rename(e),
        };

        let mut kept = Vec::new();
        if self.task.Download.keep_original_files {
            kept = self.keep_original_files()?;
        }
        if let Err(e) = safe_remove(&cwd, &self.task.File.relative_files) {
            return rename(e);
        }

        self.delete_embedded_cover();
        self.delete_embedded_subtitles();
        self.delete_embedded_chapter();

        let mut files = vec![final_output];
        files.extend(kept);
        self.add_file(&files, true);
        self.mark_as_completed();
        Ok(())
    }

    /// FFmpeg finished on the audio: drop the raw input (before the ext
    /// flip, while the temp name still points at it), then deliver.
    fn on_convert_completed(&mut self) -> Result<(), MergeOutcome> {
        let cwd = self.cwd();
        if let Err(e) = safe_remove(&cwd, &[self.temp_audio_file_name()]) {
            return Err(MergeOutcome::failed(error_messages::RENAME_FAILED, e));
        }
        if let Some(ext) = self.converted_audio_ext.take() {
            self.task.File.audio_file_ext = ext.to_string();
        }
        self.rename_output_file()
    }

    /// Deliver by rename only (no merge output involved).
    fn rename_output_file(&mut self) -> Result<(), MergeOutcome> {
        let has_video = self.task.Download.type_ & download_type::VIDEO != 0;
        let has_audio = self.task.Download.type_ & download_type::AUDIO != 0;
        let cwd = self.cwd();

        let err = |e: String| MergeOutcome::failed(error_messages::RENAME_FAILED, e);
        let files = if has_video && has_audio {
            self.keep_original_files()?
        } else if has_video {
            vec![safe_rename(
                &cwd,
                &self.temp_video_file_name(),
                &self.final_mp4_video_file_name(),
                self.conflict,
            )
            .map_err(err)?]
        } else if has_audio {
            vec![safe_rename(
                &cwd,
                &self.output_audio_file_name(),
                &self.final_audio_file_name(),
                self.conflict,
            )
            .map_err(err)?]
        } else {
            Vec::new()
        };

        self.add_file(&files, true);
        self.mark_as_completed();
        Ok(())
    }

    /// Rename the kept original streams to their final names.
    fn keep_original_files(&mut self) -> Result<Vec<String>, MergeOutcome> {
        let cwd = self.cwd();
        let err = |e: String| MergeOutcome::failed(error_messages::RENAME_FAILED, e);
        let mut kept = Vec::new();
        match self.keep_original_file_type() {
            OriginalFileType::Both => {
                kept.push(
                    safe_rename(
                        &cwd,
                        &self.temp_video_file_name(),
                        &self.final_video_file_name(),
                        self.conflict,
                    )
                    .map_err(err)?,
                );
                kept.push(
                    safe_rename(
                        &cwd,
                        &self.output_audio_file_name(),
                        &self.final_audio_file_name(),
                        self.conflict,
                    )
                    .map_err(err)?,
                );
            }
            OriginalFileType::Video => kept.push(
                safe_rename(
                    &cwd,
                    &self.temp_video_file_name(),
                    &self.final_video_file_name(),
                    self.conflict,
                )
                .map_err(err)?,
            ),
            OriginalFileType::Audio => kept.push(
                safe_rename(
                    &cwd,
                    &self.output_audio_file_name(),
                    &self.final_audio_file_name(),
                    self.conflict,
                )
                .map_err(err)?,
            ),
        }
        Ok(kept)
    }

    fn mark_as_completed(&mut self) {
        self.task.Download.status = download_status::COMPLETED;
        // FFmpeg progress capped at 99; a completed record must show 100 or
        // the finished task's bar sits mid-way, even after a restart.
        self.task.Download.progress = 100;
        self.task.Basic.completed_time = database::store::now_timestamp();
    }

    /// Replace (`clear`) or extend the delivered-files list.
    fn add_file(&mut self, names: &[String], clear: bool) {
        if clear {
            self.task.File.relative_files.clear();
        }
        for name in names {
            if !self.task.File.relative_files.contains(name) {
                self.task.File.relative_files.push(name.clone());
            }
        }
    }

    // ---- optional attachments ------------------------------------------

    fn check_attach_cover(&mut self) -> Option<String> {
        if !self.resolve_bool(self.task.Options.attach_cover, self.globals.attach_cover) {
            return None;
        }
        let name = self.cover_file_name();
        if self.cwd().join(&name).exists() {
            self.embedded_cover = Some((
                name.clone(),
                self.resolve_bool(
                    self.task.Options.delete_cover_after_attach,
                    self.globals.delete_cover_after_attach,
                ),
            ));
            Some(name)
        } else {
            None
        }
    }

    fn delete_embedded_cover(&mut self) {
        if let Some((name, delete)) = self.embedded_cover.take() {
            if delete {
                let cwd = self.cwd();
                let _ = safe_remove(&cwd, &[name]);
            }
        }
    }

    /// ASS tracks embed only into MKV; subtitle tracks sort ahead of
    /// danmaku and the first subtitle becomes the default track.
    fn check_embed_subtitles(&mut self) -> Vec<SubtitleEntry> {
        self.embedded_subtitles.clear();
        if self.task.File.merge_file_ext != "mkv" {
            return Vec::new();
        }
        let cwd = self.cwd();
        let mut embedded: Vec<(serde_json::Map<String, serde_json::Value>, String)> = Vec::new();
        for entry in &self.task.File.subtitle_track_list {
            let Some(file) = entry.get("file").and_then(|v| v.as_str()) else {
                continue;
            };
            if !cwd.join(file).exists() {
                continue;
            }
            let kind = entry
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            embedded.push((entry.clone(), kind));
        }
        if embedded.is_empty() {
            return Vec::new();
        }

        // Stable sort: same-kind tracks keep parse order; subtitles first.
        embedded.sort_by_key(|(_, kind)| *kind != "subtitle");
        let first_is_subtitle = matches!(embedded.first(), Some((_, kind)) if kind == "subtitle");

        let mut entries = Vec::with_capacity(embedded.len());
        for (index, (entry, kind)) in embedded.iter().enumerate() {
            let file = entry
                .get("file")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            self.embedded_subtitles
                .push((file.to_string(), kind.clone()));
            entries.push(SubtitleEntry {
                file: file.to_string(),
                title: entry
                    .get("title")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(String::from),
                language: entry
                    .get("language")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(String::from),
                default: first_is_subtitle && index == 0,
            });
        }
        entries
    }

    fn delete_embedded_subtitles(&mut self) {
        if self.embedded_subtitles.is_empty() {
            return;
        }
        let delete_danmaku = self.resolve_bool(
            self.task.Options.delete_danmaku_after_embed,
            self.globals.delete_danmaku_after_embed,
        );
        let delete_subtitle = self.resolve_bool(
            self.task.Options.delete_subtitle_after_embed,
            self.globals.delete_subtitle_after_embed,
        );
        let files: Vec<String> = self
            .embedded_subtitles
            .iter()
            .filter(|(_, kind)| {
                if kind == "danmaku" {
                    delete_danmaku
                } else {
                    delete_subtitle
                }
            })
            .map(|(file, _)| file.clone())
            .collect();
        let cwd = self.cwd();
        let _ = safe_remove(&cwd, &files);
    }

    fn check_attach_chapter(&self) -> Option<String> {
        let name = chapter_file_name(self.task_id());
        self.cwd().join(&name).exists().then_some(name)
    }

    fn delete_embedded_chapter(&mut self) {
        // Pure intermediate file: gone after a successful merge.
        let cwd = self.cwd();
        let _ = safe_remove(&cwd, &[chapter_file_name(self.task_id())]);
    }

    /// Write the concat list naming every downloaded part, in order.
    fn create_lists_file(&self) -> String {
        let name = format!("lists_{}.txt", self.task_id());
        let mut body = String::new();
        for i in 0..self.task.Download.video_parts_count {
            body.push_str(&format!(
                "file 'video_{}_{}.{}'\n",
                self.task_id(),
                i,
                self.task.File.video_file_ext
            ));
        }
        let _ = std::fs::write(self.cwd().join(&name), body);
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chapter_file_name_matches_the_python_format() {
        assert_eq!(chapter_file_name("abc123"), "chapter_abc123.txt");
    }

    #[test]
    fn conflict_resolution_codes_map() {
        assert_eq!(
            FileConflictResolution::from_code(1),
            FileConflictResolution::AutoRename
        );
        assert_eq!(
            FileConflictResolution::from_code(2),
            FileConflictResolution::Overwrite
        );
    }

    #[test]
    fn unknown_original_file_type_degrades_to_both() {
        assert_eq!(OriginalFileType::from_code(9), OriginalFileType::Both);
        assert_eq!(OriginalFileType::from_code(2), OriginalFileType::Audio);
    }
}
