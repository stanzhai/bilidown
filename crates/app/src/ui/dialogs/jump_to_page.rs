//! Jump-to-page dialog (port of `src/gui/dialog/misc/jump_to_page.py`).
//!
//! Integer-only text box (Python's `QIntValidator(1, 9999)`). The
//! accept just yields the page number; the caller validates it
//! against the real pager bounds and toasts (see the Pager port in
//! `ui/parse/screen.rs`).

use gpui_kit::component::button::Button;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::WindowExt;
use gpui_kit::{div, prelude::*, px, AppContext, ParentElement, Window};

use super::active_catalog;

/// i18n context of the Python dialog class.
const CTX: &str = "JumpToPageDialog";

/// Validator bounds of the Python line edit (`QIntValidator(1, 9999)`).
pub const MIN_PAGE: i64 = 1;
pub const MAX_PAGE: i64 = 9999;

/// Headless view-model: the text box content.
#[derive(Debug, Clone, Default)]
pub struct JumpToPageState {
    pub text: String,
}

impl JumpToPageState {
    /// `accept()`: the page number as an int. `Err` mirrors the
    /// validator rejection (empty, non-numeric, out of 1–9999).
    pub fn page(&self) -> Result<i64, ()> {
        let t = self.text.trim();
        let n: i64 = t.parse().map_err(|_| ())?;
        (MIN_PAGE..=MAX_PAGE).contains(&n).then_some(n).ok_or(())
    }
}

/// Open the dialog; `on_accept` gets the entered page number.
pub fn open_jump_to_page(
    cfg: config::ConfigStore,
    on_accept: impl FnOnce(i64, &mut gpui_kit::App) + 'static,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let catalog = active_catalog(&cfg);
    let title = catalog.tr(CTX, "Jump To Page").to_owned();
    let ok = catalog.tr(CTX, "OK").to_owned();
    let placeholder = catalog.tr(CTX, "Enter page number").to_owned();
    let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
    let on_accept = std::rc::Rc::new(std::cell::RefCell::new(Some(on_accept)));

    window.open_dialog(cx, move |dialog, _, _| {
        let input_cell = input.clone();
        dialog
            .title(title.clone())
            .w(px(350.))
            .content(move |content, _, _| {
                content.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(Input::new(&input_cell).id("page-number")),
                )
            })
            .footer(
                DialogFooter::new().child(Button::new("jump-ok").label(ok.clone()).on_click({
                    let input = input.clone();
                    let on_accept = on_accept.clone();
                    move |_, window, cx| {
                        let state = JumpToPageState {
                            text: input.read(cx).value().to_string(),
                        };
                        match state.page() {
                            Ok(page) => {
                                window.close_dialog(cx);
                                if let Some(cb) = on_accept.borrow_mut().take() {
                                    cb(page, cx);
                                }
                            }
                            Err(()) => {
                                window.push_notification(
                                    Notification::warning("Invalid page number"),
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

    #[test]
    fn parses_integers() {
        assert_eq!(JumpToPageState { text: "3".into() }.page(), Ok(3));
        assert_eq!(JumpToPageState { text: " 7 ".into() }.page(), Ok(7));
        assert_eq!(
            JumpToPageState {
                text: "9999".into()
            }
            .page(),
            Ok(9999)
        );
    }

    #[test]
    fn rejects_out_of_bounds_and_non_numbers() {
        for text in ["", "0", "10000", "-1", "abc", "1.5"] {
            assert_eq!(
                JumpToPageState { text: text.into() }.page(),
                Err(()),
                "{text}"
            );
        }
    }
}
