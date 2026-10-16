//! Notification tests: category→icon map (sys_tray.py), gate, payload.

use services::notify::{
    download_completed, enabled, should_notify, MessageIcon, Notification, ToastNotificationCategory,
};

#[test]
fn category_to_icon_mapping() {
    use ToastNotificationCategory::*;
    assert_eq!(Success.icon(), MessageIcon::Information);
    assert_eq!(Info.icon(), MessageIcon::Information);
    assert_eq!(Error.icon(), MessageIcon::Critical);
    assert_eq!(Warning.icon(), MessageIcon::Warning);
}

#[test]
fn download_completed_notification_payload() {
    let n = download_completed();
    assert_eq!(
        n,
        Notification {
            category: ToastNotificationCategory::Info,
            title: "Download completed".into(),
            message: "All download tasks have been completed.".into(),
        }
    );
}

#[test]
fn gate_defaults_off_and_fires_only_at_zero() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config::ConfigStore::with_paths(dir.path(), dir.path().join("downloads"));
    assert!(!enabled(&cfg)); // Behavior/show_notification default false
    assert!(!should_notify(&cfg, 0));

    let mut cfg = cfg;
    cfg.set("Behavior", "show_notification", serde_json::Value::Bool(true));
    assert!(should_notify(&cfg, 0));
    assert!(!should_notify(&cfg, 1)); // still-downloading: no notification
    assert!(!should_notify(&cfg, 3));
}
