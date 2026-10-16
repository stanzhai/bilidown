//! Window-state tests: restore validation and save guards, ported from
//! `MainWindow.get_saved_window_state` / `save_window_state`.

use config::ConfigStore;
use services::window_state::{
    save_state, saved_state, ScreenRect, WindowState, DEFAULT_HEIGHT, DEFAULT_WIDTH,
};

fn full_screen() -> Vec<ScreenRect> {
    vec![ScreenRect { x: 0, y: 0, width: 2560, height: 1440 }]
}

fn state(x: i64, y: i64, w: i64, h: i64, maximized: bool) -> WindowState {
    WindowState { x, y, width: w, height: h, maximized }
}

fn remember_store(dir: &std::path::Path, remember: bool, state: serde_json::Value) -> ConfigStore {
    let mut cfg = ConfigStore::with_paths(dir, dir.join("downloads"));
    cfg.set("Behavior", "remember_window_state", serde_json::Value::Bool(remember));
    cfg.set("Behavior", "window_state", state);
    cfg
}

#[test]
fn restore_requires_remember_flag() {
    let dir = tempfile::tempdir().unwrap();
    let good = serde_json::json!({"x": 10, "y": 20, "width": 1200, "height": 800, "maximized": false});
    let cfg = remember_store(dir.path(), false, good.clone());
    assert_eq!(saved_state(&cfg, &full_screen()), None);
    let cfg = remember_store(dir.path(), true, good);
    assert_eq!(
        saved_state(&cfg, &full_screen()),
        Some(state(10, 20, 1200, 800, false))
    );
}

#[test]
fn restore_rejects_non_object_and_non_int() {
    let dir = tempfile::tempdir().unwrap();
    for bad in [
        serde_json::json!(null),
        serde_json::json!("oops"),
        serde_json::json!({"x": "abc", "y": 0, "width": 1000, "height": 700}),
        serde_json::json!({"x": 0, "y": 0}), // missing size
    ] {
        let cfg = remember_store(dir.path(), true, bad.clone());
        assert_eq!(saved_state(&cfg, &full_screen()), None, "should reject {bad}");
    }
}

#[test]
fn restore_enforces_minimum_size() {
    let dir = tempfile::tempdir().unwrap();
    let mk = |w: i64, h: i64| {
        remember_store(
            dir.path(),
            true,
            serde_json::json!({"x": 0, "y": 0, "width": w, "height": h}),
        )
    };
    assert_eq!(saved_state(&mk(949, 600), &full_screen()), None);
    assert_eq!(saved_state(&mk(950, 599), &full_screen()), None);
    assert_eq!(saved_state(&mk(0, 0), &full_screen()), None); // "no record yet"
    assert_eq!(saved_state(&mk(950, 600), &full_screen()), Some(state(0, 0, 950, 600, false)));
    assert_eq!(DEFAULT_WIDTH, 950);
    assert_eq!(DEFAULT_HEIGHT, 600);
}

#[test]
fn restore_requires_title_bar_on_a_screen() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = |x: i64, y: i64, w: i64| {
        remember_store(
            dir.path(),
            true,
            serde_json::json!({"x": x, "y": y, "width": w, "height": 800}),
        )
    };
    // on the only screen
    assert!(saved_state(&cfg(100, 100, 1000), &full_screen()).is_some());
    // just off the left edge but title bar still overlaps → visible
    assert!(saved_state(&cfg(-500, 100, 1000), &full_screen()).is_some());
    // fully off screen → rejected
    assert_eq!(saved_state(&cfg(3000, 100, 1000), &full_screen()), None);
    // below every screen → rejected
    assert_eq!(saved_state(&cfg(100, 5000, 1000), &full_screen()), None);

    // second screen scenario: title bar intersects the second monitor
    let screens = vec![
        ScreenRect { x: 0, y: 0, width: 1440, height: 900 },
        ScreenRect { x: 1440, y: 0, width: 1920, height: 1080 },
    ];
    let cfg = remember_store(
        dir.path(),
        true,
        serde_json::json!({"x": 2000, "y": 100, "width": 1000, "height": 800}),
    );
    assert!(saved_state(&cfg, &screens).is_some());
}

#[test]
fn maximized_flag_round_trips_and_defaults_false() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = remember_store(
        dir.path(),
        true,
        serde_json::json!({"x": 1, "y": 2, "width": 1000, "height": 700, "maximized": true}),
    );
    assert!(saved_state(&cfg, &full_screen()).unwrap().maximized);

    let cfg = remember_store(
        dir.path(),
        true,
        serde_json::json!({"x": 1, "y": 2, "width": 1000, "height": 700}),
    );
    assert!(!saved_state(&cfg, &full_screen()).unwrap().maximized);
}

#[test]
fn save_guards_and_persist() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = remember_store(dir.path(), true, serde_json::json!({}));

    // remember off → no-op
    let mut off = remember_store(dir.path(), false, serde_json::json!({}));
    save_state(&mut off, true, state(10, 10, 1000, 700, false), None);
    assert!(off.get("Behavior", "window_state").unwrap().as_object().unwrap().is_empty());

    // never shown (silent start) → no-op
    save_state(&mut cfg, false, state(10, 10, 1000, 700, false), None);
    assert!(cfg.get("Behavior", "window_state").unwrap().as_object().unwrap().is_empty());

    // shown + valid → persisted
    save_state(&mut cfg, true, state(10, 20, 1000, 700, false), None);
    assert_eq!(saved_state(&cfg, &full_screen()), Some(state(10, 20, 1000, 700, false)));

    // maximized saves the last normal geometry, keeps the flag
    save_state(&mut cfg, true, state(0, 0, 2560, 1440, true), Some(state(10, 20, 1000, 700, false)));
    assert_eq!(
        saved_state(&cfg, &full_screen()),
        Some(state(10, 20, 1000, 700, true))
    );

    // non-positive size → no write
    let mut cfg2 = remember_store(dir.path(), true, serde_json::json!({}));
    save_state(&mut cfg2, true, state(10, 10, 0, 700, false), None);
    assert!(cfg2.get("Behavior", "window_state").unwrap().as_object().unwrap().is_empty());
}

#[test]
fn save_then_reload_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = remember_store(dir.path(), true, serde_json::json!({}));
    save_state(&mut cfg, true, state(42, 24, 1280, 720, false), None);
    cfg.save().unwrap();
    let reloaded = ConfigStore::with_paths(dir.path(), dir.path().join("downloads"));
    assert_eq!(saved_state(&reloaded, &full_screen()), Some(state(42, 24, 1280, 720, false)));
}
