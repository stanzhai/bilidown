//! Task download status, mirroring the Python `DownloadStatus` values that
//! the chunked engine itself transitions between. Later statuses (merge,
//! conversion, additional processing) belong to the muxing/task-manager
//! tickets; the engine stops at `Completed`.

/// Engine-observable task states (subset of Python `DownloadStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskStatus {
    /// Waiting for a parallel slot (auto-retry requeue lands here).
    #[default]
    Queued,
    Downloading,
    Paused,
    Completed,
    /// Download failed; if an auto-retry is scheduled the task stays here
    /// while counting down (Python keeps FAILED during the countdown so the
    /// scheduler does not pick it up early).
    Failed,
}

impl TaskStatus {
    /// Integer code matching `DownloadStatus` (task DB persistence).
    pub fn code(self) -> i64 {
        match self {
            TaskStatus::Queued => 0,
            TaskStatus::Downloading => 2,
            TaskStatus::Paused => 3,
            TaskStatus::Completed => 4,
            TaskStatus::Failed => 100,
        }
    }

    pub fn from_code(code: i64) -> Option<Self> {
        Some(match code {
            0 => TaskStatus::Queued,
            2 => TaskStatus::Downloading,
            3 => TaskStatus::Paused,
            4 => TaskStatus::Completed,
            100 => TaskStatus::Failed,
            _ => return None,
        })
    }
}

/// Snapshot of engine-observable task state (what a UI/task-manager reads).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TaskState {
    pub status: TaskStatus,
    /// Confirmed bytes across all files (sum of offset tables for pending
    /// chunks plus full chunk sizes for finished ones).
    pub downloaded_size: u64,
    /// Sum of all file sizes, known up front from the playurl parse.
    pub total_size: u64,
    /// Bytes/second over the last measurement window.
    pub speed: u64,
    /// Retry countdown in seconds (0 = none pending); mirrors the Python
    /// `AUTO_RETRY_PENDING` label contents.
    pub retry_remaining_secs: u64,
    /// 1-based count of the pending auto retry.
    pub retry_count: u64,
    /// Total configured auto-retry quota.
    pub retry_max: u64,
    /// File keys still to download, in order.
    pub queue: Vec<String>,
    /// Per-file finished-chunk counts, for progress display.
    pub finished_chunks: std::collections::BTreeMap<String, usize>,
    /// Last chunk-level failure that reached the task level, with its
    /// retryability (drives auto-retry decisions).
    pub last_error: Option<DownloadError>,
}

impl TaskState {
    /// Progress percentage 0-100 (Python `progress` field).
    pub fn progress(&self) -> u64 {
        (self.downloaded_size * 100)
            .checked_div(self.total_size)
            .map_or(0, |p| p.min(100))
    }
}

/// Chunk/task failure classification (Python `_is_retryable_exception` +
/// the `retryable` flag carried into `on_download_error`).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DownloadError {
    #[error("{0}")]
    Retryable(String),
    #[error("{0}")]
    Permanent(String),
}

impl DownloadError {
    pub fn retryable(&self) -> bool {
        matches!(self, DownloadError::Retryable(_))
    }

    pub fn message(&self) -> &str {
        match self {
            DownloadError::Retryable(m) | DownloadError::Permanent(m) => m,
        }
    }
}
