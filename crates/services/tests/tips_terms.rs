//! Terms gate + teaching tips tests.

use config::ConfigStore;
use services::terms::{accept, gate, TermsGate};
use services::tips::{
    area_dialog_pending, auto_parse_tip_pending, login_tip_pending, mark_area_dialog_shown,
    mark_auto_parse_tip_shown, mark_tutorial_shown, next_tip, tutorial_pending, PendingTip,
};

#[test]
fn terms_gate_pending_until_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = ConfigStore::with_paths(dir.path(), dir.path().join("downloads"));
    assert_eq!(gate(&cfg), TermsGate::Pending); // Application/accepted_terms default false

    let mut cfg = cfg;
    accept(&mut cfg);
    assert_eq!(gate(&cfg), TermsGate::Accepted);

    // persists through a reload (config.json round-trip)
    cfg.save().unwrap();
    let reloaded = ConfigStore::with_paths(dir.path(), dir.path().join("downloads"));
    assert_eq!(gate(&reloaded), TermsGate::Accepted);
}

#[test]
fn tip_flags_default_to_not_shown() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = ConfigStore::with_paths(dir.path(), dir.path().join("downloads"));
    assert!(tutorial_pending(&cfg));
    assert!(area_dialog_pending(&cfg));
    assert!(login_tip_pending(&cfg)); // Cookie/is_login defaults false → logged out
    // first tip in the post-terms order
    assert_eq!(next_tip(&cfg), PendingTip::TutorialDialog);
}

#[test]
fn marks_flip_flags_and_advance_order() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = ConfigStore::with_paths(dir.path(), dir.path().join("downloads"));

    mark_tutorial_shown(&mut cfg);
    assert!(!tutorial_pending(&cfg));
    assert_eq!(next_tip(&cfg), PendingTip::SelectAreaDialog);

    mark_area_dialog_shown(&mut cfg);
    assert!(!area_dialog_pending(&cfg));
    assert_eq!(next_tip(&cfg), PendingTip::LoginTeachingTip);
    // login tip has no shown-flag; it clears when the user logs in
    cfg.set("Cookie", "is_login", serde_json::Value::Bool(true));
    assert!(!login_tip_pending(&cfg));
    assert_eq!(next_tip(&cfg), PendingTip::None);
}

#[test]
fn auto_parse_tip_requires_pagination_and_flags() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = ConfigStore::with_paths(dir.path(), dir.path().join("downloads"));

    // single page → never
    assert!(!auto_parse_tip_pending(&cfg, 1));
    // paginated + first time → show
    assert!(auto_parse_tip_pending(&cfg, 2));
    assert!(auto_parse_tip_pending(&cfg, 30));

    // user turned the "don't show" switch on → never
    let mut cfg = cfg;
    cfg.set("Misc", "show_auto_parse_dialog", serde_json::Value::Bool(true));
    assert!(!auto_parse_tip_pending(&cfg, 2));

    // once shown, never again
    cfg.set("Misc", "show_auto_parse_dialog", serde_json::Value::Bool(false));
    mark_auto_parse_tip_shown(&mut cfg);
    assert!(!auto_parse_tip_pending(&cfg, 2));
}

#[test]
fn tips_persist_through_save_reload() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = ConfigStore::with_paths(dir.path(), dir.path().join("downloads"));
    mark_tutorial_shown(&mut cfg);
    mark_area_dialog_shown(&mut cfg);
    mark_auto_parse_tip_shown(&mut cfg);
    cfg.save().unwrap();

    let reloaded = ConfigStore::with_paths(dir.path(), dir.path().join("downloads"));
    assert!(!tutorial_pending(&reloaded));
    assert!(!area_dialog_pending(&reloaded));
    assert!(!auto_parse_tip_pending(&reloaded, 5));
}
