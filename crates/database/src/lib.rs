//! Drop-in SQLite persistence for the three databases the Python build keeps
//! in the app data dir: `task.db`, `thumbnail.db`, `history.db`.
//!
//! Schemas, column names, WAL mode and `PRAGMA user_version` rehash
//! migration semantics are identical to the Python build, so both versions
//! read and write the same files in place. (spec: user-data compatibility,
//! ticket 03)

pub mod hash_id;
pub mod info;
pub mod store;

pub use info::TaskInfo;
pub use store::{CoverDatabase, HistoryDatabase, HistoryEntry, TaskDatabase, TaskRecord};
