//! Auto-parse pagination dialog (port of
//! `src/gui/dialog/misc/auto_parse.py`).
//!
//! Page-range choice (all pages, or X..=Y), inter-page interval and two
//! behavior checkboxes; all three `Misc` keys persist on BOTH accept
//! and cancel, mirroring Python's `save_config()` in `accept()` and
//! `reject()`. The walk itself (`parsers::auto_parse::walk_pages`) runs
//! in the parse screen after accept.

use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::label::Label;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::WindowExt;
use gpui_kit::{div, prelude::*, px, AppContext, ParentElement, Window};

use super::active_catalog;

/// i18n context of the Python dialog class.
const CTX: &str = "AutoParseDialog";

/// Accepted walk request (Python `AutoParsePayload` minus parser_type;
/// the url is supplied by the caller that knows the current parse).
#[derive(Debug, Clone, PartialEq)]
pub struct AutoParsePayload {
    pub url: String,
    pub start_page: i64,
    pub end_page: i64,
    /// Seconds between page fetches, clamped 0.1..=15.0.
    pub interval: f64,
    pub auto_add: bool,
}

/// Headless view-model: everything the dialog edits, as strings the
/// number inputs produce plus parsed numbers.
#[derive(Debug, Clone)]
pub struct AutoParseState {
    pub all_pages: bool,
    pub start_page: i64,
    pub end_page: i64,
    pub total_pages: i64,
    /// Interval text as typed (clamped at read time).
    pub interval: f64,
    pub auto_add: bool,
    pub show_dialog: bool,
}

impl AutoParseState {
    /// Python ctor: "all pages" default, From/To default current →
    /// total, interval + checkboxes from config.
    pub fn new(cfg: &config::ConfigStore, current_page: i64, total_pages: i64) -> Self {
        Self {
            all_pages: true,
            start_page: current_page.max(1).min(total_pages.max(1)),
            end_page: total_pages.max(1),
            total_pages,
            interval: cfg
                .get("Misc", "auto_parse_interval")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(2.0),
            auto_add: cfg
                .get("Misc", "auto_add_to_download_list")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            show_dialog: cfg
                .get("Misc", "show_auto_parse_dialog")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        }
    }

    /// Clamp helper shared by both page fields: 1..=total_pages.
    pub fn clamp_page(page: i64, total_pages: i64) -> i64 {
        page.max(1).min(total_pages.max(1))
    }

    /// Clamp helper for the interval input: 0.1..=15.0.
    pub fn clamp_interval(interval: f64) -> f64 {
        interval.clamp(0.1, 15.0)
    }

    /// `validate()`: a specified range must have start ≤ end.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.all_pages || self.start_page <= self.end_page {
            return Ok(());
        }
        Err("The starting page cannot be greater than the ending page")
    }

    /// The payload for `accept()`: the page range and behavior flags.
    pub fn payload(&self, url: &str) -> AutoParsePayload {
        let (start_page, end_page) = if self.all_pages {
            (1, self.total_pages)
        } else {
            (self.start_page, self.end_page)
        };
        AutoParsePayload {
            url: url.to_owned(),
            start_page,
            end_page,
            interval: Self::clamp_interval(self.interval),
            auto_add: self.auto_add,
        }
    }

    /// `save_config()` — runs on accept AND cancel.
    pub fn save_config(&self, cfg: &mut config::ConfigStore) {
        cfg.set(
            "Misc",
            "auto_parse_interval",
            serde_json::Value::from(Self::clamp_interval(self.interval)),
        );
        cfg.set(
            "Misc",
            "auto_add_to_download_list",
            serde_json::Value::Bool(self.auto_add),
        );
        cfg.set(
            "Misc",
            "show_auto_parse_dialog",
            serde_json::Value::Bool(self.show_dialog),
        );
    }
}

/// Open the dialog. `on_accept` receives the walk payload.
pub fn open_auto_parse(
    cfg: config::ConfigStore,
    url: &str,
    current_page: i64,
    total_pages: i64,
    on_accept: impl FnOnce(AutoParsePayload, &mut gpui_kit::App) + 'static,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let catalog = active_catalog(&cfg);
    let title = catalog.tr(CTX, "Auto-parse Pagination").to_owned();
    let subtitle = catalog
        .tr(
            CTX,
            "Please select the parsing range and subsequent processing method",
        )
        .to_owned();
    let all_label = catalog.tr(CTX, "Parse all pages").to_owned();
    let range_label = catalog.tr(CTX, "Parse only pages X to Y").to_owned();
    let from_label = catalog.tr(CTX, "From").to_owned();
    let to_label = catalog.tr(CTX, "To").to_owned();
    let interval_label = catalog.tr(CTX, "Parsing Interval").to_owned();
    let seconds_label = catalog.tr(CTX, "seconds").to_owned();
    let auto_label = catalog
        .tr(
            CTX,
            "Automatically add to download list after parsing each page",
        )
        .to_owned();
    let show_label = catalog.tr(CTX, "Automatically show this dialog").to_owned();
    let warning = catalog.tr(
        CTX,
        "Warning: Due to Bilibili's anti-abuse mechanism, parsing too many pages and requesting too frequently may cause failure and IP ban. Use with caution.",
    )
    .to_owned();
    let start_label = catalog.tr(CTX, "Start Parsing").to_owned();
    let cancel_label = "Cancel".to_owned();

    let state = std::rc::Rc::new(std::cell::RefCell::new(AutoParseState::new(
        &cfg,
        current_page,
        total_pages,
    )));
    let cfg_cell = std::rc::Rc::new(std::cell::RefCell::new(cfg));
    let url = url.to_owned();
    let on_accept = std::rc::Rc::new(std::cell::RefCell::new(Some(on_accept)));

    let from_input = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder("1")
            .default_value(state.borrow().start_page.to_string())
    });
    let to_input = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder("1")
            .default_value(state.borrow().end_page.to_string())
    });
    let interval_input = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder("2.0")
            .default_value(state.borrow().interval.to_string())
    });

    window.open_dialog(cx, move |dialog, _, _| {
        let st = state.borrow();
        let all_pages = st.all_pages;
        let auto_add = st.auto_add;
        let show_dialog = st.show_dialog;
        drop(st);
        let range_selected = if all_pages { 0 } else { 1 };
        dialog
            .title(title.clone())
            .w(px(560.))
            .content({
                let state = state.clone();
                let from_input = from_input.clone();
                let to_input = to_input.clone();
                let interval_input = interval_input.clone();
                let all_label = all_label.clone();
                let range_label = range_label.clone();
                let from_label = from_label.clone();
                let to_label = to_label.clone();
                let interval_label = interval_label.clone();
                let seconds_label = seconds_label.clone();
                let show_label = show_label.clone();
                let auto_label = auto_label.clone();
                let subtitle = subtitle.clone();
                let warning = warning.clone();
                move |content, _, _| {
                    content.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(Label::new(subtitle.clone()))
                            .child(
                                RadioGroup::new("range")
                                    .selected_index(Some(range_selected))
                                    .child(
                                        Radio::new("all")
                                            .checked(all_pages)
                                            .label(all_label.clone())
                                            .on_click({
                                                let state = state.clone();
                                                move |checked, _, _| {
                                                    if *checked {
                                                        state.borrow_mut().all_pages = true;
                                                    }
                                                }
                                            }),
                                    )
                                    .child(
                                        Radio::new("range")
                                            .checked(!all_pages)
                                            .label(range_label.clone())
                                            .on_click({
                                                let state = state.clone();
                                                move |checked, _, _| {
                                                    if *checked {
                                                        state.borrow_mut().all_pages = false;
                                                    }
                                                }
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(Label::new(from_label.clone()))
                                    .child(Input::new(&from_input).id("from").disabled(all_pages))
                                    .child(Label::new(to_label.clone()))
                                    .child(Input::new(&to_input).id("to").disabled(all_pages)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(Label::new(interval_label.clone()))
                                    .child(Input::new(&interval_input).id("interval").w(px(80.)))
                                    .child(Label::new(seconds_label.clone())),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(Checkbox::new("auto-add").checked(auto_add).on_click({
                                        let state = state.clone();
                                        move |checked, _, _| {
                                            state.borrow_mut().auto_add = *checked;
                                        }
                                    }))
                                    .child(Label::new(auto_label.clone())),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        Checkbox::new("show-dialog").checked(show_dialog).on_click(
                                            {
                                                let state = state.clone();
                                                move |checked, _, _| {
                                                    state.borrow_mut().show_dialog = *checked;
                                                }
                                            },
                                        ),
                                    )
                                    .child(Label::new(show_label.clone())),
                            )
                            .child(Label::new(warning.clone())),
                    )
                }
            })
            .footer(
                DialogFooter::new()
                    .child(Button::new("cancel").label(cancel_label.clone()).on_click({
                        let state = state.clone();
                        let cfg_cell = cfg_cell.clone();
                        move |_, window, cx| {
                            // Python `reject()` also saves the config.
                            state.borrow().save_config(&mut cfg_cell.borrow_mut());
                            let _ = cfg_cell.borrow_mut().save();
                            window.close_dialog(cx);
                        }
                    }))
                    .child(Button::new("start").label(start_label.clone()).on_click({
                        let state = state.clone();
                        let cfg_cell = cfg_cell.clone();
                        let url = url.clone();
                        let on_accept = on_accept.clone();
                        let from_input = from_input.clone();
                        let to_input = to_input.clone();
                        let interval_input = interval_input.clone();
                        move |_, window, cx| {
                            {
                                let mut st = state.borrow_mut();
                                st.start_page =
                                    from_input.read(cx).value().parse().unwrap_or(st.start_page);
                                st.end_page =
                                    to_input.read(cx).value().parse().unwrap_or(st.end_page);
                                st.interval = interval_input
                                    .read(cx)
                                    .value()
                                    .parse()
                                    .unwrap_or(st.interval);
                                st.start_page =
                                    AutoParseState::clamp_page(st.start_page, st.total_pages);
                                st.end_page =
                                    AutoParseState::clamp_page(st.end_page, st.total_pages);
                                match st.validate() {
                                    Ok(()) => {
                                        st.save_config(&mut cfg_cell.borrow_mut());
                                        let _ = cfg_cell.borrow_mut().save();
                                    }
                                    Err(msg) => {
                                        let body = catalog.tr(CTX, msg).to_owned();
                                        let err_title = catalog.tr(CTX, "Invalid Range").to_owned();
                                        drop(st);
                                        window.push_notification(
                                            Notification::error(err_title).message(body),
                                            cx,
                                        );
                                        return;
                                    }
                                }
                            }
                            let payload = state.borrow().payload(&url);
                            window.close_dialog(cx);
                            if let Some(cb) = on_accept.borrow_mut().take() {
                                cb(payload, cx);
                            }
                        }
                    })),
            )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> config::ConfigStore {
        config::ConfigStore::with_paths("", "")
    }

    #[test]
    fn start_after_end_is_rejected() {
        let mut st = AutoParseState::new(&cfg(), 1, 5);
        st.all_pages = false;
        st.start_page = 4;
        st.end_page = 2;
        assert_eq!(
            st.validate().unwrap_err(),
            "The starting page cannot be greater than the ending page"
        );
        st.end_page = 4;
        assert!(st.validate().is_ok());
        // all-pages mode never trips the range check.
        st.start_page = 5;
        st.end_page = 1;
        st.all_pages = true;
        assert!(st.validate().is_ok());
    }

    #[test]
    fn all_pages_payload_spans_one_to_total() {
        let st = AutoParseState::new(&cfg(), 2, 7);
        let p = st.payload("u");
        assert_eq!((p.start_page, p.end_page), (1, 7));
        assert!(!p.url.is_empty());
    }

    #[test]
    fn specified_range_uses_edited_pages() {
        let mut st = AutoParseState::new(&cfg(), 2, 7);
        st.all_pages = false;
        st.start_page = 3;
        st.end_page = 5;
        let p = st.payload("u");
        assert_eq!((p.start_page, p.end_page), (3, 5));
    }

    #[test]
    fn interval_clamps_to_bounds() {
        assert_eq!(AutoParseState::clamp_interval(0.0), 0.1);
        assert_eq!(AutoParseState::clamp_interval(99.0), 15.0);
        assert_eq!(AutoParseState::clamp_interval(2.5), 2.5);
    }

    #[test]
    fn config_persists_all_three_keys() {
        let mut c = cfg();
        let mut st = AutoParseState::new(&c, 1, 3);
        st.interval = 4.0;
        st.auto_add = true;
        st.show_dialog = true;
        st.save_config(&mut c);
        assert_eq!(
            c.get("Misc", "auto_parse_interval")
                .and_then(serde_json::Value::as_f64),
            Some(4.0)
        );
        assert_eq!(
            c.get("Misc", "auto_add_to_download_list")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            c.get("Misc", "show_auto_parse_dialog")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }
}
