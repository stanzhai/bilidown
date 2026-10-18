//! System tray (port of `src/gui/component/sys_tray.py` + `src/gui/app/tray.py`).
//!
//! Menu: "Show main window", separator, "Exit" (Python `SystemTrayIcon` with
//! `Show`/`Quit` actions). Left-click/`Trigger` activates the main window.
//! Events are forwarded through a channel that the GPUI main loop drains; the
//! tray lives for the whole process (dropped only on exit, matching Python).

use std::sync::mpsc::{Receiver, Sender};

use muda::accelerator::Accelerator;
use muda::{Menu, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent, menu::MenuEvent};

pub const MENU_ID_SHOW: &str = "show";
pub const MENU_ID_EXIT: &str = "exit";
const TRAY_ID: &str = "bili23-tray";

/// 128×128 RGBA raster of `assets/app.svg` (qlmanage-rendered), embedded so
/// the tray does not depend on runtime asset loading. Decompressed once.
static APP_ICON_PNG: &[u8] = include_bytes!("../assets/tray-icon.png");

/// What the tray asked the app to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    /// Menu "Show main window" or a left-click `Trigger`.
    Show,
    /// Menu "Exit" — force-close path (no exit dialog).
    Exit,
    /// Window-close dialog chose "minimize to tray" (not a tray pick).
    ShowMinimize,
}

/// Map a menu id to its action; `None` for anything else.
pub fn action_for_menu_id(id: &str) -> Option<TrayAction> {
    match id {
        MENU_ID_SHOW => Some(TrayAction::Show),
        MENU_ID_EXIT => Some(TrayAction::Exit),
        _ => None,
    }
}

/// Map a tray-icon click to an action. Qt `QSystemTrayIcon.Trigger` ≈ a
/// completed left click; tray-icon reports that as Left+Up.
pub fn action_for_click(button: tray_icon::MouseButton, state: tray_icon::MouseButtonState) -> Option<TrayAction> {
    use tray_icon::MouseButton::*;
    use tray_icon::MouseButtonState::*;
    match (button, state) {
        (Left, Up) => Some(TrayAction::Show),
        _ => None,
    }
}

fn decode_icon() -> Option<Icon> {
    let img = image::load_from_memory(APP_ICON_PNG).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).ok()
}

/// Create the tray icon and start pumping its events into `tx`.
/// Returns the owned icon handle; keep it alive for the process lifetime.
pub fn spawn_tray(show_label: &str, exit_label: &str, tooltip: &str, tx: Sender<TrayAction>) -> Option<TrayIcon> {
    let menu_tx = tx.clone();
    let icon = decode_icon()?;
    let tray = TrayIconBuilder::new()
        .with_id(TRAY_ID)
        .with_menu(Box::new(tray_menu(show_label, exit_label)))
        .with_tooltip(tooltip)
        .with_icon(icon)
        .build()
        .ok()?;

    // Menu picks: muda's global receiver, forwarded from a plain thread.
    std::thread::Builder::new()
        .name("tray-menu-pump".into())
        .spawn(move || {
            for ev in MenuEvent::receiver().into_iter() {
                if let Some(action) = action_for_menu_id(ev.id().as_ref()) {
                    if menu_tx.send(action).is_err() {
                        return;
                    }
                }
            }
        })
        .ok();

    // Tray clicks: install a handler that mirrors picks into the channel.
    tray_icon::TrayIconEvent::set_event_handler(Some(move |ev: TrayIconEvent| {
        let (button, state) = match &ev {
            TrayIconEvent::Click { button, button_state, .. } => (*button, *button_state),
            _ => return,
        };
        if let Some(action) = action_for_click(button, state) {
            let _ = tx.send(action);
        }
    }));

    Some(tray)
}

fn tray_menu(show_label: &str, exit_label: &str) -> Menu {
    let show = MenuItem::with_id(MENU_ID_SHOW, show_label, true, None::<Accelerator>);
    let exit = MenuItem::with_id(MENU_ID_EXIT, exit_label, true, None::<Accelerator>);
    let menu = Menu::new();
    menu.append_items(&[&show, &PredefinedMenuItem::separator(), &exit]).unwrap();
    menu
}

/// Block until the next tray action (test helper).
#[cfg(test)]
pub(crate) fn next_action(rx: &Receiver<TrayAction>) -> Option<TrayAction> {
    rx.recv().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_map_to_actions() {
        assert_eq!(action_for_menu_id(MENU_ID_SHOW), Some(TrayAction::Show));
        assert_eq!(action_for_menu_id(MENU_ID_EXIT), Some(TrayAction::Exit));
        assert_eq!(action_for_menu_id("other"), None);
    }

    #[test]
    fn trigger_maps_left_click_release_only() {
        use tray_icon::MouseButton::*;
        use tray_icon::MouseButtonState::*;
        assert_eq!(action_for_click(Left, Up), Some(TrayAction::Show));
        assert_eq!(action_for_click(Left, Down), None);
        assert_eq!(action_for_click(Right, Up), None);
    }

    #[test]
    fn embedded_icon_decodes() {
        decode_icon().expect("embedded PNG must decode into a tray Icon");
    }

    #[test]
    fn channel_round_trip() {
        use std::sync::mpsc::channel;
        let (tx, rx) = channel();
        tx.send(TrayAction::Exit).unwrap();
        assert_eq!(next_action(&rx), Some(TrayAction::Exit));
    }
}
