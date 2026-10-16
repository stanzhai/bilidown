//! Manager-level status/type codes (Python `DownloadStatus` /
//! `DownloadType` / `DuplicateDownloadResolution` / `NumberingType`).
//! Mux-tail codes (4/5/6/7/101) mirror `mux::download_status`; the
//! manager persists the full Python set on `TaskInfo.Download.status`.

/// `DownloadStatus` values persisted in the task DB.
pub mod download_status {
    pub const QUEUED: i64 = 0;
    pub const PARSING: i64 = 1;
    pub const DOWNLOADING: i64 = 2;
    pub const PAUSED: i64 = 3;
    pub const COMPLETED: i64 = 4;
    pub const FFMPEG_QUEUED: i64 = 5;
    pub const MERGING: i64 = 6;
    pub const CONVERTING: i64 = 7;
    pub const ADDITIONAL_PROCESSING: i64 = 8;
    pub const FAILED: i64 = 100;
    pub const FFMPEG_FAILED: i64 = 101;
}

/// `DownloadType` bits deciding which files get delivered.
pub mod download_type {
    pub const VIDEO: i64 = 1 << 0;
    pub const AUDIO: i64 = 1 << 1;
    pub const DANMAKU: i64 = 1 << 2;
    pub const SUBTITLE: i64 = 1 << 3;
    pub const COVER: i64 = 1 << 4;
    pub const METADATA: i64 = 1 << 5;
    pub const CHAPTER: i64 = 1 << 6;
}

/// `DuplicateDownloadResolution`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicateResolution {
    Continue = 0,
    Skip = 1,
    AlwaysAsk = 2,
}

impl DuplicateResolution {
    pub fn from_code(code: i64) -> Self {
        match code {
            0 => Self::Continue,
            1 => Self::Skip,
            _ => Self::AlwaysAsk,
        }
    }
}

/// `NumberingType` (`File Naming.numbering_type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberingType {
    FromSpecified = 0,
    UseParseList = 1,
    Continuous = 2,
}

impl NumberingType {
    pub fn from_code(code: i64) -> Self {
        match code {
            0 => Self::FromSpecified,
            1 => Self::UseParseList,
            _ => Self::Continuous,
        }
    }
}
