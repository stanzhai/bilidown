//! Download-as-single-video confirm dialog (port of
//! `src/gui/dialog/misc/download_as_single_video.py`).
//!
//! Informational body plus a "Don't show this again" checkbox
//! persisted to `Behavior/show_download_as_single_video_dialog`
//! (`config.set(show_download_as_single_video_dialog,
//! not never_show_check.isChecked())`). Python hides the cancel
use super::active_catalog;
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::label::Label;
use gpui_kit::component::WindowExt;
use gpui_kit::{div, prelude::*, px, ParentElement, Window};

/// i18n context of the Python dialog class.
const CTX: &str = "DownloadAsSingleVideoDialog";

/// Behavior flag the checkbox writes.
const FLAG_GROUP: &str = "Behavior";
const FLAG_KEY: &str = "show_download_as_single_video_dialog";

/// Whether the confirm dialog should show before a single-video
/// download (default true, as in Python).
pub fn show_dialog_flag(cfg: &config::ConfigStore) -> bool {
    cfg.get(FLAG_GROUP, FLAG_KEY)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true)
}

/// Open the dialog. `on_accept` fires after the flag is persisted.
pub fn open_download_as_single_video(
    cfg: config::ConfigStore,
    on_accept: impl FnOnce(&mut gpui_kit::App) + 'static,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let catalog = active_catalog(&cfg);
    let title = catalog.tr(CTX, "Download as Single Video").to_owned();
    let desc = catalog
        .tr(
            CTX,
            "This will save the file directly using the video's title as its name, without applying your configured file naming rules.",
        )
        .to_owned();
    let never_label = catalog.tr(CTX, "Don't show this again").to_owned();

    let never_show = std::rc::Rc::new(std::cell::RefCell::new(false));
    let cfg_path = cfg
        .path()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();
    let on_accept = std::rc::Rc::new(std::cell::RefCell::new(Some(on_accept)));

    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title(title.clone())
            .w(px(420.))
            .content({
                let desc = desc.clone();
                let never_label = never_label.clone();
                let never_show = never_show.clone();
                move |content, _, _| {
                    content.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(Label::new(desc.clone()))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(Checkbox::new("never-show").on_click({
                                        let never_show = never_show.clone();
                                        move |checked, _, _| {
                                            *never_show.borrow_mut() = *checked;
                                        }
                                    }))
                                    .child(Label::new(never_label.clone())),
                            ),
                    )
                }
            })
            .footer(
                DialogFooter::new().child(Button::new("dasv-ok").label("OK").on_click({
                    let never_show = never_show.clone();
                    let cfg_path = cfg_path.clone();
                    let on_accept = on_accept.clone();
                    move |_, window, cx| {
                        let checked = *never_show.borrow();
                        let mut cfg = config::ConfigStore::open().unwrap_or_else(|_| {
                            config::ConfigStore::with_paths(cfg_path.clone(), "")
                        });
                        cfg.set(FLAG_GROUP, FLAG_KEY, serde_json::Value::Bool(!checked));
                        let _ = cfg.save();
                        window.close_dialog(cx);
                        if let Some(cb) = on_accept.borrow_mut().take() {
                            cb(cx);
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
    fn flag_defaults_to_true() {
        let cfg = config::ConfigStore::with_paths("", "");
        assert!(show_dialog_flag(&cfg));
    }
}
