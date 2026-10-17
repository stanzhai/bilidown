//! OS notifications: category enum + notify request, ported from the
//! Python build's toast/sys-tray layer (`ToastNotificationCategory` in
//! `src/util/common/enum.py`, icon mapping in `sys_tray.py::show_message`).
//!
//! Actual OS dispatch (sys-tray `showMessage` / gpui equivalent) is the UI
//! layer's job; this module supplies the request shape and the category →
//! icon map.

use config::ConfigStore;

/// `ToastNotificationCategory` (serialized values `success`/`error`/...).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastNotificationCategory {
    Success,
    Error,
    Warning,
    Info,
}

impl ToastNotificationCategory {
    /// `sys_tray.py::show_message` icon mapping: Success/Info →
    /// Information, Error → Critical, Warning → Warning.
    pub fn icon(self) -> MessageIcon {
        match self {
            Self::Success | Self::Info => MessageIcon::Information,
            Self::Error => MessageIcon::Critical,
            Self::Warning => MessageIcon::Warning,
        }
    }
}

/// `QSystemTrayIcon.MessageIcon` equivalent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageIcon {
    Information,
    Warning,
    Critical,
}

/// A notification to show: `(category, title, message)` — the exact tuple
/// `signal_bus.toast.sys_show` carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub category: ToastNotificationCategory,
    pub title: String,
    pub message: String,
}

/// `Behavior.show_notification` (default false).
pub fn enabled(cfg: &ConfigStore) -> bool {
    cfg.get("Behavior", "show_notification")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// The download-complete notification the Python build fires when the last
/// active downloader finishes (`DownloaderManager.show_notification`):
/// category INFO, title `TIP_MESSAGES("DOWNLOAD_COMPLETED")`, body
/// `DOWNLOAD_COMPLETED_DETAIL`.
pub fn download_completed(lang: crate::i18n::ResolvedLanguage) -> Notification {
    let c = crate::i18n::catalog(lang);
    Notification {
        category: ToastNotificationCategory::Info,
        title: crate::i18n::tip_messages(c, "DOWNLOAD_COMPLETED")
            .unwrap_or_else(|| "Download completed".into()),
        message: crate::i18n::tip_messages(c, "DOWNLOAD_COMPLETED_DETAIL")
            .unwrap_or_else(|| "All download tasks have been completed.".into()),
    }
}

/// Whether `show_notification` should fire: config gate plus "the active
/// downloader count just hit zero" (`list_view.py:140`).
pub fn should_notify(cfg: &ConfigStore, active_downloads: usize) -> bool {
    enabled(cfg) && active_downloads == 0
}
