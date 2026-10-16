//! The `_write` helper of `AdditionalParserBase` (`additional/base.py`):
//! builds the sidecar path, writes text or bytes, updates the running size
//! counters and returns the bare file name.

use std::fs;
use std::path::PathBuf;

use database::TaskInfo;

/// Writes `contents` to `{download_path}/{folder}/{name}[.{qualifier}].{suffix}`
/// and returns the file name (no directory).
pub fn write(
    task: &mut TaskInfo,
    contents: Contents<'_>,
    suffix: &str,
    name: Option<&str>,
    qualifier: &[&str],
) -> anyhow::Result<String> {
    let base_name = name.unwrap_or(&task.File.name).to_owned();
    let name_parts = if qualifier.is_empty() {
        base_name
    } else {
        format!("{}.{}", base_name, qualifier.join("."))
    };
    let path = PathBuf::from(&task.File.download_path)
        .join(&task.File.folder)
        .join(format!("{name_parts}.{suffix}"));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let size = match contents {
        Contents::Text(s) => {
            fs::write(&path, s)?;
            s.len() as u64
        }
        Contents::Bytes(b) => {
            fs::write(&path, b)?;
            b.len() as u64
        }
    };
    task.Download.downloaded_size += size as i64;
    task.Download.total_size += size as i64;
    Ok(path.file_name().unwrap().to_string_lossy().into_owned())
}

/// Text is written as UTF-8, bytes verbatim — Python's `str`/`bytes` split.
pub enum Contents<'a> {
    Text(&'a str),
    Bytes(&'a [u8]),
}

/// `is_embed_available`: ASS subtitle tracks fit only MKV outputs, and
/// `merge_file_ext` is only set when a merge step exists, so this one
/// comparison covers both preconditions.
pub fn is_embed_available(task: &TaskInfo) -> bool {
    task.File.merge_file_ext == "mkv"
}
