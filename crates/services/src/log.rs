//! Log viewer backend: parses `logs/app.log` lines into records and
//! filters them the way `src/gui/dialog/log.py` + `log_list/proxy_model.py`
//! do.
//!
//! Line format (CompactLogFormatter in `src/main.py`):
//! `[{asctime}] - {name} - {levelname} - at {callsite}: {message}` with
//! asctime `%Y-%m-%d %H:%M:%S.%f` and callsite `{filename}:{lineno} in {funcName}`.
//! Continuation lines (multi-line messages, tracebacks) match nothing and
//! append to the previous record with `\n` — the LOG_PATTERN in
//! `src/gui/dialog/log.py` is deliberately loose, so we copy it verbatim
//! (`.+?` name, `\w+` level) instead of guessing a stricter shape.

use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

static RECORD_RE: LazyLock<Regex> = LazyLock::new(|| {
    // Verbatim port of Python LOG_PATTERN (re.match → anchored at start).
    Regex::new(
        r"^\[(?P<time>.*)\] - (?P<name>.+?) - (?P<level>\w+) - at (?P<callsite>.+?): (?P<msg>.*)$",
    )
    .unwrap()
});

/// One parsed log record.
pub struct LogRecord {
    /// `2026-01-02 03:04:05.123456` (verbatim from the line).
    pub timestamp: String,
    pub name: String,
    /// `DEBUG` / `INFO` / `WARNING` / `ERROR`.
    pub level: String,
    /// `file.py:123 in func` (verbatim).
    pub callsite: String,
    /// Message; multi-line messages keep their `\n`.
    pub message: String,
}

impl LogRecord {
    /// Parse `logs/app.log` content (any `\n` line ending). The first line
    /// must be a record header; anything before it is dropped, matching
    /// Python's behavior of ignoring garbage at the top of the file.
    pub fn parse_all(content: &str) -> Vec<LogRecord> {
        let mut out: Vec<LogRecord> = Vec::new();
        for line in content.lines() {
            if let Some(caps) = RECORD_RE.captures(line) {
                out.push(LogRecord {
                    timestamp: caps["time"].to_owned(),
                    name: caps["name"].to_owned(),
                    level: caps["level"].to_owned(),
                    callsite: caps["callsite"].to_owned(),
                    message: caps["msg"].to_owned(),
                });
            } else if let Some(last) = out.last_mut() {
                last.message.push('\n');
                last.message.push_str(line);
            }
        }
        out
    }

    /// Parse a log file from disk.
    pub fn parse_file(path: &Path) -> std::io::Result<Vec<LogRecord>> {
        let content = std::fs::read_to_string(path)?;
        Ok(Self::parse_all(&content))
    }
}

/// Log level filter (`LogViewerDialog` combo: `ALL, DEBUG, INFO, WARNING, ERROR`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LevelFilter {
    #[default]
    All,
    Debug,
    Info,
    Warning,
    Error,
}

impl LevelFilter {
    /// Exact uppercase match on the record level — the proxy model compares
    /// `record.level_name == filter`, so `INFO` does not match `ERROR`.
    pub fn matches(self, record: &LogRecord) -> bool {
        let want = match self {
            Self::All => return true,
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warning => "WARNING",
            Self::Error => "ERROR",
        };
        record.level == want
    }
}

/// Log viewer filter: level + free text. The text filter casefolds a
/// substring over `timestamp name level message` joined by spaces — NOT the
/// callsite (proxy_model checks `record.timestamp/level_name/message/name`).
#[derive(Debug, Clone, Default)]
pub struct LogFilter {
    pub level: LevelFilter,
    pub text: String,
}

impl LogFilter {
    pub fn matches(&self, record: &LogRecord) -> bool {
        if !self.level.matches(record) {
            return false;
        }
        if self.text.is_empty() {
            return true;
        }
        let haystack = format!(
            "{} {} {} {}",
            record.timestamp, record.name, record.level, record.message
        );
        haystack.to_lowercase().contains(&self.text.to_lowercase())
    }

    pub fn apply<'a>(&self, records: &'a [LogRecord]) -> Vec<&'a LogRecord> {
        records.iter().filter(|r| self.matches(r)).collect()
    }
}
