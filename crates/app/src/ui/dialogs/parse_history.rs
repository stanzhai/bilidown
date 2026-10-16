//! Parse-history dialog (port of `src/gui/dialog/misc/parse_history.py`).
//!
//! Read/delete/clear over `history.db` (`database::HistoryDatabase`,
//! capped at 100 rows newest-first). A row's Parse action closes the
//! dialog and re-parses its url; Delete/Clear mutate the database and
//! the row list. The db handle lives in a `RefCell` so delete/clear
//! share one connection.
use gpui_kit::component::button::Button;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::label::Label;
use gpui_kit::component::WindowExt;
use gpui_kit::{div, prelude::*, px, ParentElement, Window};

use services::i18n::{self, Catalog};

use super::active_catalog;

/// i18n context of the Python dialog class.
const CTX: &str = "ParseHistoryDialog";

/// One rendered row (Python `init_history_list` fields). `display_title`
/// falls back to the localized type when the stored title is blank.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryRow {
    pub history_id: String,
    pub title: String,
    pub url: String,
    pub type_: String,
    pub created_time: i64,
}

impl HistoryRow {
    pub fn from_entry(entry: &database::HistoryEntry) -> Self {
        Self {
            history_id: entry.history_id.clone(),
            title: entry.title.clone(),
            url: entry.url.clone(),
            type_: entry.type_.clone(),
            created_time: entry.created_time,
        }
    }

    /// Localized type label (unknown keys render as-is).
    fn type_label_str(&self, c: &Catalog) -> String {
        i18n::episode_type(c, &self.type_).unwrap_or_else(|| self.type_.clone())
    }

    /// Title column: the type label when the title is empty (Python
    /// `title = type_str if not title else title`).
    pub fn display_title(&self, c: &Catalog) -> String {
        let type_str = self.type_label_str(c);
        if self.title.is_empty() {
            type_str
        } else {
            self.title.clone()
        }
    }

    /// Type column.
    pub fn type_label(&self, c: &Catalog) -> String {
        self.type_label_str(c)
    }

    /// Parse Time column (`Time.format_timestamp` `%Y-%m-%d %H:%M:%S`).
    pub fn time_label(&self) -> String {
        crate::ui::fmt::format_timestamp(self.created_time)
    }
}

/// Headless row list: apply/clear/delete operations on `Vec<HistoryRow>`
/// (the database mutations run beside them in `open_parse_history`).
#[derive(Debug, Clone, Default)]
pub struct ParseHistoryState {
    pub rows: Vec<HistoryRow>,
}

impl ParseHistoryState {
    pub fn from_rows(rows: Vec<HistoryRow>) -> Self {
        Self { rows }
    }

    /// `on_delete`: drop the row with this history_id.
    pub fn delete(&mut self, history_id: &str) {
        self.rows.retain(|r| r.history_id != history_id);
    }

    /// `on_clear_history`.
    pub fn clear(&mut self) {
        self.rows.clear();
    }
}

/// Open the dialog. `on_accept` fires with the url of the row whose
/// Parse button was clicked (after the dialog closes).
pub fn open_parse_history(
    cfg: config::ConfigStore,
    on_accept: impl FnOnce(String, &mut gpui_kit::App) + 'static,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let catalog = active_catalog(&cfg);
    let title = catalog.tr(CTX, "Parse History").to_owned();
    let tip = catalog
        .tr(CTX, "Only the latest 100 records are kept.")
        .to_owned();
    let clear_label = catalog.tr(CTX, "Clear History").to_owned();
    let parse_label = catalog.tr("ParseActionWidget", "Parse").to_owned();
    let delete_label = catalog.tr("ParseTreeView", "Delete").to_owned();
    let ok = "OK".to_owned();

    // Synchronous sqlite, ≤100 rows: fine at dialog-open time.
    let db_path = dirs::data_dir().map(|d| d.join(config::APP_NAME).join("history.db"));
    let db = db_path
        .and_then(|p| database::HistoryDatabase::open(p).ok())
        .map(std::rc::Rc::new);
    let initial: Vec<HistoryRow> = db
        .as_ref()
        .and_then(|db| db.query_all().ok())
        .map(|entries| entries.iter().map(HistoryRow::from_entry).collect())
        .unwrap_or_default();

    let state = std::rc::Rc::new(std::cell::RefCell::new(ParseHistoryState::from_rows(
        initial,
    )));
    let on_accept = std::rc::Rc::new(std::cell::RefCell::new(Some(on_accept)));

    window.open_dialog(cx, move |dialog, _, _| {
        let catalog = catalog;
        let title = title.clone();
        let tip = tip.clone();
        let clear_label = clear_label.clone();
        let parse_label = parse_label.clone();
        let delete_label = delete_label.clone();
        let ok = ok.clone();
        dialog
            .title(title)
            .w(px(640.))
            .content({
                let state = state.clone();
                let db = db.clone();
                let on_accept = on_accept.clone();
                move |content, _, _| {
                    // Rows re-render from state on every dialog rebuild
                    // (≤100 rows; no per-row entity state to lose).
                    let rows = state.borrow().rows.clone();
                    let mut list = div()
                        .id("history-list")
                        .flex()
                        .flex_col()
                        .gap_2()
                        .max_h(px(400.))
                        .overflow_y_scroll()
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .items_center()
                                .child(Label::new(tip.clone()))
                                .child(
                                    Button::new("clear-history")
                                        .label(clear_label.clone())
                                        .on_click({
                                            let state = state.clone();
                                            let db = db.clone();
                                            move |_, _, _| {
                                                if let Some(db) = &db {
                                                    let _ = db.clear();
                                                }
                                                state.borrow_mut().clear();
                                            }
                                        }),
                                ),
                        );
                    if rows.is_empty() {
                        list = list.child(Label::new(""));
                    }
                    for (ix, row) in rows.iter().enumerate() {
                        let row_label = format!(
                            "{}  ·  {}  ·  {}  ·  {}",
                            ix + 1,
                            row.display_title(catalog),
                            row.type_label(catalog),
                            row.time_label()
                        );
                        let row_id = row.history_id.clone();
                        let row_url = row.url.clone();
                        list = list.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(div().flex_1().child(row_label))
                                .child(
                                    Button::new(("history-parse", ix))
                                        .label(parse_label.clone())
                                        .on_click({
                                            let on_accept = on_accept.clone();
                                            let url = row_url.clone();
                                            move |_, window, cx| {
                                                window.close_dialog(cx);
                                                if let Some(cb) = on_accept.borrow_mut().take() {
                                                    cb(url.clone(), cx);
                                                }
                                            }
                                        }),
                                )
                                .child(
                                    Button::new(("history-delete", ix))
                                        .label(delete_label.clone())
                                        .on_click({
                                            let state = state.clone();
                                            let db = db.clone();
                                            let history_id = row_id.clone();
                                            move |_, _, _| {
                                                if let Some(db) = &db {
                                                    // ponytail: delete failure keeps the row
                                                    // (IO corruption only); no toast path here.
                                                    let _ = db.delete(&history_id);
                                                }
                                                state.borrow_mut().delete(&history_id);
                                            }
                                        }),
                                ),
                        );
                    }
                    content.child(list)
                }
            })
            .footer(
                DialogFooter::new().child(Button::new("close").label(ok.clone()).on_click(
                    |_, window, cx| {
                        window.close_dialog(cx);
                    },
                )),
            )
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use services::i18n::{catalog, ResolvedLanguage};

    #[test]
    fn title_falls_back_to_type_label() {
        let c = catalog(ResolvedLanguage::En);
        let mut row = HistoryRow {
            history_id: "h1".into(),
            title: String::new(),
            url: "https://b23.tv/x".into(),
            type_: "USER_UPLOADS".into(),
            created_time: 1_700_000_000,
        };
        assert_eq!(row.display_title(c), "User Uploads");
        row.title = "标题".into();
        assert_eq!(row.display_title(c), "标题");
        assert_eq!(row.type_label(c), "User Uploads");
    }

    #[test]
    fn delete_and_clear_drop_rows() {
        let mut st = ParseHistoryState::from_rows(vec![
            HistoryRow {
                history_id: "a".into(),
                title: "1".into(),
                url: "u1".into(),
                type_: "USER_UPLOADS".into(),
                created_time: 1,
            },
            HistoryRow {
                history_id: "b".into(),
                title: "2".into(),
                url: "u2".into(),
                type_: "FAVORITES".into(),
                created_time: 2,
            },
        ]);
        st.delete("a");
        assert_eq!(st.rows.len(), 1);
        assert_eq!(st.rows[0].history_id, "b");
        st.clear();
        assert!(st.rows.is_empty());
    }

    #[test]
    fn time_label_formats_timestamp() {
        let row = HistoryRow {
            history_id: "h".into(),
            title: "t".into(),
            url: "u".into(),
            type_: "ANIME".into(),
            created_time: 1_700_000_000,
        };
        assert_eq!(row.time_label(), "2023-11-14 22:13:20");
    }
}
