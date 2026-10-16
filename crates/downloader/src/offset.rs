//! Per-chunk offset table persisted as a JSON blob (Python
//! `TaskInfo.Download.files[file_key]` shape: `chunk_offsets` keyed by chunk
//! index as a string). The database crate later implements [`OffsetTableStore`]
//! over the task JSON; the engine only needs the two methods.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Chunk index -> confirmed-on-disk bytes for that chunk.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OffsetTable(BTreeMap<String, u64>);

impl OffsetTable {
    /// Confirmed bytes for `index`, clamped to the chunk size (Python
    /// `_load_offset`: `max(min(offset, chunk_size), 0)`).
    pub fn offset(&self, index: usize, chunk_size: u64) -> u64 {
        self.0
            .get(&index.to_string())
            .cloned()
            .unwrap_or(0)
            .min(chunk_size)
    }

    /// Record the new confirmed offset for `index` (Python `_commit_offset`).
    pub fn commit(&mut self, index: usize, written: u64) {
        self.0.insert(index.to_string(), written);
    }

    /// Sum of recorded bytes, each clamped to its chunk capacity (Python
    /// `_verify_file_complete` sums `max(min(offset, end - start), 0)`).
    /// `capacity(i)` returns the size of chunk `i`.
    pub fn total_written(&self, capacity: impl Fn(usize) -> u64) -> u64 {
        self.0
            .iter()
            .filter_map(|(k, v)| k.parse::<usize>().ok().map(|i| (i, *v)))
            .map(|(i, v)| v.min(capacity(i)))
            .sum()
    }

    /// Serialize to the JSON blob shape persisted in the task snapshot.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("offset table is plain data")
    }

    /// Parse from a persisted JSON blob (string keys, per Python).
    pub fn from_json(value: serde_json::Value) -> Self {
        serde_json::from_value(value).unwrap_or_default()
    }
}

/// Persistence seam: the database crate implements this to persist the
/// offset table as part of the task JSON blob. The engine never touches
/// the task DB (ticket 13 scope: pure engine).
pub trait OffsetTableStore: Send + Sync {
    /// Persist the table for `file_key`. Called after every confirmed flush;
    /// must not block download workers for long (implementations typically
    /// hand off to a writer thread).
    fn save(&self, file_key: &str, table: &OffsetTable);
}

/// No-op store for tests and callers without persistence yet.
impl OffsetTableStore for () {
    fn save(&self, _file_key: &str, _table: &OffsetTable) {}
}
