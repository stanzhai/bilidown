//! Batch select dialog (port of `src/gui/dialog/misc/batch_select.py`).
//!
//! "1,3,5-10" → the number list handed to
//! `ParseViewModel::batch_select`. Validation mirrors the Python
//! `validate()`: empty input, bad range (non-digit, <= 0, start > end),
//! bad single number.

use crate::ui::parse::model::parse_ranges;

use gpui_kit::component::button::Button;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::WindowExt;
use gpui_kit::{div, prelude::*, px, AppContext, ParentElement, Window};

use super::active_catalog;

const CTX: &str = "BatchSelectDialog";

/// Validation error of the dialog's `accept()` (Python `validate()`
/// messages, localized at the call site).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchSelectError {
    Empty,
    InvalidRange,
    InvalidNumber,
}

impl BatchSelectError {
    /// Localized toast message.
    pub fn message(&self, c: &services::i18n::Catalog) -> String {
        c.tr(
            CTX,
            match self {
                Self::Empty => "Please enter line numbers",
                Self::InvalidRange => "Invalid line number range",
                Self::InvalidNumber => "Invalid line number",
            },
        )
        .to_owned()
    }
}

/// Headless view-model: the raw text box content.
#[derive(Debug, Clone, Default)]
pub struct BatchSelectState {
    pub text: String,
}

impl BatchSelectState {
    /// `to_number_list()` + `validate()`: parse or fail with the
    /// dialog's error.
    ///
    /// Python checks digits and `> 0` explicitly (so "0" and "-1" are
    /// invalid numbers, "3-1" an invalid range); `parse_ranges`
    /// accepts zero/negatives, so those gates run here.
    pub fn numbers(&self) -> Result<Vec<i64>, BatchSelectError> {
        let trimmed = self.text.trim();
        if trimmed.is_empty() {
            return Err(BatchSelectError::Empty);
        }
        for part in trimmed.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            match part.split_once('-') {
                Some((start, end)) => {
                    let (start, end) = (start.trim(), end.trim());
                    let ok = start.chars().all(|ch| ch.is_ascii_digit())
                        && end.chars().all(|ch| ch.is_ascii_digit())
                        && !start.is_empty()
                        && !end.is_empty();
                    if !ok {
                        return Err(BatchSelectError::InvalidRange);
                    }
                    let (s, e): (i64, i64) = (start.parse().unwrap_or(0), end.parse().unwrap_or(0));
                    if s <= 0 || e <= 0 || s > e {
                        return Err(BatchSelectError::InvalidRange);
                    }
                }
                None => {
                    if part.is_empty()
                        || part.len() > 9
                        || !part.chars().all(|ch| ch.is_ascii_digit())
                        || part.parse::<i64>().unwrap_or(0) <= 0
                    {
                        return Err(BatchSelectError::InvalidNumber);
                    }
                }
            }
        }
        parse_ranges(trimmed).ok_or(BatchSelectError::InvalidNumber)
    }
}

/// Open the dialog; `on_accept` gets the parsed numbers.
pub fn open_batch_select(
    cfg: config::ConfigStore,
    on_accept: impl FnOnce(Vec<i64>, &mut gpui_kit::App) + 'static,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let catalog = active_catalog(&cfg);
    let title = catalog.tr(CTX, "Batch Selection").to_owned();
    let ok = catalog.tr(CTX, "OK").to_owned();
    let placeholder = catalog
        .tr(CTX, "Enter line numbers (e.g. 1,3,5-10)")
        .to_owned();
    let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
    let state = std::rc::Rc::new(std::cell::RefCell::new(BatchSelectState::default()));
    let on_accept = std::rc::Rc::new(std::cell::RefCell::new(Some(on_accept)));

    window.open_dialog(cx, move |dialog, _, _| {
        let input_cell = input.clone();
        dialog
            .title(title.clone())
            .w(px(500.))
            .content(move |content, _, _| {
                content.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(Input::new(&input_cell).id("ranges")),
                )
            })
            .footer(
                DialogFooter::new().child(Button::new("confirm").label(ok.clone()).on_click({
                    let state = state.clone();
                    let input = input.clone();
                    let on_accept = on_accept.clone();
                    move |_, window, cx| {
                        let text = input.read(cx).value().to_string();
                        let mut st = state.borrow_mut();
                        st.text = text;
                        match st.numbers() {
                            Ok(nums) => {
                                drop(st);
                                window.close_dialog(cx);
                                if let Some(cb) = on_accept.borrow_mut().take() {
                                    cb(nums, cx);
                                }
                            }
                            Err(e) => {
                                drop(st);
                                window
                                    .push_notification(Notification::error(e.message(catalog)), cx);
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

    fn st(text: &str) -> BatchSelectState {
        BatchSelectState {
            text: text.to_owned(),
        }
    }

    #[test]
    fn parses_plain_and_ranges() {
        assert_eq!(st("1,3,5-7").numbers().unwrap(), vec![1, 3, 5, 6, 7]);
        assert_eq!(st(" 2 , 4 ").numbers().unwrap(), vec![2, 4]);
        assert_eq!(st("10-12").numbers().unwrap(), vec![10, 11, 12]);
    }

    #[test]
    fn empty_input_is_rejected() {
        assert_eq!(st("").numbers(), Err(BatchSelectError::Empty));
        assert_eq!(st("   ").numbers(), Err(BatchSelectError::Empty));
    }

    #[test]
    fn bad_range_is_rejected() {
        // start > end, zero, non-digit — all "Invalid line number range".
        assert_eq!(st("3-1").numbers(), Err(BatchSelectError::InvalidRange));
        assert_eq!(st("0-5").numbers(), Err(BatchSelectError::InvalidRange));
        assert_eq!(st("a-5").numbers(), Err(BatchSelectError::InvalidRange));
        assert_eq!(st("5-").numbers(), Err(BatchSelectError::InvalidRange));
    }

    #[test]
    fn bad_number_is_rejected() {
        assert_eq!(st("x").numbers(), Err(BatchSelectError::InvalidNumber));
        assert_eq!(st("0").numbers(), Err(BatchSelectError::InvalidNumber));
        assert_eq!(st("-2").numbers(), Err(BatchSelectError::InvalidRange));
    }

    #[test]
    fn messages_translate() {
        let c = catalog(ResolvedLanguage::ZhCn);
        assert_eq!(BatchSelectError::Empty.message(c), "请输入行号");
        assert_eq!(
            BatchSelectError::InvalidRange.message(c),
            "行号范围格式错误"
        );
        assert_eq!(BatchSelectError::InvalidNumber.message(c), "行号格式错误");
    }
}
