//! Download Task manager (Python `util/download/task/*` + the download
//! list's concurrent-download scheduler). Owns the task lifecycle
//! create → run → complete, freezing Task Info at creation, duplicate
//! detection across both task tables, a single-threaded DB writer with
//! latest-wins progress coalescing, and admission of queued tasks into
//! 1-10 parallel engine slots.

pub mod duplicate;
pub mod info;
pub mod manager;
pub mod numbering;
pub mod options;
pub mod scheduler;
pub mod status;
pub use scheduler::Scheduler;
pub use manager::{TaskManager, MANAGED_DOWNLOAD_PARALLEL_MAX, MANAGED_DOWNLOAD_PARALLEL_MIN};
