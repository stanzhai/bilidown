//! Misc dialogs (ticket 25): batch parse, interactive-video node
//! selection, batch select, danmaku/subtitle style editors, update
//! prompt. Ports of `src/gui/dialog/misc/{batch_parse,interactive_video,
//! batch_select}.py`, `src/gui/dialog/setting/{danmaku_style,
//! subtitles_style}.py`, `src/gui/dialog/update.py` and
//! `src/util/misc/update.py`.
//!
//! Each dialog is a plain view-model struct (headless-testable, no
//! gpui) plus a thin `open_*` render fn using gpui-kit `Dialog`
//! through `WindowExt::open_dialog`.

pub mod about;
pub mod batch_parse;
pub mod batch_select;
pub mod interactive;
pub mod styles;
pub mod update;

use services::i18n::{self, Catalog};

/// The active catalog from the `Interface.language` config value.
pub fn active_catalog(cfg: &config::ConfigStore) -> &'static Catalog {
    let lang = cfg
        .get("Interface", "language")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Auto");
    i18n::catalog(i18n::Language::from_config(lang).resolve())
}

/// Replace `{key}` placeholders in a translated format string
/// (`.format(key=value)` in Python).
pub fn format_tr(template: &str, pairs: &[(&str, &str)]) -> String {
    let mut out = template.to_owned();
    for (key, value) in pairs {
        out = out.replace(&format!("{{{key}}}"), value);
    }
    out
}
