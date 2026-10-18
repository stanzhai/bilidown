//! Log viewer dialog state (port of `src/gui/dialog/log.py`): records
//! loaded from `logs/app.log`, level + text filter, refresh/clear/open-dir.
//! Parsing and filtering live in `services::log`; this is the UI state.

use services::log::{LevelFilter, LogFilter, LogRecord};
use std::path::{Path, PathBuf};

/// `appdata/bilidown/logs/app.log` (Python QStandardPaths
/// AppDataLocation derivation).
pub fn default_log_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(config::APP_NAME).join("logs").join("app.log"))
}

pub struct LogViewModel {
    pub path: PathBuf,
    pub records: Vec<LogRecord>,
    pub level: LevelFilter,
    pub text: String,
    pub error: Option<String>,
}

impl LogViewModel {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            records: Vec::new(),
            level: LevelFilter::All,
            text: String::new(),
            error: None,
        }
    }

    /// Re-read the file (`init` + Refresh).
    pub fn refresh(&mut self) {
        match LogRecord::parse_file(&self.path) {
            Ok(records) => {
                self.records = records;
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// Filtered view for the list (log.py level combo + search box).
    pub fn visible(&self) -> Vec<&LogRecord> {
        LogFilter {
            level: self.level,
            text: self.text.clone(),
        }
        .apply(&self.records)
    }

    /// Clear Logs: truncate the file and drop the model rows.
    pub fn clear(&mut self) {
        self.error = std::fs::write(&self.path, "").err().map(|e| e.to_string());
        if self.error.is_none() {
            self.records.clear();
        }
    }
}

/// Open the log directory in the platform file manager
/// (`Directory.open_directory_in_explorer`).
pub fn open_logs_directory(dir: &Path) -> std::io::Result<()> {
    let dir = dir.to_string_lossy().into_owned();
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("explorer").arg(&dir).spawn();
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open").arg(&dir).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let status = std::process::Command::new("xdg-open").arg(&dir).spawn();
    match status {
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn refresh_parses_and_filters() {
        let dir = log_dir();
        let path = dir.path().join("app.log");
        std::fs::write(
            &path,
            "[2026-01-02 03:04:05.123456] - app - INFO - at main.py:1 in f: hello\n\
             [2026-01-02 03:04:06.123456] - app - ERROR - at main.py:2 in g: boom\n",
        )
        .unwrap();
        let mut m = LogViewModel::new(&path);
        m.refresh();
        assert_eq!(m.records.len(), 2);
        assert!(m.error.is_none());

        m.level = LevelFilter::Error;
        assert_eq!(m.visible().len(), 1);
        assert_eq!(m.visible()[0].message, "boom");

        m.level = LevelFilter::All;
        m.text = "hello".into();
        assert_eq!(m.visible().len(), 1);
        assert_eq!(m.visible()[0].level, "INFO");

        m.text = "HELLO".into();
        assert_eq!(m.visible().len(), 1, "casefolded search");
    }

    #[test]
    fn clear_truncates_file_and_rows() {
        let dir = log_dir();
        let path = dir.path().join("app.log");
        std::fs::write(
            &path,
            "[2026-01-02 03:04:05.123456] - app - INFO - at main.py:1 in f: hello\n",
        )
        .unwrap();
        let mut m = LogViewModel::new(&path);
        m.refresh();
        assert!(!m.records.is_empty());
        m.clear();
        assert!(m.records.is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
    }

    #[test]
    fn missing_file_records_error_not_panic() {
        let dir = log_dir();
        let mut m = LogViewModel::new(dir.path().join("nope.log"));
        m.refresh();
        assert!(m.error.is_some());
        assert!(m.records.is_empty());
    }
}
