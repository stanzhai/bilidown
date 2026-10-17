//! Terms-of-use gate dialog (ticket 31 F2): first-launch modal that
//! persists `Application/accepted_terms` on accept and closes the app on
//! reject. Port of `src/gui/dialog/main_window/terms.py`; the terms text
//! comes from the i18n catalog (`Translator.TERMS_OF_USE()`).

use gpui_kit::component::button::Button;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::WindowExt;
use gpui_kit::{div, prelude::*, px, App, ParentElement, Window};

use services::i18n;

use super::active_catalog;

/// Open the gate. `on_exit` runs when the user rejects (Python closes the
/// window); `on_accept` runs after the flag is persisted.
pub fn open_terms_gate(
    cfg: config::ConfigStore,
    on_accept: impl FnOnce(&mut Window, &mut App) + 'static,
    on_exit: impl FnOnce(&mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let catalog = active_catalog(&cfg);
    let title = catalog.tr("TermsDialog", "Terms of Use").to_owned();
    let body = std::rc::Rc::new(i18n::terms_of_use(catalog));
    let cfg = std::rc::Rc::new(cfg);
    let accept_label = catalog.tr("TermsDialog", "Accept").to_owned();
    let reject_label = catalog.tr("TermsDialog", "Exit").to_owned();
    let on_accept = std::rc::Rc::new(std::cell::RefCell::new(Some(on_accept)));
    let on_exit = std::rc::Rc::new(std::cell::RefCell::new(Some(on_exit)));

    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title(title.clone())
            .content({
                let body = body.clone();
                move |content, _, _| {
                    content.child(
                        div()
                            .id("terms-body")
                            .flex()
                            .flex_col()
                            .gap_2()
                            .max_h(px(400.))
                            .overflow_y_scroll()
                            .child(body.clone().to_string()),
                    )
                }
            })
            .footer(
                DialogFooter::new()
                    .child(Button::new("reject").label(reject_label.clone()).on_click({
                        let on_exit = on_exit.clone();
                        move |_, window, cx| {
                            window.close_dialog(cx);
                            if let Some(cb) = on_exit.borrow_mut().take() {
                                cb(window, cx);
                            }
                        }
                    }))
                    .child(Button::new("accept").label(accept_label.clone()).on_click({
                        let cfg = cfg.clone();
                        let on_accept = on_accept.clone();
                        move |_, window, cx| {
                            let mut cfg = config::ConfigStore::open().unwrap_or_else(|_| {
                                config::ConfigStore::with_paths(
                                    cfg.path()
                                        .parent()
                                        .map(|p| p.to_path_buf())
                                        .unwrap_or_default(),
                                    "",
                                )
                            });
                            services::terms::accept(&mut cfg);
                            let _ = cfg.save();
                            window.close_dialog(cx);
                            if let Some(cb) = on_accept.borrow_mut().take() {
                                cb(window, cx);
                            }
                        }
                    })),
            )
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn accept_persists_the_flag() {
        let mut cfg = config::ConfigStore::with_paths("", "");
        assert_eq!(
            services::terms::gate(&cfg),
            services::terms::TermsGate::Pending
        );
        services::terms::accept(&mut cfg);
        assert_eq!(
            services::terms::gate(&cfg),
            services::terms::TermsGate::Accepted
        );
    }
}
