//! Multi-part (分P) video list dialog (port of
//! `src/gui/dialog/misc/multi_part_lists.py`).
//!
//! The caller parses the item's url (view fetch with `ugc_season`
//! stripped so the 分P list parser runs), then hands the child rows
//! here: per-row checkbox, Check All, live count label, OK disabled
//! until something is checked. Accept hands back the checked episode
//! dicts — TREE_NODE structural rows excluded — for task creation
//! with the numbering reset to 1.

use std::collections::HashSet;

use gpui_kit::base::Disableable;
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::label::Label;
use gpui_kit::component::list::ListItem;
use gpui_kit::component::WindowExt;
use gpui_kit::{div, prelude::*, px, ParentElement, Window};
use serde_json::{Map, Value};
use services::i18n::Catalog;

use super::{active_catalog, format_tr};

/// i18n context of the Python dialog class.
const CTX: &str = "MultiPartListsDialog";

/// `Attribute.TREE_NODE_BIT` on the raw `attribute` int of a dict.
const TREE_NODE_BIT: i64 = 1 << 15;

/// `Units.format_duration`: `MM:SS`, or `HH:MM:SS` past an hour.
pub fn format_duration(duration: i64) -> String {
    let hours = duration / 3600;
    let mins = (duration - hours * 3600) / 60;
    let secs = duration - hours * 3600 - mins * 60;
    let z2 = |n: i64| format!("{n:02}");
    if hours > 0 {
        format!("{}:{}:{}", z2(hours), z2(mins), z2(secs))
    } else {
        format!("{}:{}", z2(mins), z2(secs))
    }
}

/// One displayed 分P row (`appendCheckableRow(no, title, duration, data)`).
#[derive(Debug, Clone)]
pub struct MultiPartRow {
    /// 1-based part number shown in the No. column.
    pub no: usize,
    pub title: String,
    pub duration_label: String,
    /// The episode dict handed to task creation on accept.
    pub dict: Map<String, Value>,
}

/// Headless view-model of the checkable list.
#[derive(Debug, Clone, Default)]
pub struct MultiPartState {
    pub rows: Vec<MultiPartRow>,
    pub checked: HashSet<usize>,
}

impl MultiPartState {
    /// Build from the parsed node's children (in order).
    pub fn from_children(children: &[parsers::tree::TreeItem]) -> Self {
        let rows = children
            .iter()
            .enumerate()
            .map(|(ix, child)| {
                let dict = child.to_dict().as_object().cloned().unwrap_or_default();
                MultiPartRow {
                    no: ix + 1,
                    title: child.title.clone(),
                    duration_label: format_duration(child.duration),
                    dict,
                }
            })
            .collect();
        Self {
            rows,
            checked: HashSet::new(),
        }
    }

    /// `update_count_label`.
    pub fn count_label(&self, catalog: &Catalog) -> String {
        let selected = self.checked.len();
        let total = self.rows.len();
        if selected > 0 {
            format_tr(
                catalog.tr(CTX, "{selected_count} selected, {total_count} total"),
                &[
                    ("selected_count", &selected.to_string()),
                    ("total_count", &total.to_string()),
                ],
            )
        } else {
            format_tr(
                catalog.tr(CTX, "{total_count} total"),
                &[("total_count", &total.to_string())],
            )
        }
    }

    /// Toggle one row's checkbox.
    pub fn toggle(&mut self, ix: usize) {
        if !self.checked.remove(&ix) {
            self.checked.insert(ix);
        }
    }

    /// `on_check_all_chk_stateChanged`: check / uncheck everything.
    pub fn set_all(&mut self, checked: bool) {
        if checked {
            self.checked = (0..self.rows.len()).collect();
        } else {
            self.checked.clear();
        }
    }

    /// `accept()`: checked episode dicts with TREE_NODE rows excluded.
    pub fn checked_dicts(&self) -> Vec<Map<String, Value>> {
        let mut ixs: Vec<usize> = self.checked.iter().copied().collect();
        ixs.sort_unstable();
        ixs.into_iter()
            .filter(|&ix| {
                let attr = self
                    .rows
                    .get(ix)
                    .and_then(|r| r.dict.get("attribute"))
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                attr & TREE_NODE_BIT == 0
            })
            .filter_map(|ix| self.rows.get(ix).map(|r| r.dict.clone()))
            .collect()
    }
}

/// Open the dialog; `on_accept` gets the checked episode dicts
/// (TREE_NODE rows filtered out, non-empty).
pub fn open_multi_part_lists(
    cfg: config::ConfigStore,
    rows: Vec<MultiPartRow>,
    on_accept: impl FnOnce(Vec<Map<String, Value>>, &mut gpui_kit::App) + 'static,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let catalog = active_catalog(&cfg);
    let title = catalog.tr(CTX, "Multi-part video list").to_owned();
    let check_all = catalog.tr(CTX, "Check All").to_owned();
    let ok_label = catalog.tr(CTX, "Download Selected Items").to_owned();

    let state = std::rc::Rc::new(std::cell::RefCell::new(MultiPartState {
        rows,
        checked: HashSet::new(),
    }));
    let on_accept = std::rc::Rc::new(std::cell::RefCell::new(Some(on_accept)));

    window.open_dialog(cx, move |dialog, _, _| {
        let st = state.borrow();
        let count_label = st.count_label(catalog);
        let all_checked = !st.rows.is_empty() && st.checked.len() == st.rows.len();
        let ok_enabled = !st.checked.is_empty();
        let rows = st.rows.clone();
        let checked: Vec<usize> = st.checked.iter().copied().collect();
        drop(st);

        dialog
            .title(title.clone())
            .w(px(620.))
            .content({
                let state = state.clone();
                let check_all = check_all.clone();
                let rows = rows.clone();
                let checked = checked.clone();
                move |content, _, _| {
                    let row_children: Vec<_> = rows
                        .iter()
                        .enumerate()
                        .map(|(ix, row)| {
                            let is_checked = checked.contains(&ix);
                            ListItem::new(("part-row", ix))
                                .child(format!("{}. {}", row.no, row.title))
                                .child(row.duration_label.clone())
                                .suffix({
                                    let state = state.clone();
                                    move |_, _| {
                                        Checkbox::new(("part-check", ix))
                                            .checked(is_checked)
                                            .on_click({
                                                let state = state.clone();
                                                move |_, _, _| {
                                                    state.borrow_mut().toggle(ix);
                                                }
                                            })
                                    }
                                })
                        })
                        .collect();
                    content.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        Checkbox::new("check-all").checked(all_checked).on_click({
                                            let state = state.clone();
                                            move |checked, _, _| {
                                                state.borrow_mut().set_all(*checked);
                                            }
                                        }),
                                    )
                                    .child(Label::new(check_all.clone()))
                                    .child(Label::new(count_label.clone())),
                            )
                            .child(
                                div()
                                    .id("part-list")
                                    .flex()
                                    .flex_col()
                                    .max_h(px(320.))
                                    .overflow_y_scroll()
                                    .children(row_children),
                            ),
                    )
                }
            })
            .footer(
                DialogFooter::new().child(
                    Button::new("part-ok")
                        .label(ok_label.clone())
                        .disabled(!ok_enabled)
                        .on_click({
                            let state = state.clone();
                            let on_accept = on_accept.clone();
                            move |_, window, cx| {
                                let dicts = state.borrow().checked_dicts();
                                if dicts.is_empty() {
                                    return; // Python: accept is a no-op
                                }
                                window.close_dialog(cx);
                                if let Some(cb) = on_accept.borrow_mut().take() {
                                    cb(dicts, cx);
                                }
                            }
                        }),
                ),
            )
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use parsers::tree::{Attribute, Number, TreeItem};

    fn part(title: &str, duration: i64, attr: Attribute) -> TreeItem {
        let mut item = TreeItem::empty();
        item.title = title.to_owned();
        item.duration = duration;
        item.number = Number::Int(1);
        item.set_attribute(attr);
        item
    }

    fn catalog() -> &'static Catalog {
        // The En catalog is identity (no entries); assert real zh_CN
        // strings like batch_parse does.
        services::i18n::catalog(services::i18n::Language::from_config("zh_CN").resolve())
    }

    #[test]
    fn format_duration_matches_python() {
        assert_eq!(format_duration(0), "00:00");
        assert_eq!(format_duration(59), "00:59");
        assert_eq!(format_duration(60), "01:00");
        assert_eq!(format_duration(3671), "01:01:11");
        assert_eq!(format_duration(3600), "01:00:00");
    }

    #[test]
    fn checked_dicts_excludes_tree_nodes() {
        let children = vec![
            part("P1", 10, Attribute::NORMAL_BIT),
            part("section", 0, Attribute::TREE_NODE_BIT),
            part("P2", 20, Attribute::NORMAL_BIT),
        ];
        let mut st = MultiPartState::from_children(&children);
        st.set_all(true);
        let dicts = st.checked_dicts();
        assert_eq!(dicts.len(), 2);
        assert_eq!(dicts[0]["title"], "P1");
        assert_eq!(dicts[1]["title"], "P2");
    }

    #[test]
    fn count_label_and_toggle() {
        let children = vec![
            part("P1", 1, Attribute::NORMAL_BIT),
            part("P2", 2, Attribute::NORMAL_BIT),
        ];
        let mut st = MultiPartState::from_children(&children);
        assert_eq!(st.count_label(catalog()), "共 2 项");
        st.toggle(1);
        assert_eq!(st.count_label(catalog()), "已选择 1 项，共 2 项");
        st.toggle(1);
        assert_eq!(st.count_label(catalog()), "共 2 项");
        st.set_all(true);
        assert_eq!(st.checked.len(), 2);
        assert_eq!(st.count_label(catalog()), "已选择 2 项，共 2 项");
        st.set_all(false);
        assert!(st.checked.is_empty());
    }

    #[test]
    fn rows_number_from_one_and_format_duration_label() {
        let children = vec![part("First", 61, Attribute::NORMAL_BIT)];
        let st = MultiPartState::from_children(&children);
        assert_eq!(st.rows[0].no, 1);
        assert_eq!(st.rows[0].duration_label, "01:01");
    }
}
