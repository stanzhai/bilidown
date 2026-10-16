//! Chunked download engine (ticket 13).
//!
//! Ports the Python `ChunkWorker`/`Downloader` download path: 4 MiB chunks,
//! 1-10 of them in parallel per file, per-chunk offset table persisted for
//! resume, chunk-level retry with exponential backoff (1s/2s/4s capped at
//! 8s, 5 attempts, progress-bearing rounds are free), task-level auto-retry
//! (30s doubling capped at 300s, quota 1-20, reset by any confirmed
//! progress), token-bucket global speed limit, file preallocation, the
//! 206-vs-full-response guard, and pause/resume/cancel via generation
//! counting. Pure engine: no task DB; offsets persist through
//! [`OffsetTableStore`], which the database crate implements.
//!
//! Parallel *tasks* (1-10) are the task manager's concern (ticket 16): each
//! admitted task runs its own engine against the shared bucket.

mod bucket;
mod offset;
mod status;

pub use bucket::TokenBucketHandle;
pub use offset::{OffsetTable, OffsetTableStore};
pub use status::{DownloadError, TaskState, TaskStatus};

use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncSeekExt;
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;

/// Chunk size (Python `Downloader.chunk_size`).
pub const CHUNK_SIZE: u64 = 4 * 1024 * 1024;
/// Confirmed-flush granularity: a crash loses at most this many bytes per
/// chunk (Python `ChunkWorker.flush_interval`).
pub const FLUSH_INTERVAL: u64 = 1024 * 1024;
/// Chunk-level attempts before failing the task (Python `max_retries`).
pub const MAX_CHUNK_RETRIES: u32 = 5;
/// Upper bound on resume rounds even when every round makes progress
/// (Python `max_rounds = max_retries * 4`).
pub const MAX_ROUNDS: u32 = MAX_CHUNK_RETRIES * 4;
/// Chunk backoff cap (Python `min(2 ** (attempt-1), 8)`).
pub const CHUNK_BACKOFF_CAP: Duration = Duration::from_secs(8);
/// Task auto-retry cap (Python `min(30 * 2 ** n, 300)`).
pub const TASK_RETRY_CAP: u64 = 300;
/// Read/write timeout per ranged request (Python `timeout = 10`).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Poll slice for interruptible waits (Python 0.1s slices).
const SLICE: Duration = Duration::from_millis(100);

/// One file to download, from the playurl parse. `file_key` matches the
/// Python task snapshot keys (`video`, `audio`, `video_part_N`).
#[derive(Debug, Clone)]
pub struct FileSpec {
    pub file_key: String,
    pub url: String,
    pub file_size: u64,
    /// Part-file path on disk (naming engine + folder resolve it).
    pub path: PathBuf,
}

/// Engine configuration from the config store (ticket 02).
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// `Download.download_thread` (1-10): chunks downloading in parallel.
    pub parallel_chunks: usize,
    /// `Download.auto_retry_enabled`.
    pub auto_retry_enabled: bool,
    /// `Download.auto_retry_max_count` (1-20).
    pub auto_retry_max_count: u64,
    /// `Behavior.preallocate_file_space`.
    pub preallocate: bool,
    /// Backoff scale for chunk retries. Production keeps the default
    /// (1s/2s/4s, cap 8s); tests shrink it so retry suites stay fast.
    pub chunk_backoff_unit: Duration,
    /// Auto-retry delay scale. Production: 30s doubling capped at 300s;
    /// tests shrink it.
    pub task_retry_unit: Duration,
}

impl Default for EngineConfig {
    fn default() -> Self {
        // Defaults mirror config.py: download_thread 4, auto_retry_enabled
        // true, auto_retry_max_count 5, preallocate_file_space true.
        Self {
            parallel_chunks: 4,
            auto_retry_enabled: true,
            auto_retry_max_count: 5,
            preallocate: true,
            chunk_backoff_unit: Duration::from_secs(1),
            task_retry_unit: Duration::from_secs(30),
        }
    }
}

/// Cheap cloneable control surface over a running engine: pause/resume/
/// cancel from another task without owning the engine.
#[derive(Clone)]
pub struct EngineControl {
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    cancel_requested: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    state: Arc<Mutex<TaskState>>,
    bucket_rate: TokenBucketHandle,
    progress: Arc<Mutex<BTreeMap<String, FileProgress>>>,
}

impl EngineControl {
    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
        self.cancel_requested.store(true, Ordering::SeqCst);
        self.stop.store(true, Ordering::SeqCst);
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.state.lock().status = TaskStatus::Paused;
    }

    /// Observable state of the running engine.
    pub fn snapshot(&self) -> TaskState {
        self.state.lock().clone()
    }

    /// Persisted offset tables (for resuming into a fresh engine).
    pub fn offset_tables(&self) -> BTreeMap<String, OffsetTable> {
        self.progress
            .lock()
            .iter()
            .map(|(k, p)| (k.clone(), p.offsets.clone()))
            .collect()
    }

    /// Adjust the global speed limit (bytes/second; 0 = unlimited).
    pub fn set_speed_limit(&self, bytes_per_sec: f64) {
        self.bucket_rate.set_rate(bytes_per_sec);
    }
}

/// Engine events for observers (UI log viewer / task manager).
#[derive(Debug, Clone)]
pub enum EngineEvent {
    FileCompleted(String),
    DownloadCompleted,
    /// Task failed; `retry_in_secs` is 0 when terminal.
    Failed { retry_in_secs: u64, error: DownloadError },
    /// Chunk-level retry with its backoff (diagnostics).
    ChunkRetry { chunk: usize, backoff: Duration },
}

/// Per-file chunk plan and offset table.
#[derive(Debug, Default, Clone)]
struct FileProgress {
    offsets: OffsetTable,
    /// Chunk indices still unfinished, ascending.
    remaining: Vec<usize>,
    total_chunks: usize,
    finished_chunks: usize,
}

/// The chunked download engine for one task.
pub struct DownloadEngine {
    client: network::HttpClient,
    config: EngineConfig,
    store: Arc<dyn OffsetTableStore>,
    bucket: TokenBucketHandle,
    state: Arc<Mutex<TaskState>>,
    progress: Arc<Mutex<BTreeMap<String, FileProgress>>>,
    /// file_key -> (url, path, size)
    files: BTreeMap<String, FileSpec>,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    /// Set ONLY by pause()/cancel(); distinct from `stop`, which internal
    /// failure-unwinding also sets — run() must not misread a chunk
    /// failure as a user cancel.
    cancel_requested: Arc<AtomicBool>,
    /// Bumped on pause/cancel/failure; chunk tasks carry the generation they
    /// were spawned with and discard themselves when it goes stale
    /// (Python `download_generation`).
    generation: Arc<AtomicU64>,
    events: watch::Sender<Option<EngineEvent>>,
    chunk_retries: Arc<Mutex<Vec<(usize, Duration)>>>,
}

impl DownloadEngine {
    /// Build an engine for one task's file list. `bucket` is the global
    /// speed limiter (rate 0 = unlimited).
    pub fn new(
        client: network::HttpClient,
        config: EngineConfig,
        bucket: TokenBucketHandle,
        store: Arc<dyn OffsetTableStore>,
        files: Vec<FileSpec>,
    ) -> Self {
        let (events, _) = watch::channel(None);
        let stop = Arc::new(AtomicBool::new(false));
        let bucket = bucket.with_stop(stop.clone());
        let total_size = files.iter().map(|f| f.file_size).sum();
        let mut progress = BTreeMap::new();
        let mut queue = Vec::new();
        let mut file_map = BTreeMap::new();
        for f in files {
            let total_chunks = chunk_count(f.file_size);
            progress.insert(
                f.file_key.clone(),
                FileProgress {
                    offsets: OffsetTable::default(),
                    remaining: (0..total_chunks).collect(),
                    total_chunks,
                    finished_chunks: 0,
                },
            );
            queue.push(f.file_key.clone());
            file_map.insert(f.file_key.clone(), f);
        }
        let state = TaskState {
            total_size,
            queue,
            retry_max: config.auto_retry_max_count,
            ..Default::default()
        };
        Self {
            client,
            config,
            store,
            bucket,
            state: Arc::new(Mutex::new(state)),
            progress: Arc::new(Mutex::new(progress)),
            files: file_map,
            stop,
            paused: Arc::new(AtomicBool::new(false)),
            cancel_requested: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(0)),
            events,
            chunk_retries: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Seed resume state from persisted offset tables (task snapshot after
    /// a pause/crash/restart). Files absent from the snapshot restart from
    /// zero; chunks whose recorded offset already equals their capacity
    /// count as finished.
    pub fn resume_from(&mut self, persisted: BTreeMap<String, OffsetTable>) {
        let sizes: BTreeMap<String, u64> = self
            .files
            .values()
            .map(|f| (f.file_key.clone(), f.file_size))
            .collect();
        {
            let mut progress = self.progress.lock();
            for (key, table) in persisted {
                let Some(file) = progress.get_mut(&key) else {
                    continue;
                };
                let size = sizes.get(&key).copied().unwrap_or(0);
                file.offsets = table;
                file.remaining = (0..file.total_chunks)
                    .filter(|&i| {
                        let cap = chunk_capacity(i, file.total_chunks, size);
                        file.offsets.offset(i, cap) < cap
                    })
                    .collect();
                file.finished_chunks = file.total_chunks - file.remaining.len();
            }
            let queue: Vec<String> = progress
                .iter()
                .filter(|(_, p)| !p.remaining.is_empty())
                .map(|(k, _)| k.clone())
                .collect();
            self.state.lock().queue = queue;
        }
        self.recompute_downloaded();
    }

    /// Run to completion, pause, cancel or terminal failure. Resumes from
    /// offset tables seeded via [`Self::resume_from`]. Auto-retry loops
    /// internally (30s doubling, capped) while quota lasts.
    pub async fn run(&mut self) -> TaskState {

        let mut attempt: u64 = 0;
        let mut baseline = self.state.lock().downloaded_size;
        loop {
            // Pause/cancel requested before run() must still leave the
            // preallocated file on disk (sized placeholder semantics).
            for spec in self.files.values() {
                let _ = self.prepare_file(spec);
            }
            // pause()/cancel() called before run() (or on a control
            // handle) must not be clobbered by the start-of-pass reset.
            if self.cancel_requested.load(Ordering::SeqCst) {
                self.state.lock().status = TaskStatus::Paused;
                return self.snapshot();
            }
            self.paused.store(false, Ordering::SeqCst);
            self.stop.store(false, Ordering::SeqCst);
            self.state.lock().status = TaskStatus::Downloading;
            let gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;

            let r = self.run_once(gen).await;
            match r {
                Ok(()) => {
                    {
                        let mut s = self.state.lock();
                        s.status = TaskStatus::Completed;
                        s.downloaded_size = s.total_size;
                    }
                    let _ = self.events.send(Some(EngineEvent::DownloadCompleted));
                    return self.snapshot();
                }
                Err(reason) => {
                    if self.paused.load(Ordering::SeqCst) {
                        self.state.lock().status = TaskStatus::Paused;
                        return self.snapshot();
                    }
                    if self.cancel_requested.load(Ordering::SeqCst) {
                        // cancelled from outside: offsets stay persisted
                        let mut s = self.state.lock();
                        s.status = TaskStatus::Paused;
                        s.last_error = Some(reason.clone());
                        drop(s);
                        return self.snapshot();
                    }
                    // The chunk-failure unwind set `stop` to cull siblings;
                    // that is internal, not a user cancel. Clear it so the
                    // retry countdown and next pass run.
                    self.stop.store(false, Ordering::SeqCst);
                    let downloaded = self.state.lock().downloaded_size;
                    let wait = match self.schedule_auto_retry(&reason, downloaded, baseline, &mut attempt) {
                        Some(secs) => secs,
                        None => {
                            {
                                let mut s = self.state.lock();
                                s.status = TaskStatus::Failed;
                                s.last_error = Some(reason.clone());
                            }
                            let _ = self.events.send(Some(EngineEvent::Failed {
                                retry_in_secs: 0,
                                error: reason,
                            }));
                            return self.snapshot();
                        }
                    };
                    baseline = self.state.lock().downloaded_size;
                    let _ = self.events.send(Some(EngineEvent::Failed {
                        retry_in_secs: wait,
                        error: reason,
                    }));
                    if self.sleep_interruptible(self.config.task_retry_unit.mul_f64(wait as f64 / 30.0), &*self).await {
                        // paused/cancelled during the countdown: Python
                        // cancels the pending retry and stays FAILED/PAUSED
                        let mut s = self.state.lock();
                        s.retry_remaining_secs = 0;
                        s.retry_count = 0;
                        if self.paused.load(Ordering::SeqCst) {
                            s.status = TaskStatus::Paused;
                        }
                        drop(s);
                        return self.snapshot();
                    }
                    // Python requeues (QUEUED) and hands control to the
                    // scheduler; this engine owns its loop, so continue.
                    self.state.lock().status = TaskStatus::Queued;
                }
            }
        }
    }

    /// Python `_schedule_auto_retry`: Some(delay secs) or None (disabled /
    /// permanent / quota spent). Progress since the last schedule — in
    /// either direction — resets the quota (network recovered, or the task
    /// was reset and redownloads from zero).
    fn schedule_auto_retry(
        &self,
        reason: &DownloadError,
        downloaded: u64,
        baseline: u64,
        attempt: &mut u64,
    ) -> Option<u64> {
        let mut s = self.state.lock();
        if !reason.retryable() || !self.config.auto_retry_enabled {
            s.last_error = Some(reason.clone());
            return None;
        }
        if downloaded != baseline {
            *attempt = 0;
        }
        if *attempt >= self.config.auto_retry_max_count {
            s.last_error = Some(reason.clone());
            return None;
        }
        *attempt += 1;
        let delay = (30u64 << (*attempt - 1).min(4)).min(TASK_RETRY_CAP);
        s.retry_count = *attempt;
        s.retry_remaining_secs = delay;
        s.status = TaskStatus::Failed;
        s.last_error = Some(reason.clone());
        Some(delay)
    }

    /// One pass over the remaining queue; returns the first terminal error.
    async fn run_once(&mut self, gen: u64) -> Result<(), DownloadError> {
        loop {
            let key = match self.state.lock().queue.first().cloned() {
                Some(k) => k,
                None => return Ok(()),
            };
            let spec = self
                .files
                .get(&key)
                .expect("queue keys come from files")
                .clone();
            self.download_file(&spec, gen).await?;
            {
                let mut s = self.state.lock();
                if s.queue.first().is_some_and(|k| k == &key) {
                    s.queue.remove(0);
                }
            }
            self.recompute_downloaded();
            self.store.save(&key, &self.offset_table(&key));
            let _ = self.events.send(Some(EngineEvent::FileCompleted(key)));
            if self.paused_or_stopped() {
                return Err(DownloadError::Permanent("stopped".into()));
            }
        }
    }

    /// Pause: bump the generation so in-flight chunk tasks discard
    /// themselves; offsets of confirmed flushes stay persisted.
    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
        self.cancel_requested.store(true, Ordering::SeqCst);
        self.stop.store(true, Ordering::SeqCst);
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.state.lock().status = TaskStatus::Paused;
    }

    /// Cancel: same mechanics as pause; the caller drops the engine.
    /// Persisted offsets keep a future resume cheap.
    pub fn cancel(&self) {
        self.pause();
    }

    /// Shared control surface for callers that move the engine into a
    /// spawned `run()` (task manager, tests).
    pub fn control(&self) -> EngineControl {
        EngineControl {
            stop: self.stop.clone(),
            paused: self.paused.clone(),
            cancel_requested: self.cancel_requested.clone(),
            generation: self.generation.clone(),
            state: self.state.clone(),
            bucket_rate: self.bucket.clone(),
            progress: self.progress.clone(),
        }
    }

    /// Resume after a pause: continues from persisted offsets.
    pub async fn resume(mut self) -> TaskState {
        self.run().await
    }

    /// Observable task state.
    pub fn snapshot(&self) -> TaskState {
        let mut s = self.state.lock().clone();
        s.finished_chunks = self
            .progress
            .lock()
            .iter()
            .map(|(k, p)| (k.clone(), p.finished_chunks))
            .collect();
        s
    }

    /// All offset tables, for persistence by the caller.
    pub fn offset_tables(&self) -> BTreeMap<String, OffsetTable> {
        self.progress
            .lock()
            .iter()
            .map(|(k, p)| (k.clone(), p.offsets.clone()))
            .collect()
    }

    fn offset_table(&self, key: &str) -> OffsetTable {
        self.progress
            .lock()
            .get(key)
            .map(|p| p.offsets.clone())
            .unwrap_or_default()
    }

    fn paused_or_stopped(&self) -> bool {
        self.paused.load(Ordering::SeqCst) || self.stop.load(Ordering::SeqCst)
    }

    fn recompute_downloaded(&self) {
        let sizes: BTreeMap<String, u64> = self
            .files
            .values()
            .map(|f| (f.file_key.clone(), f.file_size))
            .collect();
        let progress = self.progress.lock();
        let total: u64 = progress.iter().map(|(key, file)| {
            let size = sizes.get(key).copied().unwrap_or(0);
            let total_chunks = file.total_chunks;
            file.offsets.total_written(|i| chunk_capacity(i, total_chunks, size))
        }).sum();
        drop(progress);
        let mut s = self.state.lock();
        s.downloaded_size = total;
        s.speed = 0;
    }

    async fn sleep_interruptible(&self, mut total: Duration, _: &DownloadEngine) -> bool {
        while !total.is_zero() {
            if self.paused_or_stopped() {
                return true;
            }
            let slice = total.min(SLICE);
            tokio::time::sleep(slice).await;
            total -= slice;
        }
        false
    }

    /// Download all remaining chunks of one file, `parallel_chunks` at a
    /// time. Preallocates (or creates a placeholder for) the file first.
    async fn download_file(&mut self, spec: &FileSpec, gen: u64) -> Result<(), DownloadError> {
        self.prepare_file(spec)?;

        let chunks: Vec<usize> = self
            .progress
            .lock()
            .get(&spec.file_key)
            .map(|p| p.remaining.clone())
            .unwrap_or_default();
        if chunks.is_empty() {
            return Ok(());
        }

        self.spawn_chunks(spec, chunks, gen).await
    }

    /// Create the parent directory and the target file (preallocated to
    /// full size when configured), resetting stale offsets if the file
    /// vanished since the last run.
    fn prepare_file(&self, spec: &FileSpec) -> Result<(), DownloadError> {
        if let Some(parent) = spec.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                DownloadError::Permanent(format!("cannot create directory: {e}"))
            })?;
        }
        // A vanished file invalidates every recorded offset: without this
        // check the engine would "finish" chunks into a file of holes.
        if !spec.path.exists() {
            if self.config.preallocate && spec.file_size > 0 {
                preallocate(&spec.path, spec.file_size).map_err(|e| {
                    DownloadError::Permanent(format!("preallocation failed: {e}"))
                })?;
            } else {
                std::fs::File::create(&spec.path).map_err(|e| {
                    DownloadError::Permanent(format!("cannot create file: {e}"))
                })?;
            }
            self.reset_file_progress(&spec.file_key);
        }
        Ok(())
    }

    async fn spawn_chunks(&self, spec: &FileSpec, chunks: Vec<usize>, gen: u64) -> Result<(), DownloadError> {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(
            self.config.parallel_chunks.clamp(1, 10),
        ));
        let mut set = tokio::task::JoinSet::new();
        for chunk in chunks {
            if self.paused_or_stopped() || !self.generation_is(gen) {
                break;
            }
            let task = self.chunk_task(spec, chunk, gen);
            let permits = semaphore.clone();
            set.spawn(async move {
                let _permit = permits.acquire_owned().await;
                task.run().await
            });
        }
        let mut first_err: Option<DownloadError> = None;
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    if first_err.is_none() && !self.paused_or_stopped() {
                        first_err = Some(e);
                        // sibling chunks unwind quickly on the stop flag
                        self.stop.store(true, Ordering::SeqCst);
                    }
                }
                Err(join_err) => {
                    if first_err.is_none() {
                        first_err = Some(DownloadError::Retryable(format!(
                            "chunk task panicked: {join_err}"
                        )));
                    }
                }
            }
        }
        match first_err {
            None => Ok(()),
            Some(e) => {
                self.generation.fetch_add(1, Ordering::SeqCst);
                Err(e)
            }
        }
    }

    fn reset_file_progress(&self, key: &str) {
        let mut progress = self.progress.lock();
        if let Some(file) = progress.get_mut(key) {
            let total = file.total_chunks;
            file.offsets = OffsetTable::default();
            file.remaining = (0..total).collect();
            file.finished_chunks = 0;
        }
    }

    fn generation_is(&self, gen: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == gen
    }

    fn chunk_task(&self, spec: &FileSpec, chunk: usize, gen: u64) -> ChunkTask {
        let total_chunks = chunk_count(spec.file_size);
        ChunkTask {
            client: self.client.clone(),
            url: spec.url.clone(),
            path: spec.path.clone(),
            chunk,
            chunk_size: chunk_capacity(chunk, total_chunks, spec.file_size),
            chunk_start: chunk as u64 * CHUNK_SIZE,
            file_key: spec.file_key.clone(),
            progress: self.progress.clone(),
            state: self.state.clone(),
            store: self.store.clone(),
            bucket: self.bucket.clone(),
            stop: self.stop.clone(),
            backoff_unit: self.config.chunk_backoff_unit,
            generation: self.generation.clone(),
            gen,
            retries: self.chunk_retries.clone(),
            events: self.events.clone(),
        }
    }

    /// Chunk-retry (index, backoff) samples from the last run.
    pub fn chunk_retry_log(&self) -> Vec<(usize, Duration)> {
        self.chunk_retries.lock().clone()
    }

    /// Subscribe to engine events.
    pub fn events(&self) -> watch::Receiver<Option<EngineEvent>> {
        self.events.subscribe()
    }
}



/// Everything one chunk task owns; spawned onto the runtime.
struct ChunkTask {
    client: network::HttpClient,
    url: String,
    path: PathBuf,
    chunk: usize,
    chunk_size: u64,
    chunk_start: u64,
    file_key: String,
    progress: Arc<Mutex<BTreeMap<String, FileProgress>>>,
    state: Arc<Mutex<TaskState>>,
    store: Arc<dyn OffsetTableStore>,
    bucket: TokenBucketHandle,
    stop: Arc<AtomicBool>,
    backoff_unit: Duration,
    generation: Arc<AtomicU64>,
    gen: u64,
    retries: Arc<Mutex<Vec<(usize, Duration)>>>,
    events: watch::Sender<Option<EngineEvent>>,
}

impl ChunkTask {
    fn active(&self) -> bool {
        !self.stop.load(Ordering::SeqCst) && self.generation.load(Ordering::SeqCst) == self.gen
    }

    fn load_offset(&self) -> u64 {
        self.progress
            .lock()
            .get(&self.file_key)
            .map(|p| p.offsets.offset(self.chunk, self.chunk_size))
            .unwrap_or(0)
    }

    /// Persist a new confirmed offset for this chunk (Python
    /// `_commit_offset`: only called after a successful flush, so the
    /// recorded value never exceeds what is really on disk).
    fn commit_offset(&self, written: u64) {
        let table = {
            let mut progress = self.progress.lock();
            match progress.get_mut(&self.file_key) {
                Some(file) => {
                    file.offsets.commit(self.chunk, written);
                    file.offsets.clone()
                }
                None => return,
            }
        };
        self.store.save(&self.file_key, &table);
    }

    async fn run(mut self) -> Result<(), DownloadError> {
        let mut written = self.load_offset();
        let mut attempt: u32 = 0;
        let mut rounds: u32 = 0;

        loop {
            if !self.active() {
                return Ok(());
            }
            if written >= self.chunk_size {
                self.finish_chunk();
                return Ok(());
            }
            if attempt >= MAX_CHUNK_RETRIES || rounds >= MAX_ROUNDS {
                break;
            }
            rounds += 1;
            let written_before = written;
            match self.transfer_round(written).await {
                Ok(advanced) => {
                    written += advanced;
                    // Not full yet: the server gave less than asked. The
                    // offset advanced, so the next round resumes from there
                    // without consuming a retry (Python behavior).
                }
                Err(RoundError::Guard(msg)) => {
                    // Non-206 full response: this chunk's round is void.
                    // Bytes already flushed at the chunk offset this run
                    // are suspect (the server may have been serving the
                    // whole file earlier too), so the chunk restarts from
                    // its last trusted offset on the task-level retry.
                    return Err(DownloadError::Retryable(msg));
                }
                Err(RoundError::Fatal(e)) => return Err(e),
                Err(RoundError::Retryable(e)) => {
                    if written > written_before {
                        continue;
                    }
                    attempt += 1;
                    if attempt >= MAX_CHUNK_RETRIES {
                        return Err(DownloadError::Retryable(format!(
                            "chunk {} failed after {attempt} attempts: {}",
                            self.chunk + 1,
                            e.message()
                        )));
                    }
                    let backoff =
                        CHUNK_BACKOFF_CAP.min(self.backoff_unit * (1u32 << (attempt - 1).min(3)));
                    self.retries.lock().push((self.chunk, backoff));
                    let _ = self.events.send(Some(EngineEvent::ChunkRetry {
                        chunk: self.chunk,
                        backoff,
                    }));
                    if self.sleep_interruptible(backoff).await {
                        return Ok(());
                    }
                }
            }
        }

        if written >= self.chunk_size {
            self.finish_chunk();
            return Ok(());
        }
        Err(DownloadError::Permanent(format!(
            "chunk {} incomplete after {rounds} rounds ({written}/{} bytes)",
            self.chunk + 1,
            self.chunk_size
        )))
    }

    /// One ranged request → streamed write loop. Returns the bytes
    /// confirmed flushed during this round.
    async fn transfer_round(&mut self, written: u64) -> Result<u64, RoundError> {
        let range = format!(
            "bytes={}-{}",
            self.chunk_start + written,
            self.chunk_start + self.chunk_size - 1
        );
        let response = self
            .client
            .inner()
            .get(&self.url)
            .header("Range", &range)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|e| RoundError::Retryable(DownloadError::Retryable(e.to_string())))?;

        let status = response.status();
        if status.is_client_error() || status.is_server_error() {
            // Python: 400/401/403/404/405/410/416 permanent; 408/429 and
            // any 5xx retryable; other 4xx treated by default as
            // non-retryable at the chunk level.
            let code = status.as_u16();
            let err = if matches!(code, 400 | 401 | 403 | 404 | 405 | 410 | 416) {
                DownloadError::Permanent(format!("HTTP {code}"))
            } else if code >= 500 || matches!(code, 408 | 429) {
                DownloadError::Retryable(format!("HTTP {code}"))
            } else {
                DownloadError::Permanent(format!("HTTP {code}"))
            };
            return Err(match err {
                DownloadError::Retryable(e) => RoundError::Retryable(DownloadError::Retryable(e)),
                DownloadError::Permanent(e) => RoundError::Fatal(DownloadError::Permanent(e)),
            });
        }
        if status.as_u16() != 206 {
            // Server ignored Range; the body is the whole file. Writing it
            // at this chunk's offset would clobber sibling chunks that may
            // be writing concurrently — discard the round, never write.
            drop(response);
            return Err(RoundError::Guard(format!(
                "server ignored Range (HTTP {}), round discarded",
                status.as_u16()
            )));
        }

        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .open(&self.path)
            .await
            .map_err(|e| RoundError::Fatal(DownloadError::Permanent(format!("open failed: {e}"))))?;
        file.seek(std::io::SeekFrom::Start(self.chunk_start + written))
            .await
            .map_err(|e| RoundError::Fatal(DownloadError::Permanent(format!("seek failed: {e}"))))?;

        let mut confirmed = written;
        let mut pending: u64 = 0;
        let mut stream = response.bytes_stream();
        use futures_util::StreamExt;
        while let Some(item) = stream.next().await {
            if !self.active() {
                break;
            }
            let mut bytes = item
                .map_err(|e| RoundError::Retryable(DownloadError::Retryable(e.to_string())))?;
            let allowed = (self.chunk_size - confirmed - pending) as usize;
            if allowed == 0 {
                break;
            }
            if bytes.len() > allowed {
                bytes = bytes.slice(0..allowed);
            }
            let n = bytes.len() as u64;
            self.bucket.consume(n).await;
            if !self.active() {
                break;
            }
            file.write_all(&bytes).await.map_err(|e| {
                RoundError::Fatal(DownloadError::Permanent(format!("write failed: {e}")))
            })?;
            pending += n;
            self.state.lock().downloaded_size += n;
            if pending >= FLUSH_INTERVAL {
                file.flush().await.map_err(|e| {
                    RoundError::Fatal(DownloadError::Permanent(format!("flush failed: {e}")))
                })?;
                confirmed += pending;
                pending = 0;
                self.commit_offset(confirmed);
            }
        }
        if pending > 0 {
            file.flush().await.map_err(|e| {
                RoundError::Fatal(DownloadError::Permanent(format!("flush failed: {e}")))
            })?;
            confirmed += pending;
            self.commit_offset(confirmed);
        }
        Ok(confirmed - written)
    }

    fn finish_chunk(&self) {
        let table = {
            let mut progress = self.progress.lock();
            match progress.get_mut(&self.file_key) {
                Some(file) => {
                    file.offsets.commit(self.chunk, self.chunk_size);
                    file.finished_chunks += 1;
                    file.remaining.retain(|&i| i != self.chunk);
                    file.offsets.clone()
                }
                None => return,
            }
        };
        self.store.save(&self.file_key, &table);
    }

    async fn sleep_interruptible(&self, mut dur: Duration) -> bool {
        while !dur.is_zero() {
            if !self.active() {
                return true;
            }
            let slice = dur.min(SLICE);
            tokio::time::sleep(slice).await;
            dur -= slice;
        }
        false
    }
}

enum RoundError {
    Guard(String),
    Retryable(DownloadError),
    Fatal(DownloadError),
}

/// Chunk count for a file size (Python: `ceil(size/chunk)`, min 1).
pub fn chunk_count(file_size: u64) -> usize {
    if file_size == 0 {
        1
    } else {
        file_size.div_ceil(CHUNK_SIZE) as usize
    }
}

/// Size of chunk `index` (Python `calc_chunk_range`).
pub fn chunk_capacity(index: usize, total_chunks: usize, file_size: u64) -> u64 {
    let _ = total_chunks;
    let start = index as u64 * CHUNK_SIZE;
    if file_size == 0 {
        0
    } else {
        (start + CHUNK_SIZE).min(file_size) - start
    }
}

/// Preallocate `size` bytes (Python `File.preallocate_file`: seek to
/// size-1, write one zero byte).
fn preallocate(path: &Path, size: u64) -> std::io::Result<()> {
    use std::io::{Seek, Write};
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    f.seek(std::io::SeekFrom::Start(size - 1))?;
    f.write_all(&[0u8])?;
    f.flush()
}
