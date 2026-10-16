//! Teaching-tips state: the one-shot "show this tip once" gates from
//! `src/gui/interface/main_window.py::init_utils` (post-terms-check
//! sequence) and the auto-parse tip in `parse.py`.
//!
//! Config keys (all default false):
//!
//! - `Misc/tutorial_dialog_shown`
//! - `Misc/select_area_dialog_shown`
//! - `Misc/auto_parse_teaching_tip_shown_`
//! - `Misc/show_auto_parse_dialog`
//!
//! The login tip is gated on `Cookie/is_login` instead of a shown-flag.

use config::ConfigStore;

/// Which tip to show next, in Python's post-terms order
/// (`main_window.py:374-396`): update check → tutorial dialog → area
/// detection → login tip → download-path/ffmpeg/naming checks → clipboard
/// startup re-check. This module owns the three flag-gated ones; the
/// clipboard re-check lives in `clipboard`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingTip {
    /// First run: feature-overview dialog.
    TutorialDialog,
    /// First run: area-selection dialog (cn/ov).
    SelectAreaDialog,
    /// Login teaching tip (shown while `Cookie/is_login` is false).
    LoginTeachingTip,
    /// Auto-parse teaching tip (shown on the first paginated parse).
    AutoParseTeachingTip,
    None,
}

fn flag(cfg: &ConfigStore, group: &str, item: &str) -> bool {
    cfg.get(group, item).and_then(|v| v.as_bool()).unwrap_or(false)
}

/// Whether the tutorial dialog should show now (first launch only).
pub fn tutorial_pending(cfg: &ConfigStore) -> bool {
    !flag(cfg, "Misc", "tutorial_dialog_shown")
}

/// Whether the area-selection dialog should show now (first launch only).
pub fn area_dialog_pending(cfg: &ConfigStore) -> bool {
    !flag(cfg, "Misc", "select_area_dialog_shown")
}

/// Whether the login teaching tip should show now: only while logged out.
pub fn login_tip_pending(cfg: &ConfigStore) -> bool {
    !flag(cfg, "Cookie", "is_login")
}

/// Whether the auto-parse teaching tip should show now, per `parse.py`:
/// the parse is paginated (`total_pages > 1`), the tip was never shown,
/// and the "don't show auto-parse dialog" switch is off.
pub fn auto_parse_tip_pending(cfg: &ConfigStore, total_pages: u32) -> bool {
    total_pages > 1
        && !flag(cfg, "Misc", "auto_parse_teaching_tip_shown_")
        && !flag(cfg, "Misc", "show_auto_parse_dialog")
}

/// Record that the tutorial dialog was shown.
pub fn mark_tutorial_shown(cfg: &mut ConfigStore) {
    cfg.set("Misc", "tutorial_dialog_shown", serde_json::Value::Bool(true));
}

/// Record that the area-selection dialog was shown.
pub fn mark_area_dialog_shown(cfg: &mut ConfigStore) {
    cfg.set("Misc", "select_area_dialog_shown", serde_json::Value::Bool(true));
}

/// Record that the auto-parse teaching tip was shown.
pub fn mark_auto_parse_tip_shown(cfg: &mut ConfigStore) {
    cfg.set("Misc", "auto_parse_teaching_tip_shown_", serde_json::Value::Bool(true));
}

/// The next flag-gated tip in the post-terms order, or `None`.
pub fn next_tip(cfg: &ConfigStore) -> PendingTip {
    if tutorial_pending(cfg) {
        return PendingTip::TutorialDialog;
    }
    if area_dialog_pending(cfg) {
        return PendingTip::SelectAreaDialog;
    }
    if login_tip_pending(cfg) {
        return PendingTip::LoginTeachingTip;
    }
    PendingTip::None
}
