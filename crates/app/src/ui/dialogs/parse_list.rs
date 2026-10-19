//! Parse-list settings dialog (port of
//! `src/gui/dialog/setting/parse_list.py`): column visibility/order plus
//! two list-behavior toggles. Python uses a checkable drag list; gpui-kit
//! has none, so each column row is a checkbox plus ↑/↓ buttons. The
//! first column (`number`) stays checked and pinned at row 0
//! (`setRowEnabled(0, False)` / `setMinDragRow(1)`).

use serde_json::{json, Value};

use gpui_kit::base::Disableable as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::label::Label;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::WindowExt;
use gpui_kit::{div, prelude::*, px, ParentElement, Window};

use super::active_catalog;

const CTX: &str = "ParseListSettingsDialog";

/// One column entry exactly as the config stores it.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnEntry {
    pub attr_key: String,
    pub width: i64,
    pub show: bool,
}

/// `Translator.COLUMN_NAME` (translator.py:251): attr_key → source key.
fn column_source(key: &str) -> &'static str {
    match key {
        "number" => "No.",
        "title" => "Title",
        "badge" => "Notes",
        "duration" => "Duration",
        "dyn_time" => "Publish Time / Favorite Time / Last Watched",
        "pubtime" => "Publish Time",
        "favtime" => "Favorite Time",
        "viewtime" => "Last Watched",
        _ => "No.",
    }
}

pub fn column_label(c: &services::i18n::Catalog, key: &str) -> String {
    c.tr("COLUMN_NAME", column_source(key)).to_owned()
}

/// Headless state: ordered column entries + the two toggles.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseListState {
    pub columns: Vec<ColumnEntry>,
    pub alternate_row_color: bool,
    pub show_floating_command_bar: bool,
}

impl ParseListState {
    pub fn load(cfg: &config::ConfigStore) -> Self {
        let b = |item: &str| {
            cfg.get("Behavior", item)
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        };
        Self {
            columns: cfg
                .get("Behavior", "parse_list_column")
                .and_then(|v| v.as_array().cloned())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| {
                            Some(ColumnEntry {
                                attr_key: v.get("attr_key")?.as_str()?.to_owned(),
                                width: v
                                    .get("width")
                                    .and_then(Value::as_i64)
                                    .unwrap_or(0),
                                show: v.get("show").and_then(Value::as_bool).unwrap_or(true),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
            alternate_row_color: b("parse_list_alternate_row_color"),
            show_floating_command_bar: b("parse_list_show_floating_command_bar"),
        }
    }

    /// Move column `ix` within `[1, len)` — row 0 (the number column) is
    /// pinned (`setMinDragRow(1)`).
    pub fn move_column(&mut self, ix: usize, up: bool) {
        if ix == 0 {
            return;
        }
        let target = if up { ix.checked_sub(1) } else { Some(ix + 1) };
        if let Some(target) = target {
            if target >= 1 && target < self.columns.len() {
                self.columns.swap(ix, target);
            }
        }
    }

    /// `accept()`: skip the write on an empty list (`if not column_list:
    /// return`) and keep `number` shown + first.
    pub fn apply(&self, cfg: &mut config::ConfigStore) {
        if self.columns.is_empty() {
            return;
        }
        let mut columns = self.columns.clone();
        columns[0].attr_key = "number".into();
        columns[0].show = true;
        cfg.set(
            "Behavior",
            "parse_list_column",
            json!(columns
                .iter()
                .map(|c| json!({"attr_key": c.attr_key, "width": c.width, "show": c.show}))
                .collect::<Vec<_>>()),
        );
        cfg.set(
            "Behavior",
            "parse_list_alternate_row_color",
            json!(self.alternate_row_color),
        );
        cfg.set(
            "Behavior",
            "parse_list_show_floating_command_bar",
            json!(self.show_floating_command_bar),
        );
    }
}

/// Open the dialog (Python `ParsingSettingCard` →
/// `custom_parse_list_btn`).
pub fn open_parse_list(cfg: config::ConfigStore, window: &mut Window, cx: &mut gpui_kit::App) {
    let catalog = active_catalog(&cfg);
    let title = catalog.tr(CTX, "Parse List Settings").to_owned();
    let column_hint = catalog
        .tr(CTX, "Select columns to display and drag to reorder")
        .to_owned();
    let alternate_label = catalog.tr(CTX, "Enable alternate row colors").to_owned();
    let floating_label = catalog.tr(CTX, "Show floating command bar").to_owned();
    let ok = "OK".to_owned();

    let state = std::rc::Rc::new(std::cell::RefCell::new(ParseListState::load(&cfg)));
    let cfg_cell = std::rc::Rc::new(std::cell::RefCell::new(cfg));
    let order_cell = state.clone();

    window.open_dialog(cx, move |dialog, _, _| {
        // `open_dialog` takes `Fn`; clone captures so inner `move`
        // closures own locals instead of this closure's captures.
        let column_hint = column_hint.clone();

        let rows = {
            let state = state.borrow();
            state
                .columns
                .iter()
                .enumerate()
                .map(|(ix, c)| (ix, c.attr_key.clone(), c.show))
                .collect::<Vec<_>>()
        };
        let (alternate_label, floating_label) = (alternate_label.clone(), floating_label.clone());
        let content_rows = rows.clone();
        let content_order_cell = order_cell.clone();
        let footer_state = state.clone();
        dialog
            .title(title.clone())
            .w(px(520.))
            .content({
                let state = state.clone();
                move |content, _, _| {
                    let st = state.borrow();
                    content.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(Label::new(column_hint.clone()))
                            .child(
                                div()
                                    .id("parse-list-columns")
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .max_h(px(300.))
                                    .overflow_y_scroll()
                                    .children(content_rows.iter().map(|&(ix, ref key, show)| {
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                Checkbox::new(("col-check", ix))
                                                    .checked(show)
                                                    .disabled(ix == 0)
                                                    .label(column_label(catalog, key))
                                                    .on_click({
                                                        let state = state.clone();
                                                        move |v, _, _| {
                                                            if ix != 0 {
                                                                state.borrow_mut().columns[ix]
                                                                    .show = *v;
                                                            }
                                                        }
                                                    }),
                                            )
                                            .child(div().flex_1())
                                            .child(
                                                Button::new(("col-up", ix))
                                                    .label("↑")
                                                    .outline()
                                                    .disabled(ix <= 1)
                                                    .on_click({
                                                        let order_cell = content_order_cell.clone();
                                                        move |_, _, cx| {
                                                            order_cell
                                                                .borrow_mut()
                                                                .move_column(ix, true);
                                                            cx.refresh_windows();
                                                        }
                                                    }),
                                            )
                                            .child(
                                                Button::new(("col-down", ix))
                                                    .label("↓")
                                                    .outline()
                                                    .disabled(
                                                        ix + 1
                                                            >= content_order_cell
                                                                .borrow()
                                                                .columns
                                                                .len(),
                                                    )
                                                    .on_click({
                                                        let order_cell = content_order_cell.clone();
                                                        move |_, _, cx| {
                                                            order_cell
                                                                .borrow_mut()
                                                                .move_column(ix, false);
                                                            cx.refresh_windows();
                                                        }
                                                    }),
                                            )
                                    })),
                            )
                            .child(
                                Switch::new("alternate-row-color")
                                    .checked(st.alternate_row_color)
                                    .label(alternate_label.clone())
                                    .on_change({
                                        let state = state.clone();
                                        move |v, _, _| {
                                            state.borrow_mut().alternate_row_color = *v;
                                        }
                                    }),
                            )
                            .child(
                                Switch::new("floating-command-bar")
                                    .checked(st.show_floating_command_bar)
                                    .label(floating_label.clone())
                                    .on_change({
                                        let state = state.clone();
                                        move |v, _, _| {
                                            state.borrow_mut().show_floating_command_bar = *v;
                                        }
                                    }),
                            ),
                    )
                }
            })
            .footer(
                DialogFooter::new().child(Button::new("confirm").label(ok.clone()).on_click({
                    let state = footer_state.clone();
                    let cfg_cell = cfg_cell.clone();
                    move |_, window, cx| {
                        state.borrow().apply(&mut cfg_cell.borrow_mut());
                        if let Err(e) = cfg_cell.borrow().save() {
                            window.push_notification(Notification::error(e.to_string()), cx);
                            return;
                        }
                        window.close_dialog(cx);
                    }
                })),
            )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_cfg() -> config::ConfigStore {
        let dir = std::env::temp_dir().join(format!(
            "t31-parse-list-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        config::ConfigStore::with_paths(&dir, "")
    }

    #[test]
    fn loads_defaults_and_pins_first_column() {
        let cfg = scratch_cfg();
        let st = ParseListState::load(&cfg);
        assert!(!st.columns.is_empty());
        assert_eq!(st.columns[0].attr_key, "number");
        assert!(st.alternate_row_color);
        assert!(st.show_floating_command_bar);
    }

    #[test]
    fn move_column_never_touches_row_zero() {
        let mut st = ParseListState::load(&scratch_cfg());
        let first = st.columns[0].clone();
        st.move_column(0, false);
        assert_eq!(st.columns[0], first);
        st.move_column(1, true);
        assert_eq!(st.columns[0], first);
    }

    #[test]
    fn apply_forces_number_shown_and_round_trips() {
        let mut cfg = scratch_cfg();
        let mut st = ParseListState::load(&cfg);
        st.columns[0].show = false;
        st.alternate_row_color = false;
        st.apply(&mut cfg);
        let reread = ParseListState::load(&cfg);
        assert!(reread.columns[0].show);
        assert!(!reread.alternate_row_color);
        assert_eq!(reread.columns.len(), st.columns.len());
    }

    #[test]
    fn column_labels_localize() {
        let c = services::i18n::catalog(services::i18n::ResolvedLanguage::ZhCn);
        assert_eq!(column_label(c, "number"), "序号");
        assert_eq!(column_label(c, "title"), "标题");
        assert_eq!(column_label(c, "duration"), "时长");
    }
}
