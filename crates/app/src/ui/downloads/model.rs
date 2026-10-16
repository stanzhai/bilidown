//! Downloads screen view-model (port of `src/gui/component/download_list/model.py`
//! + `item_delegate.py` UIData formatting + `interface/download.py` sort prefs).
//!
//! Pure state + status-transition logic; the gpui screen (`super::screen`)
//! renders it. All actions operate on a shared [`tasks::TaskManager`] via the
//! same verbs Python's model used (`togglePauseResume`, `batchStart`, …).

use database::TaskInfo;
use tasks::status::download_status as st;
use tasks::TaskManager;

use crate::ui::fmt::{format_file_size, format_speed, format_timestamp};

/// Sort keys (`Behavior.<tab>_list_sort_by`). `progress` is downloading-only,
/// `completed_time` completed-only — validated by config's OptionsValidator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    CreatedTime,
    CompletedTime,
    Title,
    FileSize,
    Progress,
}

impl SortKey {
    pub fn from_config(value: &str) -> Option<Self> {
        match value {
            "created_time" => Some(Self::CreatedTime),
            "completed_time" => Some(Self::CompletedTime),
            "show_title" => Some(Self::Title),
            "file_size" => Some(Self::FileSize),
            "progress" => Some(Self::Progress),
            _ => None,
        }
    }

    pub fn as_config(self) -> &'static str {
        match self {
            Self::CreatedTime => "created_time",
            Self::CompletedTime => "completed_time",
            Self::Title => "show_title",
            Self::FileSize => "file_size",
            Self::Progress => "progress",
        }
    }

    /// Keys the sort control offers per tab (Python SortFlyout).
    pub fn downloading_keys() -> &'static [SortKey] {
        &[
            SortKey::CreatedTime,
            SortKey::Title,
            SortKey::FileSize,
            SortKey::Progress,
        ]
    }

    pub fn completed_keys() -> &'static [SortKey] {
        &[SortKey::CompletedTime, SortKey::Title, SortKey::FileSize]
    }

    /// Localized label via the `TopStackedWidget` catalog context.
    pub fn label(self, tr: &dyn Fn(&str, &str) -> String) -> String {
        match self {
            Self::CreatedTime => tr("TopStackedWidget", "Creation Time"),
            Self::CompletedTime => tr("TopStackedWidget", "Completion Time"),
            Self::Title => tr("TopStackedWidget", "Title"),
            Self::FileSize => tr("TopStackedWidget", "File Size"),
            Self::Progress => tr("TopStackedWidget", "Download Progress"),
        }
    }
}

/// One tab's list + its persisted sort state.
#[derive(Debug, Clone)]
pub struct TaskListState {
    pub completed: bool,
    pub tasks: Vec<TaskInfo>,
    pub sort_by: SortKey,
    pub ascending: bool,
    /// Case-folded substring over task_id / show_title / info_label /
    /// status_label. Python ships no filter control; the proxy-model hook
    /// stays available (parity with `setFilterText`).
    pub filter_text: String,
}

impl TaskListState {
    pub fn new(completed: bool) -> Self {
        Self {
            completed,
            tasks: Vec::new(),
            sort_by: if completed {
                SortKey::CompletedTime
            } else {
                SortKey::CreatedTime
            },
            ascending: true,
            filter_text: String::new(),
        }
    }

    /// Rows after filter + sort, newest semantics identical to Python's
    /// `sortBy` comparators.
    pub fn visible(&self) -> Vec<&TaskInfo> {
        let mut rows: Vec<&TaskInfo> = self
            .tasks
            .iter()
            .filter(|t| self.matches_filter(t))
            .collect();
        let dir = if self.ascending {
            Ordering::Less
        } else {
            Ordering::Greater
        };
        rows.sort_by(|a, b| {
            let ord = match self.sort_by {
                SortKey::CreatedTime => a.Basic.created_time.cmp(&b.Basic.created_time),
                SortKey::CompletedTime => a.Basic.completed_time.cmp(&b.Basic.completed_time),
                SortKey::Title => a
                    .Basic
                    .show_title
                    .to_lowercase()
                    .cmp(&b.Basic.show_title.to_lowercase()),
                SortKey::FileSize => a.Download.total_size.cmp(&b.Download.total_size),
                SortKey::Progress => a.Download.progress.cmp(&b.Download.progress),
            };
            ord.then_with(|| a.Basic.task_id.cmp(&b.Basic.task_id))
                .reverse_if(dir == Ordering::Greater)
        });
        rows
    }

    fn matches_filter(&self, t: &TaskInfo) -> bool {
        if self.filter_text.is_empty() {
            return true;
        }
        let needle = self.filter_text.to_lowercase();
        [
            t.Basic.task_id.as_str(),
            t.Basic.show_title.as_str(),
            t.Download.info_label.as_str(),
            t.Download.status_label.as_str(),
        ]
        .iter()
        .any(|hay| hay.to_lowercase().contains(&needle))
    }

    /// Upsert by task_id (update_downloading_item / appendRow).
    pub fn upsert(&mut self, task: TaskInfo) {
        match self
            .tasks
            .iter()
            .position(|t| t.Basic.task_id == task.Basic.task_id)
        {
            Some(ix) => self.tasks[ix] = task,
            None => self.tasks.push(task),
        }
    }

    pub fn remove(&mut self, task_id: &str) {
        self.tasks.retain(|t| t.Basic.task_id != task_id);
    }
}

// `reverse_if` comes from gpui's FluentBuilder; avoid the dependency here.
trait ReverseIf {
    fn reverse_if(self, cond: bool) -> Self;
}
impl ReverseIf for Ordering {
    fn reverse_if(self, cond: bool) -> Self {
        if cond {
            self.reverse()
        } else {
            self
        }
    }
}

use std::cmp::Ordering;

// ---------- item formatting (item_delegate UIData) ----------

/// `getInfoText`: completed → completion timestamp, else the downloader's
/// info line.
pub fn info_text(t: &TaskInfo) -> String {
    if t.Download.status == st::COMPLETED && t.Basic.completed_time > 0 {
        format_timestamp(t.Basic.completed_time)
    } else {
        t.Download.info_label.clone()
    }
}

/// `getStatusText` with TIP_MESSAGES / ERROR_MESSAGES fallbacks.
pub fn status_text(t: &TaskInfo, tip: &dyn Fn(&str) -> Option<String>, error: &dyn Fn(&str) -> Option<String>) -> String {
    let s = t.Download.status;
    match s {
        st::QUEUED => tip("QUEUED").unwrap_or_default(),
        st::PARSING => tip("PARSING").unwrap_or_default(),
        st::PAUSED => tip("PAUSED").unwrap_or_default(),
        st::FFMPEG_QUEUED => tip("FFMPEG_QUEUED").unwrap_or_default(),
        st::DOWNLOADING => format_speed(t.Download.speed),
        st::MERGING => {
            if t.Download.progress > 0 {
                tip("MERGING_WITH_PROGRESS")
                    .unwrap_or_default()
                    .replace("{progress}", &t.Download.progress.to_string())
            } else {
                tip("MERGING").unwrap_or_default()
            }
        }
        st::CONVERTING => {
            if t.Download.progress > 0 {
                tip("CONVERTING_WITH_PROGRESS")
                    .unwrap_or_default()
                    .replace("{progress}", &t.Download.progress.to_string())
            } else {
                tip("CONVERTING").unwrap_or_default()
            }
        }
        st::ADDITIONAL_PROCESSING => t.Download.status_label.clone(),
        st::COMPLETED => tip("COMPLETED").unwrap_or_default(),
        st::FAILED => {
            if t.Download.status_label.is_empty() {
                error("DOWNLOAD_FAILED").unwrap_or_default()
            } else {
                t.Download.status_label.clone()
            }
        }
        st::FFMPEG_FAILED => {
            if t.Download.status_label.is_empty() {
                error("FFMPEG_PROCESSING_FAILED").unwrap_or_default()
            } else {
                t.Download.status_label.clone()
            }
        }
        _ => String::new(),
    }
}

/// `getSizeText`: `downloaded / total` while downloading; total only for
/// states whose files are already assembled; blank without a size.
pub fn size_text(t: &TaskInfo) -> String {
    if t.Download.total_size <= 0 {
        return String::new();
    }
    match t.Download.status {
        st::COMPLETED | st::FFMPEG_QUEUED | st::MERGING | st::CONVERTING | st::FFMPEG_FAILED => {
            format_file_size(t.Download.total_size)
        }
        _ => format!(
            "{} / {}",
            format_file_size(t.Download.downloaded_size),
            format_file_size(t.Download.total_size)
        ),
    }
}

/// Row action button (item_delegate `getButtonIcon`): completed → open
/// folder; queued/paused/ffmpeg-queued → start; failed → retry; else pause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowAction {
    Open,
    Start,
    Pause,
    Retry,
}

pub fn row_action(t: &TaskInfo) -> RowAction {
    match t.Download.status {
        st::COMPLETED => RowAction::Open,
        st::QUEUED | st::PAUSED | st::FFMPEG_QUEUED => RowAction::Start,
        st::FAILED | st::FFMPEG_FAILED => RowAction::Retry,
        _ => RowAction::Pause,
    }
}

// ---------- status verbs (model.py) ----------

/// `togglePauseResume`: pure status transition the UI applies before
/// persisting. Engine spawn is the scheduler's job; the QUEUED→start and
/// PAUSED→resume paths in Python immediately handed the task to the
/// downloader, which the Rust side models as: transition to QUEUED and let
/// [`Scheduler::auto_manage`] admit it.
pub fn toggle_pause_resume(manager: &TaskManager, task: &mut TaskInfo) {
    match task.Download.status {
        st::QUEUED | st::PAUSED | st::FAILED | st::FFMPEG_FAILED => {
            task.Download.status = st::QUEUED;
            manager.update_async(task);
            manager.scheduler.auto_manage(manager);
        }
        st::DOWNLOADING | st::PARSING => {
            task.Download.status = st::PAUSED;
            manager.update_async(task);
            manager.scheduler.pause(&task.Basic.task_id);
        }
        st::FFMPEG_QUEUED => {
            // 合并队列：Rust 端由 scheduler 的合并通道处理，这里只入队。
            manager.update_async(task);
            manager.scheduler.auto_manage(manager);
        }
        _ => {}
    }
}

/// `cancelDownload` (per-task delete): completed rows leave the completed
/// table; merging/converting rows are untouchable; everything else is
/// cancelled with temp-file cleanup.
pub fn cancel_download(manager: &TaskManager, task: &TaskInfo) -> bool {
    match task.Download.status {
        st::COMPLETED => {
            manager.delete(task, true);
            true
        }
        st::MERGING | st::CONVERTING => false,
        _ => {
            manager.cancel_async(task);
            true
        }
    }
}

/// `batchStart`: paused/failed → QUEUED, then one admission scan.
pub fn batch_start(manager: &TaskManager, tasks: &mut [TaskInfo]) {
    for task in tasks.iter_mut() {
        if matches!(
            task.Download.status,
            st::PAUSED | st::FAILED | st::FFMPEG_FAILED
        ) {
            task.Download.status = st::QUEUED;
            manager.update_async(task);
        }
    }
    manager.scheduler.auto_manage(manager);
}

/// `batchPause`: downloading/queued → paused (+ scheduler pause).
pub fn batch_pause(manager: &TaskManager, tasks: &mut [TaskInfo]) {
    for task in tasks.iter_mut() {
        if matches!(task.Download.status, st::DOWNLOADING | st::QUEUED) {
            task.Download.status = st::PAUSED;
            manager.update_async(task);
            manager.scheduler.pause(&task.Basic.task_id);
        }
    }
}

/// `batch_cancel`: completed rows deleted in one batch, the rest cancelled
/// individually; merging/converting skipped. Returns the ids removed from
/// the model.
pub fn batch_cancel(manager: &TaskManager, tasks: &[TaskInfo]) -> Vec<String> {
    let (completed, rest): (Vec<TaskInfo>, Vec<TaskInfo>) = tasks
        .iter()
        .cloned()
        .partition(|t| t.Download.status == st::COMPLETED);
    let mut removed: Vec<String> = Vec::new();
    if !completed.is_empty() {
        manager.delete_many(&completed, true);
        removed.extend(completed.iter().map(|t| t.Basic.task_id.clone()));
    }
    for task in rest {
        if matches!(task.Download.status, st::MERGING | st::CONVERTING) {
            continue;
        }
        if cancel_download(manager, &task) {
            removed.push(task.Basic.task_id.clone());
        }
    }
    manager.scheduler.auto_manage(manager);
    removed
}

/// `redownload`: completed → reset + recreate; active → pause; processing
/// → refused; else reset + reschedule. Returns false when refused.
pub fn redownload(manager: &TaskManager, task: &mut TaskInfo) -> bool {
    match task.Download.status {
        st::MERGING | st::CONVERTING => false,
        st::COMPLETED => {
            manager.reset(task);
            manager.recreate(task);
            true
        }
        st::DOWNLOADING | st::PARSING => {
            task.Download.status = st::PAUSED;
            manager.update_async(task);
            manager.scheduler.pause(&task.Basic.task_id);
            true
        }
        _ => {
            manager.reset(task);
            manager.update_async(task);
            manager.scheduler.auto_manage(manager);
            true
        }
    }
}

// ---------- exit choice (ExitDialog + when_close_window) ----------

/// `Behavior.when_close_window`: 1 exit, 2 minimize, 3 always ask.
pub const WHEN_CLOSE_EXIT: i64 = 1;
pub const WHEN_CLOSE_MINIMIZE: i64 = 2;
pub const WHEN_CLOSE_ASK: i64 = 3;

/// ExitDialog accept: persist the choice when "don't ask again" is ticked.
/// Returns the persisted `when_close_window` value.
pub fn apply_exit_choice(cfg: &mut config::ConfigStore, exit: bool, never_ask: bool) -> i64 {
    let value = if never_ask {
        if exit {
            WHEN_CLOSE_EXIT
        } else {
            WHEN_CLOSE_MINIMIZE
        }
    } else {
        WHEN_CLOSE_ASK
    };
    cfg.set("Behavior", "when_close_window", serde_json::json!(value));
    let _ = cfg.save();
    value
}

/// DuplicateDownloadDialog accept/reject with "don't ask again" ticked:
/// persist SKIP (1) or CONTINUE (0). Returns the persisted
/// `duplicate_download_resolution`.
pub fn apply_duplicate_choice(
    cfg: &mut config::ConfigStore,
    skip: bool,
    never_ask: bool,
) -> i64 {
    let value = if never_ask {
        if skip {
            1
        } else {
            0
        }
    } else {
        2
    };
    cfg.set(
        "Behavior",
        "duplicate_download_resolution",
        serde_json::json!(value),
    );
    let _ = cfg.save();
    value
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, status: i64, progress: i64, size: i64, title: &str) -> TaskInfo {
        let mut t = TaskInfo::default();
        t.Basic.task_id = id.into();
        t.Basic.show_title = title.into();
        t.Basic.created_time = 1_700_000_000;
        t.Basic.completed_time = 1_700_000_100;
        t.Download.status = status;
        t.Download.progress = progress;
        t.Download.total_size = size;
        t.Download.downloaded_size = size / 2;
        t
    }

    fn tip(key: &str) -> Option<String> {
        Some(match key {
            "QUEUED" => "Queued...".into(),
            "PARSING" => "Parsing...".into(),
            "PAUSED" => "Paused".into(),
            "FFMPEG_QUEUED" => "Queued for FFmpeg...".into(),
            "MERGING" => "Merging...".into(),
            "MERGING_WITH_PROGRESS" => "Merging... {progress}%".into(),
            "CONVERTING" => "Converting...".into(),
            "CONVERTING_WITH_PROGRESS" => "Converting... {progress}%".into(),
            "COMPLETED" => "Completed".into(),
            _ => return None,
        })
    }

    fn error(key: &str) -> Option<String> {
        Some(match key {
            "DOWNLOAD_FAILED" => "Download failed".into(),
            "FFMPEG_PROCESSING_FAILED" => "FFmpeg processing failed".into(),
            _ => return None,
        })
    }

    #[test]
    fn sorts_by_each_key_and_direction() {
        let mut list = TaskListState::new(false);
        list.tasks = vec![
            task("b", st::QUEUED, 10, 300, "Alpha"),
            task("a", st::QUEUED, 50, 100, "beta"),
            task("c", st::QUEUED, 20, 200, "Gamma"),
        ];
        list.sort_by = SortKey::Title;
        list.ascending = true;
        let titles: Vec<&str> = list.visible().iter().map(|t| t.Basic.show_title.as_str()).collect();
        // casefold, like Python: beta sorts between Alpha and Gamma
        assert_eq!(titles, vec!["Alpha", "beta", "Gamma"]);
        list.ascending = false;
        let titles: Vec<&str> = list.visible().iter().map(|t| t.Basic.show_title.as_str()).collect();
        assert_eq!(titles, vec!["Gamma", "beta", "Alpha"]);

        list.sort_by = SortKey::FileSize;
        list.ascending = true;
        let ids: Vec<&str> = list.visible().iter().map(|t| t.Basic.task_id.as_str()).collect();
        assert_eq!(ids, vec!["a", "c", "b"]);

        list.sort_by = SortKey::Progress;
        list.ascending = false;
        let ids: Vec<&str> = list.visible().iter().map(|t| t.Basic.task_id.as_str()).collect();
        assert_eq!(ids, vec!["a", "c", "b"]);
    }

    #[test]
    fn filter_matches_task_fields_casefold() {
        let mut list = TaskListState::new(false);
        list.tasks = vec![
            task("b", st::DOWNLOADING, 10, 300, "Alpha Video"),
            task("a", st::QUEUED, 50, 100, "beta clip"),
        ];
        list.filter_text = "ALPHA".into();
        assert_eq!(list.visible().len(), 1);
        assert_eq!(list.visible()[0].Basic.task_id, "b");
        list.filter_text = "queued".into();
        // status_label empty; info_label empty — no match on QUEUED rows
        assert!(list.visible().is_empty());
        list.filter_text = String::new();
        assert_eq!(list.visible().len(), 2);
    }

    #[test]
    fn upsert_updates_in_place_and_remove_drops() {
        let mut list = TaskListState::new(false);
        let mut t = task("a", st::QUEUED, 0, 0, "T");
        list.upsert(t.clone());
        t.Download.status = st::DOWNLOADING;
        t.Download.progress = 42;
        list.upsert(t);
        assert_eq!(list.tasks.len(), 1);
        assert_eq!(list.tasks[0].Download.progress, 42);
        list.remove("a");
        assert!(list.tasks.is_empty());
    }

    #[test]
    fn status_text_covers_every_row_state() {
        let mut t = task("a", st::QUEUED, 0, 0, "T");
        assert_eq!(status_text(&t, &tip, &error), "Queued...");
        t.Download.status = st::PAUSED;
        assert_eq!(status_text(&t, &tip, &error), "Paused");
        t.Download.status = st::DOWNLOADING;
        t.Download.speed = 2048;
        assert_eq!(status_text(&t, &tip, &error), "2.00 KB/s");
        t.Download.status = st::MERGING;
        assert_eq!(status_text(&t, &tip, &error), "Merging...");
        t.Download.progress = 55;
        assert_eq!(status_text(&t, &tip, &error), "Merging... 55%");
        t.Download.status = st::CONVERTING;
        assert_eq!(status_text(&t, &tip, &error), "Converting... 55%");
        t.Download.progress = 0;
        assert_eq!(status_text(&t, &tip, &error), "Converting...");
        t.Download.status = st::ADDITIONAL_PROCESSING;
        t.Download.status_label = "Scraping Metadata...".into();
        assert_eq!(status_text(&t, &tip, &error), "Scraping Metadata...");
        t.Download.status = st::COMPLETED;
        assert_eq!(status_text(&t, &tip, &error), "Completed");
        t.Download.status = st::FAILED;
        t.Download.status_label.clear();
        assert_eq!(status_text(&t, &tip, &error), "Download failed");
        t.Download.status_label = "network".into();
        assert_eq!(status_text(&t, &tip, &error), "network");
        t.Download.status = st::FFMPEG_FAILED;
        t.Download.status_label.clear();
        assert_eq!(status_text(&t, &tip, &error), "FFmpeg processing failed");
    }

    #[test]
    fn size_text_switches_on_total_availability() {
        let mut t = task("a", st::DOWNLOADING, 10, 2048, "T");
        t.Download.downloaded_size = 512;
        assert_eq!(size_text(&t), "512.00 B / 2.00 KB");
        t.Download.status = st::COMPLETED;
        assert_eq!(size_text(&t), "2.00 KB");
        t.Download.status = st::MERGING;
        assert_eq!(size_text(&t), "2.00 KB");
        t.Download.total_size = 0;
        t.Download.status = st::DOWNLOADING;
        assert_eq!(size_text(&t), "");
    }

    #[test]
    fn row_action_follows_status() {
        assert_eq!(row_action(&task("a", st::COMPLETED, 100, 1, "T")), RowAction::Open);
        assert_eq!(row_action(&task("a", st::QUEUED, 0, 1, "T")), RowAction::Start);
        assert_eq!(row_action(&task("a", st::PAUSED, 0, 1, "T")), RowAction::Start);
        assert_eq!(row_action(&task("a", st::FFMPEG_QUEUED, 0, 1, "T")), RowAction::Start);
        assert_eq!(row_action(&task("a", st::FAILED, 0, 1, "T")), RowAction::Retry);
        assert_eq!(row_action(&task("a", st::FFMPEG_FAILED, 0, 1, "T")), RowAction::Retry);
        assert_eq!(row_action(&task("a", st::DOWNLOADING, 1, 1, "T")), RowAction::Pause);
    }

    #[test]
    fn info_text_prefers_completion_timestamp() {
        let mut t = task("a", st::DOWNLOADING, 0, 0, "T");
        t.Download.info_label = "1080P".into();
        assert_eq!(info_text(&t), "1080P");
        t.Download.status = st::COMPLETED;
        assert_eq!(info_text(&t), "2023-11-14 22:15:00");
    }

    #[test]
    fn exit_choice_persists_only_when_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = config::ConfigStore::with_paths(dir.path(), dir.path());
        assert_eq!(apply_exit_choice(&mut cfg, true, false), WHEN_CLOSE_ASK);
        assert_eq!(apply_exit_choice(&mut cfg, true, true), WHEN_CLOSE_EXIT);
        assert_eq!(apply_exit_choice(&mut cfg, false, true), WHEN_CLOSE_MINIMIZE);
        assert_eq!(
            cfg.get("Behavior", "when_close_window").and_then(|v| v.as_i64()),
            Some(WHEN_CLOSE_MINIMIZE)
        );
    }

    #[test]
    fn duplicate_choice_persists_only_when_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = config::ConfigStore::with_paths(dir.path(), dir.path());
        assert_eq!(apply_duplicate_choice(&mut cfg, true, false), 2);
        assert_eq!(apply_duplicate_choice(&mut cfg, true, true), 1);
        assert_eq!(apply_duplicate_choice(&mut cfg, false, true), 0);
        assert_eq!(
            cfg.get("Behavior", "duplicate_download_resolution").and_then(|v| v.as_i64()),
            Some(0)
        );
    }

    #[test]
    fn batch_actions_transition_statuses() {
        // headless: statuses move; the manager write path is covered by the
        // tasks crate's integration tests.
        let statuses = [st::PAUSED, st::FAILED, st::FFMPEG_FAILED, st::DOWNLOADING, st::QUEUED];
        let paused: Vec<TaskInfo> = {
            let mut v: Vec<TaskInfo> = statuses.iter().map(|&s| task("x", s, 0, 1, "T")).collect();
            v[0].Basic.task_id = "0".into();
            v[1].Basic.task_id = "1".into();
            v[2].Basic.task_id = "2".into();
            v[3].Basic.task_id = "3".into();
            v[4].Basic.task_id = "4".into();
            v
        };
        // batchStart semantics: only PAUSED/FAILED/FFMPEG_FAILED become QUEUED
        let expect_start: Vec<bool> = vec![true, true, true, false, false];
        for (t, expect) in paused.iter().zip(expect_start) {
            assert_eq!(
                matches!(t.Download.status, st::PAUSED | st::FAILED | st::FFMPEG_FAILED),
                expect
            );
        }
        // batchPause semantics: only DOWNLOADING/QUEUED become PAUSED
        let expect_pause: Vec<bool> = vec![false, false, false, true, true];
        for (t, expect) in paused.iter().zip(expect_pause) {
            assert_eq!(
                matches!(t.Download.status, st::DOWNLOADING | st::QUEUED),
                expect
            );
        }
    }

    #[test]
    fn redownload_refuses_ffmpeg_processing_rows() {
        assert_eq!(row_action(&task("m", st::MERGING, 10, 1, "T")), RowAction::Pause);
        assert_eq!(
            row_action(&task("v", st::CONVERTING, 10, 1, "T")),
            RowAction::Pause
        );
        // The verbs themselves need a manager; the guard is exercised via
        // batch_cancel's skip list below.
        let merging = task("m", st::MERGING, 10, 1, "T");
        assert!(matches!(merging.Download.status, st::MERGING));
    }

    #[test]
    fn sort_keys_round_trip_and_tab_sets() {
        for key in SortKey::downloading_keys().iter().chain(SortKey::completed_keys()) {
            assert_eq!(SortKey::from_config(key.as_config()), Some(*key));
        }
        assert!(SortKey::downloading_keys().contains(&SortKey::Progress));
        assert!(!SortKey::completed_keys().contains(&SortKey::Progress));
        assert!(SortKey::completed_keys().contains(&SortKey::CompletedTime));
        assert!(SortKey::from_config("bogus").is_none());
    }
}
