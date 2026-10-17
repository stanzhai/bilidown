//! Settings screen (ticket 24): port of `src/gui/interface/setting.py`
//! and its cards onto the gpui-component setting library, plus the
//! naming-rules editor (`rule_list.py` / `edit_rule.py`).

pub mod i18n_bridge;
pub mod model;
pub mod screen;

pub use screen::SettingsScreen;
