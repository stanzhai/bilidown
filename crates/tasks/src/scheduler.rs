//! Download-list scheduler (Python
//! `gui/component/download_list/model.py::manageConcurrentDownloads`).
//!
//! One scan: count active tasks (DOWNLOADING|PARSING), start QUEUED tasks
//! until the parallel limit fills, re-check each started task's status
//! (a task can jump straight to FFMPEG_QUEUED/MERGING, which frees its
//! slot immediately). Re-entrant calls are ignored while a scan runs.

use crate::manager::TaskManager;
use crate::status::download_status;
use config::ConfigStore;
use downloader::{DownloadEngine, EngineConfig, EngineControl, FileSpec};
use parking_lot::Mutex;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// Inclusive clamp bounds for `Download.download_parallel`.
pub const PARALLEL_MIN: i64 = crate::manager::MANAGED_DOWNLOAD_PARALLEL_MIN;
pub const PARALLEL_MAX: i64 = crate::manager::MANAGED_DOWNLOAD_PARALLEL_MAX;

/// One admitted task's engine handle.
struct Slot {
    control: EngineControl,
}

/// Live download engines keyed by task_id.
pub struct Scheduler {
    config: Arc<ConfigStore>,
    slots: Mutex<HashMap<String, Slot>>,
    managing: Mutex<bool>,
}

impl Scheduler {
    pub fn new(config: Arc<ConfigStore>) -> Self {
        Self {
            config,
            slots: Mutex::new(HashMap::new()),
            managing: Mutex::new(false),
        }
    }

    /// `Download.download_parallel` clamped to [1, 10].
    pub fn parallel_limit(&self) -> i64 {
        let raw = self
            .config
            .get("Download", "download_parallel")
            .and_then(|v| v.as_i64())
            .unwrap_or(1);
        raw.clamp(PARALLEL_MIN, PARALLEL_MAX)
    }

    /// Build the engine config from the store (production backoffs).
    pub fn engine_config(&self) -> EngineConfig {
        let get_bool = |item: &str, fallback: bool| {
            self.config
                .get("Download", item)
                .and_then(|v| v.as_bool())
                .unwrap_or(fallback)
        };
        let parallel = self
            .config
            .get("Download", "download_thread")
            .and_then(|v| v.as_i64())
            .unwrap_or(4)
            .clamp(PARALLEL_MIN, PARALLEL_MAX) as usize;
        EngineConfig {
            parallel_chunks: parallel,
            auto_retry_enabled: get_bool("auto_retry_enabled", true),
            auto_retry_max_count: self
                .config
                .get("Download", "auto_retry_max_count")
                .and_then(|v| v.as_i64())
                .unwrap_or(5)
                .clamp(1, 20) as u64,
            preallocate: get_bool("preallocate_file_space", true),
            ..EngineConfig::default()
        }
    }

    /// Start one task's engine (after admission). Returns its control
    /// surface; the engine task updates the manager as it progresses.
    pub fn start_engine(
        &self,
        manager: &Arc<TaskManager>,
        task_id: &str,
        client: network::HttpClient,
        files: Vec<FileSpec>,
    ) -> anyhow::Result<EngineControl> {
        let engine = DownloadEngine::new(
            client,
            self.engine_config(),
            downloader::TokenBucketHandle::unlimited(),
            Arc::new(manager.offset_store(task_id)),
            files,
        );
        let control = engine.control();
        self.slots.lock().insert(
            task_id.to_string(),
            Slot {
                control: control.clone(),
            },
        );
        Ok(control)
    }

    /// Build the engine, register its control slot, and spawn `run()` on
    /// the caller's runtime (ticket 31: the pipeline awaits the final
    /// state; `start_engine` alone drops the engine before it runs).
    /// Returns the join handle plus the control surface so the caller
    /// can sample live progress while awaiting.
    pub fn spawn_engine(
        &self,
        manager: &Arc<TaskManager>,
        task_id: &str,
        client: network::HttpClient,
        files: Vec<FileSpec>,
    ) -> anyhow::Result<(
        tokio::task::JoinHandle<downloader::TaskState>,
        EngineControl,
    )> {
        self.spawn_engine_resumed(manager, task_id, client, files, None)
    }

    /// [`Self::spawn_engine`] with resume seeding: `resume` maps file
    /// keys to their persisted offset tables (a resumed run skips every
    /// chunk already confirmed on disk).
    pub fn spawn_engine_resumed(
        &self,
        manager: &Arc<TaskManager>,
        task_id: &str,
        client: network::HttpClient,
        files: Vec<FileSpec>,
        resume: Option<BTreeMap<String, downloader::OffsetTable>>,
    ) -> anyhow::Result<(
        tokio::task::JoinHandle<downloader::TaskState>,
        EngineControl,
    )> {
        let mut engine = DownloadEngine::new(
            client,
            self.engine_config(),
            downloader::TokenBucketHandle::unlimited(),
            Arc::new(manager.offset_store(task_id)),
            files,
        );
        if let Some(tables) = resume {
            engine.resume_from(tables);
        }
        let control = engine.control();
        self.slots.lock().insert(
            task_id.to_string(),
            Slot {
                control: control.clone(),
            },
        );
        Ok((tokio::spawn(async move { engine.run().await }), control))
    }

    pub fn pause(&self, task_id: &str) {
        if let Some(slot) = self.slots.lock().get(task_id) {
            slot.control.pause();
        }
    }

    /// Live engine snapshot for a running task (progress/speed/pause),
    /// or `None` when no engine owns the id (queued/merge/done stages —
    /// those progress through the DB).
    pub fn snapshot(&self, task_id: &str) -> Option<downloader::TaskState> {
        self.slots.lock().get(task_id).map(|s| s.control.snapshot())
    }

    pub fn release(&self, task_id: &str) {
        self.slots.lock().remove(task_id);
    }

    pub fn active_count(&self) -> usize {
        self.slots.lock().len()
    }

    /// `manageConcurrentDownloads`: admit queued tasks into free slots.
    /// Returns the task ids started this scan. Status transitions are
    /// persisted through the manager's writer thread.
    pub fn auto_manage(&self, manager: &TaskManager) -> Vec<String> {
        {
            let mut managing = self.managing.lock();
            if *managing {
                return Vec::new();
            }
            *managing = true;
        }
        let result = self.manage_locked(manager);
        *self.managing.lock() = false;
        result
    }

    /// Cancel: stop the engine and drop the slot. The caller removes the
    /// DB row via `cancel_many_async` separately.
    pub fn cancel(&self, task_id: &str) {
        if let Some(slot) = self.slots.lock().get(task_id) {
            // Engine pause and cancel share mechanics; a cancel also
            // drops the engine (downloader semantics).
            slot.control.pause();
        }
        self.slots.lock().remove(task_id);
    }

    fn manage_locked(&self, manager: &TaskManager) -> Vec<String> {
        let Ok(tasks) = manager.query(false, None) else {
            return Vec::new();
        };
        let mut active = 0i64;
        let mut queued = Vec::new();
        for task in &tasks {
            match task.Download.status {
                download_status::DOWNLOADING | download_status::PARSING => active += 1,
                download_status::QUEUED => queued.push(task),
                _ => {}
            }
        }
        let limit = self.parallel_limit();
        let mut started = Vec::new();
        for task in queued {
            if active >= limit {
                break;
            }
            // Python starts the task then re-checks: the start can move it
            // straight past DOWNLOADING into the merge pipeline.
            let transitioned = self.start_task(manager, task);
            // Re-read the persisted status (the spawned engine may have
            // already finished and flipped it).
            let now = manager
                .query_by_id(&task.Basic.task_id)
                .ok()
                .flatten()
                .map(|t| t.Download.status)
                .unwrap_or(download_status::QUEUED);
            if transitioned {
                started.push(task.Basic.task_id.clone());
                if !matches!(
                    now,
                    download_status::FFMPEG_QUEUED
                        | download_status::MERGING
                        | download_status::CONVERTING
                        | download_status::ADDITIONAL_PROCESSING
                        | download_status::COMPLETED
                ) {
                    active += 1;
                }
            }
        }
        started
    }

    /// Toggle-start one task (Python `togglePauseResume` path): flip the
    /// row to DOWNLOADING and hand it to the engine layer. The engine
    /// spawn itself lives with the caller (needs HTTP client + files);
    /// here we only persist the transition. Returns false when the task
    /// could not be started.
    fn start_task(&self, manager: &TaskManager, task: &database::TaskInfo) -> bool {
        let mut updated = task.clone();
        updated.Download.status = download_status::DOWNLOADING;
        manager.update_async(&updated);
        true
    }
}
