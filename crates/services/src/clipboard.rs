//! Clipboard monitor: debounced URL detection for auto-parse, ported from
//! `src/gui/interface/parse.py`.
//!
//! The Python build hooks `QApplication.clipboard().dataChanged`, checks
//! whether the text matches a parseable URL, and re-parses only if the URL
//! changed or the last parse of it was more than
//! [`CLIPBOARD_DEBOUNCE_S`] ago (monotonic clock). Gated on
//! `Behavior.monitor_clipboard` and suppressed while a download-options
//! dialog is open. `check_clipboard_on_startup()` runs the same path once
//! at startup to cover a URL copied before launch.
//!
//! The clipboard source and clock are injectable so tests run headless.

use config::ConfigStore;

/// Same-URL re-parse suppression window (seconds).
pub const CLIPBOARD_DEBOUNCE_S: f64 = 1.5;

/// Result of one clipboard check: what the caller should parse, if anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardDecision {
    /// Parse this URL (it matched a parser pattern and passed debounce).
    Parse(String),
    /// Nothing to do (no text / not a parseable URL / gated / debounced).
    Skip,
}

/// Where clipboard text comes from; the app injects the OS clipboard, tests
/// inject a canned value.
pub trait ClipboardSource {
    /// Current clipboard text; `None` when empty or non-text.
    fn text(&mut self) -> Option<String>;
}

/// Monotonic clock in seconds (QElapsedTimer.elapsed() / 1000.0).
pub trait Clock {
    fn now_secs(&self) -> f64;
}

/// A clipboard source that always reports the same text.
pub struct FixedClipboard(Option<String>);
impl FixedClipboard {
    pub fn new(text: Option<&str>) -> Self {
        Self(text.map(str::to_owned))
    }
}
impl ClipboardSource for FixedClipboard {
    fn text(&mut self) -> Option<String> {
        self.0.clone()
    }
}

/// A clock the test can advance by hand.
#[derive(Default)]
pub struct ManualClock {
    pub secs: f64,
}
impl Clock for ManualClock {
    fn now_secs(&self) -> f64 {
        self.secs
    }
}

/// The real system clock (`Instant`-based, monotonic).
pub struct SystemClock(std::time::Instant);
impl SystemClock {
    pub fn new() -> Self {
        Self(std::time::Instant::now())
    }
}
impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}
impl Clock for SystemClock {
    fn now_secs(&self) -> f64 {
        self.0.elapsed().as_secs_f64()
    }
}

/// Clipboard auto-parse state machine. `options_dialog_open` mirrors the
/// Python guard and is set by the UI layer while a download-options dialog
/// is up.
pub struct ClipboardMonitor {
    last_url: Option<String>,
    last_parse_at: Option<f64>,
    /// Set while a download-options dialog is open (suppresses parsing).
    pub options_dialog_open: bool,
}

impl Default for ClipboardMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardMonitor {
    pub fn new() -> Self {
        Self {
            last_url: None,
            last_parse_at: None,
            options_dialog_open: false,
        }
    }

    /// `on_clipboard_changed` / `check_clipboard_on_startup` — the shared
    /// decision path. Reads the clipboard once, applies the config gate and
    /// the debounce window, and records state when a parse is requested.
    pub fn check<S: ClipboardSource, C: Clock>(
        &mut self,
        cfg: &ConfigStore,
        source: &mut S,
        clock: &C,
    ) -> ClipboardDecision {
        if !monitor_enabled(cfg) || self.options_dialog_open {
            return ClipboardDecision::Skip;
        }
        let Some(text) = source.text() else {
            return ClipboardDecision::Skip;
        };
        let Some(url) = extract_parseable_url(&text) else {
            return ClipboardDecision::Skip;
        };

        if self.last_url.as_deref() == Some(url.as_str()) {
            let last = self.last_parse_at.unwrap_or(f64::MIN);
            if clock.now_secs() - last < CLIPBOARD_DEBOUNCE_S {
                return ClipboardDecision::Skip;
            }
        }
        self.last_url = Some(url.clone());
        self.last_parse_at = Some(clock.now_secs());
        ClipboardDecision::Parse(url)
    }
}

/// `Behavior.monitor_clipboard` (default false).
pub fn monitor_enabled(cfg: &ConfigStore) -> bool {
    cfg.get("Behavior", "monitor_clipboard")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// The URL to parse from clipboard text, if any: Python takes the whole
/// clipboard text and hands it to the URL classifier, which pulls the
/// matching URL out; blank text is skipped.
pub fn extract_parseable_url(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    urls::classify::classify(text).map(|_| text.to_owned())
}
