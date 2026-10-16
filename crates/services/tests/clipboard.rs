//! Clipboard monitor tests: injectable source + clock, debounce matrix,
//! startup re-check. Behavior ported from `src/gui/interface/parse.py`.

use config::ConfigStore;
use services::clipboard::{
    extract_parseable_url, monitor_enabled, ClipboardDecision, ClipboardMonitor, FixedClipboard,
    ManualClock, CLIPBOARD_DEBOUNCE_S,
};

fn store_with_monitor(dir: &std::path::Path, on: bool) -> ConfigStore {
    let mut cfg = ConfigStore::with_paths(dir, dir.join("downloads"));
    cfg.set("Behavior", "monitor_clipboard", serde_json::Value::Bool(on));
    cfg
}

const BV: &str = "https://www.bilibili.com/video/BV1xx411c7mD";

#[test]
fn gate_off_by_default_and_honors_config() {
    let dir = tempfile::tempdir().unwrap();
    // default config: monitor_clipboard = false
    let cfg = ConfigStore::with_paths(dir.path(), dir.path().join("downloads"));
    assert!(!monitor_enabled(&cfg));

    let mut src = FixedClipboard::new(Some(BV));
    let clock = ManualClock { secs: 0.0 };
    let mut mon = ClipboardMonitor::new();
    assert_eq!(mon.check(&cfg, &mut src, &clock), ClipboardDecision::Skip);

    // enabled → parses
    let cfg = store_with_monitor(dir.path(), true);
    assert_eq!(
        mon.check(&cfg, &mut src, &clock),
        ClipboardDecision::Parse(BV.to_owned())
    );
}

#[test]
fn non_urls_and_empty_clipboard_skip() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = store_with_monitor(dir.path(), true);
    let mut mon = ClipboardMonitor::new();
    for bad in [
        None,
        Some(""),
        Some("just some text"),
        Some("not a bilibili link"),
    ] {
        let mut src = FixedClipboard::new(bad);
        let clock = ManualClock { secs: 0.0 };
        assert_eq!(
            mon.check(&cfg, &mut src, &clock),
            ClipboardDecision::Skip,
            "clipboard {bad:?} should not parse"
        );
    }
}

#[test]
fn same_url_debounce_within_window() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = store_with_monitor(dir.path(), true);
    let mut mon = ClipboardMonitor::new();
    let mut src = FixedClipboard::new(Some(BV));
    let mut clock = ManualClock { secs: 0.0 };

    // t=0 parse
    assert_eq!(
        mon.check(&cfg, &mut src, &clock),
        ClipboardDecision::Parse(BV.into())
    );
    // t=0.7 same URL → debounced
    clock.secs = 0.7;
    assert_eq!(mon.check(&cfg, &mut src, &clock), ClipboardDecision::Skip);
    // t=1.4999 same URL → still debounced (window is [0, 1.5))
    clock.secs = 1.4999;
    assert_eq!(mon.check(&cfg, &mut src, &clock), ClipboardDecision::Skip);
    // t=1.5 same URL → allowed again
    clock.secs = 1.5;
    assert_eq!(
        mon.check(&cfg, &mut src, &clock),
        ClipboardDecision::Parse(BV.into())
    );
    assert_eq!(CLIPBOARD_DEBOUNCE_S, 1.5);
}

#[test]
fn different_url_bypasses_debounce() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = store_with_monitor(dir.path(), true);
    let mut mon = ClipboardMonitor::new();
    let mut clock = ManualClock { secs: 0.0 };

    let mut src = FixedClipboard::new(Some(BV));
    assert_eq!(
        mon.check(&cfg, &mut src, &clock),
        ClipboardDecision::Parse(BV.into())
    );

    // a different URL at t=0.1 parses immediately — debounce is per-URL
    let other = "https://www.bilibili.com/bangumi/play/ep123";
    let mut src = FixedClipboard::new(Some(other));
    clock.secs = 0.1;
    assert_eq!(
        mon.check(&cfg, &mut src, &clock),
        ClipboardDecision::Parse(other.into())
    );
}

#[test]
fn options_dialog_open_suppresses() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = store_with_monitor(dir.path(), true);
    let mut mon = ClipboardMonitor::new();
    mon.options_dialog_open = true;
    let mut src = FixedClipboard::new(Some(BV));
    let clock = ManualClock { secs: 0.0 };
    assert_eq!(mon.check(&cfg, &mut src, &clock), ClipboardDecision::Skip);
}

/// The startup re-check uses the same path as the changed-event: a URL
/// copied before launch parses once, then debounces.
#[test]
fn startup_recheck_uses_same_entry_point() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = store_with_monitor(dir.path(), true);
    let mut mon = ClipboardMonitor::new();
    let mut src = FixedClipboard::new(Some(BV));
    let mut clock = ManualClock { secs: 10.0 };
    // startup check (some time after boot; clipboard already holds the URL)
    assert_eq!(
        mon.check(&cfg, &mut src, &clock),
        ClipboardDecision::Parse(BV.into())
    );
    // immediately-visible clipboard-change event for the same URL is
    // debounced, not re-parsed
    clock.secs = 10.2;
    assert_eq!(mon.check(&cfg, &mut src, &clock), ClipboardDecision::Skip);
}

#[test]
fn url_extraction_delegates_to_classifier() {
    // plain match
    assert_eq!(extract_parseable_url(BV).as_deref(), Some(BV));
    assert_eq!(
        extract_parseable_url("https://www.bilibili.com/bangumi/play/ep123").as_deref(),
        Some("https://www.bilibili.com/bangumi/play/ep123")
    );
    // whitespace trimmed (clipboard text often has trailing newline)
    assert_eq!(
        extract_parseable_url(&format!("{BV}\n")).as_deref(),
        Some(BV)
    );
    assert_eq!(extract_parseable_url(""), None);
    assert_eq!(extract_parseable_url("   "), None);
    assert_eq!(
        extract_parseable_url("https://example.com/not-bilibili"),
        None
    );
}
