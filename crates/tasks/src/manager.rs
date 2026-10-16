//! TaskManager: the task lifecycle facade (Python `task/manager.py`).
//!
//! All writes to task.db run on one dedicated writer thread (SQLite
//! serializes there; callers never fight over the write lock). Progress
//! snapshots coalesce latest-wins under a lock; structural ops flush the
//! pending snapshots first so an UPDATE can never land before its INSERT.
//! Temp-file removal runs on its own thread.

use crate::duplicate::{self, AskOutcome, DuplicateCheck};
use crate::info::{self, CreateInputs, NamingInput};
use crate::numbering::Numbering;
use crate::scheduler::Scheduler;
use config::ConfigStore;
use database::info::TaskInfo;
use database::{TaskDatabase, TaskRecord};
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

pub const MANAGED_DOWNLOAD_PARALLEL_MIN: i64 = 1;
pub const MANAGED_DOWNLOAD_PARALLEL_MAX: i64 = 10;

/// What the writer thread should do. Structural ops flush pending
/// snapshots first (Python `_flush_pending_snapshots` ordering).
enum WriteOp {
    /// Flush coalesced progress snapshots (dedup by task_id).
    Flush(()),
    Add(Vec<TaskRecord>),
    Delete(Vec<String>, bool),
    MoveToCompleted(TaskRecord),
    Recreate(TaskRecord),
    /// Caller-supplied blocking closure (durability tests).
    Block(std::sync::mpsc::Receiver<()>),
    /// Drain everything submitted so far, then ack.
    Sentinel(Sender<()>),
}

struct CancelOp {
    snapshots: Vec<(String, PathBuf, Vec<String>)>,
}

/// Result of one `create()` batch.
#[derive(Debug, Default)]
pub struct CreateResult {
    pub created: Vec<TaskInfo>,
    /// Episodes reported back instead of being created (NEED_PARSE_BIT).
    pub need_reparse: Vec<serde_json::Map<String, Value>>,
    /// Episodes skipped by duplicate resolution.
    pub skipped_duplicates: Vec<String>,
    /// (title, error) pairs for episodes whose task failed to build.
    pub failed: Vec<(String, String)>,
}

pub struct TaskManager {
    db_path: PathBuf,
    inner: Arc<TaskManagerInner>,
    writer_handle: Mutex<Option<JoinHandle<()>>>,
    cancel: Mutex<Sender<CancelOp>>,
    cancel_handle: Mutex<Option<JoinHandle<()>>>,
    /// Take-a-number / rollback lock (atomic take + increment).
    numbering_lock: Mutex<Numbering>,
    pub scheduler: Scheduler,
}

impl TaskManager {
    /// Open (creating if needed) the task DB at `db_path` and start the
    /// writer + cancel threads.
    pub fn open(db_path: impl Into<PathBuf>, config: Arc<ConfigStore>) -> anyhow::Result<Self> {
        let db_path = db_path.into();
        // Writer owns its connection for the process lifetime.
        let writer_db = TaskDatabase::open(&db_path)?;
        let (writer_tx, writer_rx) = channel::<WriteOp>();
        let writer = Arc::new(TaskManagerInner {
            db_path: db_path.clone(),
            pending_updates: Mutex::new(HashMap::new()),
            update_flush_scheduled: Mutex::new(false),
            writer: Mutex::new(writer_tx),
        });
        let writer_state = writer.clone();
        let writer_handle = std::thread::Builder::new()
            .name("task-db".into())
            .spawn(move || {
                let mut db = writer_db;
                let state = writer_state;
                for op in writer_rx {
                    match op {
                        WriteOp::Flush(()) => {
                            let drained: HashMap<String, String> = {
                                let mut pending = state.pending_updates.lock();
                                let drained = pending.drain().collect();
                                *state.update_flush_scheduled.lock() = false;
                                drained
                            };
                            write_updates(&mut db, &drained);
                        }
                        WriteOp::Block(rx) => {
                            // Durability tests park the writer here.
                            let _ = rx.recv();
                        }
                        WriteOp::Add(records) => {
                            flush_pending(&mut db, &state);
                            if let Err(e) = db.add_task_records(&records, false) {
                                log_error(&format!("保存下载任务记录失败: {e}"));
                            }
                        }
                        WriteOp::Delete(ids, completed) => {
                            flush_pending(&mut db, &state);
                            if let Err(e) = db.delete_tasks(&ids, completed) {
                                log_error(&format!("删除下载任务记录失败: {e}"));
                            }
                        }
                        WriteOp::MoveToCompleted(record) => {
                            flush_pending(&mut db, &state);
                            if let Err(e) = db.move_to_completed(&record) {
                                log_error(&format!("标记下载任务为已完成失败: {}", record.task_id));
                                let _ = e;
                            }
                        }
                        WriteOp::Recreate(record) => {
                            flush_pending(&mut db, &state);
                            if let Err(e) = db.recreate_task(&record) {
                                log_error(&format!("重建下载任务记录失败: {}", record.task_id));
                                let _ = e;
                            }
                        }
                        WriteOp::Sentinel(tx) => {
                            // Ack after everything before it; keep looping
                            // so shutdown can join cleanly.
                            let _ = tx.send(());
                        }
                    }
                }
            })?;

        let (cancel_tx, cancel_rx) = channel::<CancelOp>();
        let cancel_handle = std::thread::Builder::new()
            .name("task-cancel".into())
            .spawn(move || {
                for op in cancel_rx {
                    for (task_id, directory, file_names) in op.snapshots {
                        if file_names.is_empty() {
                            continue;
                        }
                        // Per-task try/except: one bad task never stops the rest.
                        if let Err(e) = remove_files(&directory, &file_names) {
                            log_error(&format!("删除下载任务临时文件失败: {task_id}: {e}"));
                        }
                    }
                }
            })?;

        Ok(Self {
            scheduler: Scheduler::new(config),
            db_path,
            inner: writer,
            writer_handle: Mutex::new(Some(writer_handle)),
            cancel: Mutex::new(cancel_tx),
            cancel_handle: Mutex::new(Some(cancel_handle)),
            numbering_lock: Mutex::new(Numbering::default()),
        })
    }

    /// A short-lived reader connection on the caller thread (WAL allows
    /// concurrent reads while the writer thread holds its connection).
    pub fn reader(&self) -> anyhow::Result<TaskDatabase> {
        TaskDatabase::open(&self.db_path)
    }

    // ---- queries (reader connection) ----

    pub fn query(&self, completed: bool, limit: Option<i64>) -> anyhow::Result<Vec<TaskInfo>> {
        Ok(self.reader()?.query_tasks(completed, limit)?)
    }

    pub fn query_by_id(&self, task_id: &str) -> anyhow::Result<Option<TaskInfo>> {
        let reader = self.reader()?;
        for completed in [false, true] {
            if let Some(task) = reader.query_task_by_id(task_id, completed)? {
                return Ok(Some(task));
            }
        }
        Ok(None)
    }

    pub fn count(&self, completed: bool) -> anyhow::Result<i64> {
        Ok(self.reader()?.count_tasks(completed)?)
    }

    pub fn is_duplicate(&self, episode: &serde_json::Map<String, Value>) -> anyhow::Result<bool> {
        Ok(duplicate::is_duplicate(&self.reader()?, episode))
    }

    // ---- create ----

    /// `create()`: build tasks for a batch of episodes. Numbering is
    /// checked after duplicate detection; a failed build rolls its number
    /// back and the batch continues.
    pub fn create(
        &self,
        config: &ConfigStore,
        episode_data: &parsers::EpisodeData,
        episodes: &[serde_json::Map<String, Value>],
        options: Option<&serde_json::Map<String, Value>>,
        naming: impl Fn(&serde_json::Map<String, Value>) -> NamingInput,
    ) -> anyhow::Result<CreateResult> {
        let reader = self.reader()?;
        let mode = crate::status::NumberingType::from_code(
            config
                .get("File Naming", "numbering_type")
                .and_then(Value::as_i64)
                .unwrap_or(2),
        );
        let inputs = CreateInputs {
            config,
            episode_data,
            video_quality_id: 200,
            audio_quality_id: 30300,
            video_codec_id: 20,
        };
        let mut result = CreateResult::default();

        for episode in episodes {

            if info::needs_reparse(episode) {
                result.need_reparse.push(episode.clone());
                continue;
            }

            let ask = || AskOutcome::Skip;
            if duplicate::check_duplicate(&reader, config, episode, options, ask)
                == DuplicateCheck::Skip
            {
                result
                    .skipped_duplicates
                    .push(episode.get("title").and_then(Value::as_str).unwrap_or_default().to_string());
                continue;
            }

            // Duplicate check first, then number: take + increment atomic.
            let (number, numbering_token) = {
                let mut numbering = self.numbering_lock.lock();
                let parse_number = episode.get("number").and_then(Value::as_str).map(str::to_owned);
                let taken = numbering.take(mode, parse_number.as_deref());
                taken
            };

            match info::episode_to_task_info(&inputs, episode, &number, options, &naming(episode)) {
                Ok(task_info) => result.created.push(task_info),
                Err(error) => {
                    let title = episode.get("title").and_then(Value::as_str).unwrap_or_default().to_string();
                    self.numbering_lock.lock().rollback(numbering_token);
                    result.failed.push((title, error.to_string()));
                }
            }
        }

        if !result.created.is_empty() {
            let records: Vec<TaskRecord> = result
                .created
                .iter()
                .map(|t| TaskDatabase::build_record(t, false))
                .collect();
            // Channel error only after shutdown; not an anyhow-convertible
            // error (SendError carries a non-Sync Receiver), so log it.
            if self.inner.writer.lock().send(WriteOp::Add(records)).is_err() {
                log_error("任务写入线程已关闭，丢弃新增任务");
            }
            // Tasks enter the queue; the scheduler picks them up.
            self.scheduler.auto_manage(self);
        }
        Ok(result)
    }

    // ---- progress updates ----

    /// `update_async`: sample + serialize inside the lock (old-snapshot
    /// overwrites are impossible), coalesce latest-wins, schedule a flush.
    pub fn update_async(&self, task_info: &TaskInfo) {
        let task_id = task_info.Basic.task_id.clone();
        let json = task_info.to_json();
        {
            let mut pending = self.inner.pending_updates.lock();
            pending.insert(task_id, json);
        }
        self.inner.schedule_flush();
    }

    fn discard_pending_updates(&self, task_ids: &[String]) {
        let mut pending = self.inner.pending_updates.lock();
        for id in task_ids {
            pending.remove(id);
        }
    }

    /// The offset-table store for one task's engine (scheduler start).
    pub fn offset_store(self: &Arc<Self>, task_id: &str) -> impl downloader::OffsetTableStore {
        struct Store {
            manager: std::sync::Weak<TaskManagerInner>,
            task_id: String,
        }
        impl downloader::OffsetTableStore for Store {
            fn save(&self, file_key: &str, table: &downloader::OffsetTable) {
                let Some(state) = self.manager.upgrade() else {
                    return;
                };
                let mut pending = state.pending_updates.lock();
                // Seed from the persisted row so the flush writes a full
                // TaskInfo snapshot (Python: update_async on the whole
                // task); a stub would corrupt the row's JSON.
                let entry = pending
                    .entry(self.task_id.clone())
                    .or_insert_with(|| {
                        TaskDatabase::open(&state.db_path)
                            .ok()
                            .and_then(|db| db.query_task_json(&self.task_id, false).ok().flatten())
                            .unwrap_or_else(|| String::from("{}"))
                    });
                // Merge chunk_offsets into the serialized snapshot's
                // Download.files[file_key]; offsets ride the next flush.
                if let Ok(mut snapshot) = serde_json::from_str::<serde_json::Value>(entry) {
                    if let Some(files) = snapshot
                        .get_mut("Download")
                        .and_then(|d| d.get_mut("files"))
                        .and_then(|f| f.as_object_mut())
                    {
                        let file = files
                            .entry(file_key.to_string())
                            .or_insert_with(|| serde_json::json!({}));
                        if let Some(map) = file.as_object_mut() {
                            map.insert("chunk_offsets".into(), table.to_json());
                        }
                        *entry = serde_json::to_string(&snapshot)
                            .unwrap_or_else(|_| entry.clone());
                    }
                }
                drop(pending);
                state.schedule_flush();
            }
        }
        Store {
            manager: Arc::downgrade(&self.inner),
            task_id: task_id.to_string(),
        }
    }

    // ---- structural ops ----

    pub fn delete_many(&self, task_infos: &[TaskInfo], completed: bool) {
        if task_infos.is_empty() {
            return;
        }
        let ids: Vec<String> = task_infos.iter().map(|t| t.Basic.task_id.clone()).collect();
        self.discard_pending_updates(&ids);
        let _ = self.inner.writer.lock().send(WriteOp::Delete(ids, completed));
    }

    pub fn delete(&self, task_info: &TaskInfo, completed: bool) {
        self.delete_many(std::slice::from_ref(task_info), completed);
    }

    /// `cancel_many_async`: delete rows and remove temp files on the
    /// cancel thread.
    pub fn cancel_many_async(&self, task_infos: &[TaskInfo]) {
        if task_infos.is_empty() {
            return;
        }
        self.delete_many(task_infos, false);
        let snapshots = task_infos
            .iter()
            .map(|t| {
                (
                    t.Basic.task_id.clone(),
                    PathBuf::from(&t.File.download_path).join(&t.File.folder),
                    t.File.relative_files.clone(),
                )
            })
            .collect();
        let _ = self.cancel.lock().send(CancelOp { snapshots });
    }

    pub fn cancel_async(&self, task_info: &TaskInfo) {
        self.cancel_many_async(std::slice::from_ref(task_info));
    }

    /// `mark_as_completed`: discard pending snapshots, build the record
    /// on the caller thread, submit the move; `wait` blocks until the
    /// write lands (bounded by `timeout` — a timeout logs and returns,
    /// the write still completes on the writer thread).
    pub fn mark_as_completed(&self, task_info: &TaskInfo, wait: bool, timeout: f64) {
        self.discard_pending_updates(std::slice::from_ref(&task_info.Basic.task_id));
        let record = TaskDatabase::build_record(task_info, true);
        let (tx, rx) = channel();
        {
            let writer = self.inner.writer.lock();
            if writer.send(WriteOp::Flush(())).is_err() {
                return;
            }
            if writer.send(WriteOp::MoveToCompleted(record)).is_err() {
                return;
            }
            if wait {
                let _ = writer.send(WriteOp::Sentinel(tx));
            }
        }
        if wait
            && !rx.recv_timeout(Duration::from_secs_f64(timeout)).is_ok() {
                log_error("等待标记下载任务为已完成的写入落盘超时");
            }
    }

    /// `reset`: rewind a task to its initial state (caller persists).
    pub fn reset(&self, task_info: &mut TaskInfo) {
        task_info.Download.status = crate::status::download_status::QUEUED;
        task_info.Download.queue.clear();
        task_info.Download.files.clear();
        task_info.Download.progress = 0;
        task_info.Download.total_size = 0;
        task_info.Download.downloaded_size = 0;
        task_info.Download.speed = 0;
        self.cancel_many_async_no_delete(task_info);
    }

    /// reset()'s temp-file removal without the DB delete.
    fn cancel_many_async_no_delete(&self, task_info: &TaskInfo) {
        let snapshots = vec![(
            task_info.Basic.task_id.clone(),
            PathBuf::from(&task_info.File.download_path).join(&task_info.File.folder),
            task_info.File.relative_files.clone(),
        )];
        let _ = self.cancel.lock().send(CancelOp { snapshots });
    }

    /// `recreate`: move a completed task back to the download table and
    /// hand it to the scheduler.
    pub fn recreate(&self, task_info: &TaskInfo) {
        self.discard_pending_updates(std::slice::from_ref(&task_info.Basic.task_id));
        let record = TaskDatabase::build_record(task_info, false);
        let _ = self
            .inner
            .writer
            .lock()
            .send(WriteOp::Recreate(record));
        self.scheduler.auto_manage(self);
    }

    /// Park the writer thread until `release` fires (durability tests).
    pub fn block_writer(&self, release: std::sync::mpsc::Receiver<()>) {
        let _ = self.inner.writer.lock().send(WriteOp::Block(release));
    }

    /// `shutdown`: wait for every queued write to land (bounded).
    pub fn shutdown(&self, timeout: f64) {
        let (tx, rx) = channel();
        let writer = self.inner.writer.lock();
        let _ = writer.send(WriteOp::Flush(()));
        let _ = writer.send(WriteOp::Sentinel(tx));
        drop(writer);
        if rx.recv_timeout(Duration::from_secs_f64(timeout)).is_err() {
            log_error("等待下载任务写入完成超时");
        }
        // Drop the senders so both threads exit.
        *self.inner.writer.lock() = channel().0;
        *self.cancel.lock() = channel().0;
        if let Some(handle) = self.writer_handle.lock().take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.cancel_handle.lock().take() {
            let _ = handle.join();
        }
    }
}

fn log_error(message: &str) {
    eprintln!("[tasks] {message}");
}

fn write_updates(db: &mut TaskDatabase, updates: &HashMap<String, String>) {
    for (task_id, json) in updates {
        // Python batches these in one transaction; SQLite autocommits per
        // statement here, which only widens the durability window between
        // rows, never the ordering.
        if let Err(e) = db.update_task_json(task_id, json) {
            log_error(&format!("异步保存下载任务失败: {e}"));
        }
    }
}

/// `safe_remove`: missing files are fine; anything else logs and the
/// remaining files still run (Python: unlink missing_ok, cancel executor
/// catches per task).
fn remove_files(directory: &Path, file_names: &[String]) -> Result<(), String> {
    for name in file_names {
        let path = directory.join(name);
        if let Err(e) = std::fs::remove_file(&path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                return Err(format!("{path:?}: {e}"));
            }
        }
    }
    Ok(())
}

/// State shared by the manager, the writer thread and the offset-table
/// stores (which outlive borrows of the manager).
struct TaskManagerInner {
    db_path: PathBuf,
    pending_updates: Mutex<HashMap<String, String>>,
    update_flush_scheduled: Mutex<bool>,
    writer: Mutex<Sender<WriteOp>>,
}

impl TaskManagerInner {
    fn schedule_flush(&self) {
        let should = {
            let mut scheduled = self.update_flush_scheduled.lock();
            if *scheduled {
                false
            } else {
                *scheduled = true;
                true
            }
        };
        if should {
            let _ = self.writer.lock().send(WriteOp::Flush(()));
        }
    }
}

/// Drain-and-write the pending snapshots (structural-op preamble).
fn flush_pending(db: &mut TaskDatabase, state: &TaskManagerInner) {
    let drained: HashMap<String, String> = {
        let mut pending = state.pending_updates.lock();
        let drained = pending.drain().collect();
        *state.update_flush_scheduled.lock() = false;
        drained
    };
    write_updates(db, &drained);
}
