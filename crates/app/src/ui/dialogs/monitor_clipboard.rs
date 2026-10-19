//! Monitor-clipboard dialog (port of
//! `src/gui/dialog/setting/monitor_clipboard.py`): toggles
//! `Behavior.monitor_clipboard` and
//! `Behavior.show_download_confirmation_dialog`. When monitoring is off,
//! the confirmation toggle is forced off and disabled (`on_monitor_clipboard`).

use serde_json::json;

use gpui_kit::base::Disableable as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::label::Label;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::WindowExt;
use gpui_kit::{div, prelude::*, px, ParentElement, Window};

use super::active_catalog;

const CTX: &str = "MonitorClipboardDialog";

/// Headless state: the two toggles plus the coupled-enable rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MonitorClipboardState {
    pub monitor_clipboard: bool,
    pub show_download_confirmation_dialog: bool,
}

impl MonitorClipboardState {
    pub fn load(cfg: &config::ConfigStore) -> Self {
        let b = |item: &str| {
            cfg.get("Behavior", item)
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        };
        Self {
            monitor_clipboard: b("monitor_clipboard"),
            show_download_confirmation_dialog: b("show_download_confirmation_dialog"),
        }
    }

    /// `on_monitor_clipboard`: turning monitoring off also turns the
    /// confirmation dialog off and disables its row.
    pub fn set_monitor_clipboard(&mut self, on: bool) {
        self.monitor_clipboard = on;
        if !on {
            self.show_download_confirmation_dialog = false;
        }
    }

    /// `accept()`: write both keys.
    pub fn apply(&self, cfg: &mut config::ConfigStore) {
        cfg.set(
            "Behavior",
            "monitor_clipboard",
            json!(self.monitor_clipboard),
        );
        cfg.set(
            "Behavior",
            "show_download_confirmation_dialog",
            json!(self.show_download_confirmation_dialog),
        );
    }
}

/// Open the dialog (Python `ParsingSettingCard` →
/// `custom_monitor_clipboard_btn`).
pub fn open_monitor_clipboard(
    cfg: config::ConfigStore,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let catalog = active_catalog(&cfg);
    let title = catalog.tr(CTX, "Monitor Clipboard Settings").to_owned();
    let monitor_label = catalog.tr(CTX, "Monitor Clipboard").to_owned();
    // Qt showed on/off description lines under each switch; one static
    // hint per switch keeps the same information without the pair.
    let monitor_hint = catalog
        .tr(
            CTX,
            "On - Automatically start parsing when a link is copied",
        )
        .to_owned();
    let confirm_label = catalog
        .tr(CTX, "Automatically Show Download Confirmation Dialog")
        .to_owned();
    let confirm_hint = catalog
        .tr(CTX, "On - Show the dialog after parsing a link")
        .to_owned();
    let about = catalog.tr(CTX, "About this option").to_owned();
    let instructions = catalog.tr(CTX, "Instructions").to_owned();
    let guide = catalog
        .tr(
            CTX,
            "This feature needs to be used in conjunction with the \"Automatic Download Item Selection Settings\". When enabled, if no items are selected in the list after parsing is complete, the download confirmation dialog will not pop up.",
        )
        .to_owned();
    let ok = "OK".to_owned();

    let state = std::rc::Rc::new(std::cell::RefCell::new(MonitorClipboardState::load(&cfg)));
    let cfg_cell = std::rc::Rc::new(std::cell::RefCell::new(cfg));

    window.open_dialog(cx, move |dialog, _, _| {
        // `open_dialog` takes `Fn` (may re-run the builder); clone every
        // capture here so the inner `move` closures take locals, not the
        // outer closure's own captures.
        let (monitor_label, monitor_hint, confirm_label, confirm_hint, about, instructions, guide) = (
            monitor_label.clone(),
            monitor_hint.clone(),
            confirm_label.clone(),
            confirm_hint.clone(),
            about.clone(),
            instructions.clone(),
            guide.clone(),
        );
        let content_state = state.clone();
        let footer_state = state.clone();
        dialog
            .title(title.clone())
            .w(px(480.))
            .content({
                let state = content_state.clone();
                move |content, _, _| {
                    let st = *state.borrow();
                    content.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_4()
                            .child(
                                div().flex().flex_col().gap_1().child(
                                    Switch::new("monitor-clipboard")
                                        .checked(st.monitor_clipboard)
                                        .label(monitor_label.clone())
                                        .on_change({
                                            let state = state.clone();
                                            move |v, _, _| {
                                                state.borrow_mut().set_monitor_clipboard(*v);
                                            }
                                        }),
                                ),
                            )
                            .child(Label::new(monitor_hint.clone()))
                            .child(
                                div().flex().flex_col().gap_1().child(
                                    Switch::new("download-confirmation")
                                        .checked(st.show_download_confirmation_dialog)
                                        .disabled(!st.monitor_clipboard)
                                        .label(confirm_label.clone())
                                        .on_change({
                                            let state = state.clone();
                                            move |v, _, _| {
                                                state
                                                    .borrow_mut()
                                                    .show_download_confirmation_dialog = *v;
                                            }
                                        }),
                                ),
                            )
                            .child(Label::new(confirm_hint.clone()))
                            .child(
                                Button::new("about-option")
                                    .label(about.clone())
                                    .outline()
                                    .on_click({
                                        let instructions = instructions.clone();
                                        let guide = guide.clone();
                                        move |_, window, cx| {
                                            let title = instructions.clone();
                                            let desc = guide.clone();
                                            window.open_alert_dialog(cx, move |alert, _, _| {
                                                alert
                                                    .title(title.clone())
                                                    .description(desc.clone())
                                                    .show_cancel(false)
                                            });
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
            "t31-monitor-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        config::ConfigStore::with_paths(&dir, "")
    }

    #[test]
    fn turning_monitor_off_forces_confirmation_off() {
        let mut st = MonitorClipboardState {
            monitor_clipboard: true,
            show_download_confirmation_dialog: true,
        };
        st.set_monitor_clipboard(false);
        assert!(!st.show_download_confirmation_dialog);

        let mut cfg = scratch_cfg();
        st.apply(&mut cfg);
        assert_eq!(
            cfg.get("Behavior", "monitor_clipboard")
                .and_then(|v| v.as_bool()),
            Some(false)
        );
        assert_eq!(
            cfg.get("Behavior", "show_download_confirmation_dialog")
                .and_then(|v| v.as_bool()),
            Some(false)
        );
        assert_eq!(MonitorClipboardState::load(&cfg), st);
    }

    #[test]
    fn defaults_load_false() {
        let cfg = scratch_cfg();
        assert_eq!(
            MonitorClipboardState::load(&cfg),
            MonitorClipboardState::default()
        );
    }
}
