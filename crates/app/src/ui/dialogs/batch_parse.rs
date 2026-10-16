//! Batch parse dialog (port of `src/gui/dialog/misc/batch_parse.py`).
//!
//! Multi-line av/BV input, live link count, Clear button, auto-add
//! checkbox persisted to `Misc/auto_add_to_download_list`. Accept
//! validates through `parsers::batch::validate_batch_input` (the same
//! blank-drop + av/BV gate) and returns the url list.

use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::label::Label;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::WindowExt;
use gpui_kit::{AppContext, ParentElement, Window, div, px, prelude::*};
use parsers::batch::{validate_batch_input, BatchError};
use services::i18n::Catalog;

use super::{active_catalog, format_tr};

/// i18n context of the Python dialog class.
const CTX: &str = "BatchParseDialog";

/// Headless view-model of the dialog: the text box content plus the
/// auto-add checkbox.
#[derive(Debug, Clone, Default)]
pub struct BatchParseState {
    pub text: String,
    pub auto_add: bool,
}

impl BatchParseState {
    pub fn new(cfg: &config::ConfigStore) -> Self {
        Self {
            text: String::new(),
            auto_add: cfg
                .get("Misc", "auto_add_to_download_list")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        }
    }

    /// Non-blank line count (`on_lines_changed`).
    pub fn link_count(&self) -> usize {
        self.text.lines().filter(|l| !l.trim().is_empty()).count()
    }

    /// Live count label (`"Link Count: {count}"`).
    pub fn count_label(&self, c: &Catalog) -> String {
        format_tr(
            c.tr(CTX, "Link Count: {count}"),
            &[("count", &self.link_count().to_string())],
        )
    }

    /// `accept()`: validate, persist the checkbox, return the url
    /// list. The error carries the toast title + body source pair
    /// (localize with [`localize_error`]).
    pub fn accept(
        &mut self,
        cfg: &mut config::ConfigStore,
    ) -> Result<Vec<String>, (String, String)> {
        let urls = validate_batch_input(&self.text).map_err(|e| match e {
            BatchError::NoLinks => (
                "No Links Provided".to_owned(),
                "Please paste video links in the text box.".to_owned(),
            ),
            _ => (
                "Invalid Link Format".to_owned(),
                "Currently only av or BV links are supported.".to_owned(),
            ),
        })?;
        cfg.set(
            "Misc",
            "auto_add_to_download_list",
            serde_json::Value::Bool(self.auto_add),
        );
        Ok(urls)
    }
}

/// Localize an `accept` error pair into the dialog's language.
pub fn localize_error(c: &Catalog, e: &(String, String)) -> (String, String) {
    (
        c.tr(CTX, &e.0).to_owned(),
        c.tr(CTX, &e.1).to_owned(),
    )
}

/// Open the dialog. `on_accept` receives the validated url list plus
/// the auto-add flag.
pub fn open_batch_parse(
    cfg: config::ConfigStore,
    on_accept: impl FnOnce(Vec<String>, bool, &mut gpui_kit::App) + 'static,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let catalog = active_catalog(&cfg);
    let title = catalog.tr(CTX, "Batch Parse").to_owned();
    let clear = catalog.tr(CTX, "Clear").to_owned();
    let start = catalog.tr(CTX, "Start Parsing").to_owned();
    let auto_label = catalog
        .tr(
            CTX,
            "Automatically add to download list after parsing each link",
        )
        .to_owned();

    let state = std::rc::Rc::new(std::cell::RefCell::new(BatchParseState::new(&cfg)));
    let textarea = cx.new(|cx| {
        TextareaState::new(window, cx).placeholder(
            "Paste video links here, one per line. Currently only av and BV links are supported.",
        )
    });
    let cfg_cell = std::rc::Rc::new(std::cell::RefCell::new(cfg));
    let on_accept = std::rc::Rc::new(std::cell::RefCell::new(Some(on_accept)));

    window.open_dialog(cx, move |dialog, _, _| {
        let count = state.borrow().count_label(catalog);
        let auto_add = state.borrow().auto_add;
        dialog
            .title(title.clone())
            .w(px(620.))
            .content({
                let state = state.clone();
                let textarea = textarea.clone();
                let auto_label = auto_label.clone();
                move |content, _, _| {
                content.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .items_center()
                                .child(Label::new(count.clone())),
                        )
                        .child(Textarea::new(&textarea).h(px(180.)))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    Checkbox::new("auto-add")
                                        .checked(auto_add)
                                        .on_click({
                                            let state = state.clone();
                                            move |checked, _, _| {
                                                state.borrow_mut().auto_add = *checked;
                                            }
                                        }),
                                )
                                .child(Label::new(auto_label.clone())),
                        ),
                )
                }
            })
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("clear").label(clear.clone()).on_click({
                            let state = state.clone();
                            let textarea = textarea.clone();
                            move |_, window, cx| {
                                state.borrow_mut().text.clear();
                                textarea.update(cx, |s, cx| {
                                    s.set_value("", window, cx);
                                });
                            }
                        }),
                    )
                    .child(Button::new("start").label(start.clone()).on_click({
                        let state = state.clone();
                        let cfg_cell = cfg_cell.clone();
                        let on_accept = on_accept.clone();
                        let textarea = textarea.clone();
                        move |_, window, cx| {
                            let text = textarea.read(cx).value().to_string();
                            state.borrow_mut().text = text;
                            let mut st = state.borrow_mut();
                            match st.accept(&mut cfg_cell.borrow_mut()) {
                                Ok(urls) => {
                                    let auto_add = st.auto_add;
                                    drop(st);
                                    window.close_dialog(cx);
                                    if let Some(cb) = on_accept.borrow_mut().take() {
                                        cb(urls, auto_add, cx);
                                    }
                                }
                                Err(e) => {
                                    drop(st);
                                    let (title, body) = localize_error(catalog, &e);
                                    window.push_notification(
                                        Notification::error(title).message(body),
                                        cx,
                                    );
                                }
                            }
                        }
                    })),
            )
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use services::i18n::{catalog, ResolvedLanguage};

    #[test]
    fn link_count_drops_blank_lines() {
        let st = BatchParseState {
            text: "BV1aa\n\n  \nav123\n".to_owned(),
            auto_add: false,
        };
        assert_eq!(st.link_count(), 2);
        let empty = BatchParseState::default();
        assert_eq!(empty.link_count(), 0);
    }
    #[test]
    fn count_label_formats_the_count() {
        let c = catalog(ResolvedLanguage::ZhCn);
        let st = BatchParseState {
            text: "BV1aa\nav2\nBV3".to_owned(),
            auto_add: false,
        };
        assert_eq!(st.count_label(c), "链接数量：3");
    }

    #[test]
    fn accept_validates_and_persists_auto_add() {
        let mut cfg = config::ConfigStore::with_paths("", "");
        let mut st = BatchParseState {
            text: "BV1aa\nav123".to_owned(),
            auto_add: true,
        };
        let urls = st.accept(&mut cfg).unwrap();
        assert_eq!(urls, vec!["BV1aa", "av123"]);
        assert_eq!(
            cfg.get("Misc", "auto_add_to_download_list"),
            Some(&serde_json::json!(true))
        );
    }

    #[test]
    fn accept_empty_is_no_links() {
        let mut cfg = config::ConfigStore::with_paths("", "");
        let mut st = BatchParseState {
            text: "  \n \n".to_owned(),
            auto_add: false,
        };
        let err = st.accept(&mut cfg).unwrap_err();
        assert_eq!(err.0, "No Links Provided");
        // nothing persisted on rejection
        assert_eq!(
            cfg.get("Misc", "auto_add_to_download_list"),
            Some(&serde_json::json!(false))
        );
    }

    #[test]
    fn accept_non_av_bv_line_is_invalid_format() {
        let mut cfg = config::ConfigStore::with_paths("", "");
        let mut st = BatchParseState {
            text: "BV1aa\nhttps://space.bilibili.com/42".to_owned(),
            auto_add: false,
        };
        let err = st.accept(&mut cfg).unwrap_err();
        assert_eq!(err.0, "Invalid Link Format");
    }

    #[test]
    fn errors_localize() {
        let c = catalog(ResolvedLanguage::ZhCn);
        let e = localize_error(
            c,
            &("No Links Provided".to_owned(), "Please paste video links in the text box.".to_owned()),
        );
        assert_eq!(e.0, "链接数量为空");
        assert_eq!(e.1, "请在文本框中粘贴视频链接");
    }
}
