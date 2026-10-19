use std::rc::Rc;
use std::sync::mpsc::channel;
use std::time::Duration;

use anyhow::Context as _;
// NOTE: no glob imports — `gpui_kit::*`/`gpui_kit::gpui::*` re-export gpui's
// `test` attribute macro, which shadows the builtin `#[test]` and makes every
// plain test fn expand through gpui's macro recursively (compile blowup).
use gpui_kit::gpui::{
    self, point, px, size, AnyWindowHandle, App, AppContext, AsyncApp, Bounds, Global,
    PlatformDisplay, TitlebarOptions, UpdateGlobal, Window, WindowBounds, WindowOptions,
};

mod bootstrap;
mod download_pipeline;
mod pipeline_extras;
mod single_instance;
mod tray;
mod ui;

use config::ConfigStore;
use single_instance::{AppInstance, WakeEvent, ENSURE_RUNNING_FLAG};
pub use ui::{Screen, Shell};

const APP_DISPLAY_NAME: &str = "bilidown";

/// Product identity line used by the About surface and smoke checks.
pub fn version_line() -> String {
    format!("bilidown v{}", env!("CARGO_PKG_VERSION"))
}

/// Live displays as `services::window_state::ScreenRect`s.
fn display_rect(d: &Rc<dyn PlatformDisplay>) -> services::window_state::ScreenRect {
    let b = d.bounds();
    services::window_state::ScreenRect {
        x: b.origin.x.as_f32() as i64,
        y: b.origin.y.as_f32() as i64,
        width: b.size.width.as_f32() as i64,
        height: b.size.height.as_f32() as i64,
    }
}

/// Loaded config store shared across the app.
struct ConfigGlobal(parking_lot::Mutex<ConfigStore>);

impl Global for ConfigGlobal {}

/// Save the window geometry (Python `save_window_state`), then flush config.
fn persist_window_state(window: &mut Window, cx: &mut App) {
    let Some(global) = cx.try_global::<ConfigGlobal>() else {
        return;
    };
    let mut store = global.0.lock();
    let b = window.bounds();
    let state = services::window_state::WindowState {
        x: b.origin.x.as_f32() as i64,
        y: b.origin.y.as_f32() as i64,
        width: b.size.width.as_f32() as i64,
        height: b.size.height.as_f32() as i64,
        maximized: window.is_maximized(),
    };
    services::window_state::save_state(&mut store, true, state, None);
    let _ = store.save();
}

fn active_catalog(cx: &App) -> &'static services::i18n::Catalog {
    let lang = cx
        .try_global::<ConfigGlobal>()
        .map(|g| {
            services::i18n::Language::from_config(
                &g.0.lock()
                    .get("Interface", "language")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default(),
            )
            .resolve()
        })
        .unwrap_or_else(|| services::i18n::Language::from_config("").resolve());
    services::i18n::catalog(lang)
}

/// Apply `QFluentWidgets.ThemeMode` to the kit theme: Light/Dark load that
/// mode directly; Auto re-resolves against the OS appearance (the service
/// layer cannot see it — unknown appearance defaults to Light).
fn apply_configured_theme(cfg: &ConfigStore, cx: &mut App) {
    use gpui_kit::component::theme::{Theme, ThemeMode};
    match services::theme::ThemeModeSetting::from_config(
        &cfg.get("QFluentWidgets", "ThemeMode")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "Auto".into()),
    ) {
        services::theme::ThemeModeSetting::Light => Theme::change(ThemeMode::Light, None, cx),
        services::theme::ThemeModeSetting::Dark => Theme::change(ThemeMode::Dark, None, cx),
        services::theme::ThemeModeSetting::Auto => Theme::sync_system_appearance(None, cx),
    }
}

/// Window bounds for startup: saved state (F2) when
/// `Behavior/window_state` is remembered and validates; otherwise the
/// first-run default (2/3 of the main display, centered, ≥ 950×600).
fn startup_bounds(cfg: &ConfigStore, cx: &App) -> Option<WindowBounds> {
    let screens = cx.displays().iter().map(display_rect).collect::<Vec<_>>();
    let state = services::window_state::saved_state(cfg, &screens).or_else(|| {
        // First-run fallback (37): 2/3 of the main display, centered.
        cx.primary_display()
            .map(|d| services::window_state::default_state(display_rect(&d)))
    })?;
    let bounds = Bounds {
        origin: point(px(state.x as f32), px(state.y as f32)),
        size: size(px(state.width as f32), px(state.height as f32)),
    };
    Some(if state.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    })
}

/// Opens one native window; returns when the app event loop exits cleanly.
pub fn run() -> anyhow::Result<()> {
    let ensure_running = std::env::args().any(|a| a == ENSURE_RUNNING_FLAG);

    // Terms gate (F2): first launch checks before any UI init.
    let cfg =
        config::ConfigStore::open().unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
    if services::terms::gate(&cfg) == services::terms::TermsGate::Pending {
        // No UI toolkit is up yet; Python blocks startup on the dialog.
        // Terminal ack keeps the shipped path real until the dialog lands.
        eprintln!("Terms of use not accepted; see config Application/accepted_terms.");
    }

    // --- single instance gate, before any UI -----------------------------
    let (wake_tx, wake_rx) = channel::<WakeEvent>();
    let (tray_tx, tray_rx) = channel::<tray::TrayAction>();
    // The guard owns the lock file + Windows mutex for the whole process;
    // held on this stack frame until `run` returns, then dropped below
    // (Python's `instance_lock.unlock()` on quit).
    let mut guard = match AppInstance::acquire(ensure_running, wake_tx)
        .context("single-instance gate failed")?
    {
        Ok(guard) => guard,
        // Secondary launch: the primary was woken (or is dead but
        // unwakeable, same as Python logging and exiting 0).
        Err(_) => return Ok(()),
    };
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            // Must be called before using any GPUI Component features.
            gpui_kit::init(cx);

            let cfg = ConfigStore::open().unwrap_or_else(|_| {
                ConfigStore::with_paths(std::env::temp_dir().join("bilidown"), std::env::temp_dir())
            });
            // Apply the configured theme before any window exists: the kit
            // default is Light, so a Dark `QFluentWidgets.ThemeMode` must be
            // loaded here. Auto re-resolves against the OS appearance
            // (`theme_from_config` cannot see it and defaults to Light).
            apply_configured_theme(&cfg, cx);
            ConfigGlobal::set_global(cx, ConfigGlobal(parking_lot::Mutex::new(cfg)));
            let cat = active_catalog(cx);

            // --- tray ------------------------------------------------------
            let show_label = cat.tr("SystemTrayIcon", "Show main window").to_string();
            let exit_label = cat.tr("SystemTrayIcon", "Exit").to_string();
            let mut _tray = tray::spawn_tray(&show_label, &exit_label, APP_DISPLAY_NAME, tray_tx);

            // --- window (titlebar/min-size + F2 saved bounds or 2/3 default) --
            let mut options = WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some(APP_DISPLAY_NAME.into()),
                    ..Default::default()
                }),
                window_min_size: Some(gpui::Size {
                    width: px(950.),
                    height: px(600.),
                }),
                app_id: Some("cn.stanzhai.bilidown".to_string()),
                ..Default::default()
            };
            let cfg =
                config::ConfigStore::open().unwrap_or_else(|_| ConfigStore::with_paths("", ""));
            if let Some(bounds) = startup_bounds(&cfg, cx) {
                options.window_bounds = Some(bounds);
            }

            let (window, shell) = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| Shell::new(window, cx))
            })
            .expect("Failed to open window");

            // --- close → OS default: window close exits the app ----------
            // `on_window_should_close` returning true lets the platform
            // close the window (quit behavior follows the OS default);
            // the only app work left is persisting window state.
            window
                .update(cx, |_, window, cx| {
                    window.on_window_should_close(cx, |window, cx| {
                        persist_window_state(window, cx);
                        true
                    });
                })
                .expect("window gone");

            // --- tray + wake pump ------------------------------------------
            // tray-icon/muda deliver on background threads; drain both
            // channels from a foreground task so all UI work stays on the
            // main loop.
            cx.spawn(async move |cx| loop {
                while let Ok(ev) = tray_rx.try_recv() {
                    handle_tray_action(ev, window, cx).await;
                }
                while let Ok(ev) = wake_rx.try_recv() {
                    handle_wake_event(ev, window, cx).await;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
            })
            .detach();
            Shell::boot(&shell, cx);
        });
    // Python `cleanup()` → `instance_lock.unlock()`: the lock file is gone
    // before this returns, so an immediate relaunch opens a window.
    guard.release();
    Ok(())
}

/// Bring the window back from tray/minimize (Python `_activate_window`).
fn activate_main_window(window: AnyWindowHandle, cx: &mut App) {
    cx.activate(true);
    let _ = window.update(cx, |_, window, _| window.activate_window());
}

async fn handle_tray_action(action: tray::TrayAction, window: AnyWindowHandle, cx: &AsyncApp) {
    cx.update(|cx| match action {
        tray::TrayAction::Show => activate_main_window(window, cx),
        tray::TrayAction::Exit => {
            let _ = window.update(cx, |_, window, cx| persist_window_state(window, cx));
            cx.quit();
        }
    });
}

async fn handle_wake_event(ev: WakeEvent, window: AnyWindowHandle, cx: &AsyncApp) {
    match ev {
        WakeEvent::Activate => {
            cx.update(|cx| activate_main_window(window, cx));
        }
        // Python: probe only, never steals focus.
        WakeEvent::EnsureRunning => {}
    }
}

fn main() {
    run().expect("app failed");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_line_reports_app_identity() {
        // Product renamed bilidown; version line continues the v2.21.0 line.
        assert_eq!(
            crate::version_line(),
            format!("bilidown v{}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn lock_file_lives_under_app_data() {
        let p = single_instance::lock_path().expect("data dir on all supported platforms");
        assert!(p.ends_with(single_instance::INSTANCE_LOCK_NAME));
    }

    #[test]
    fn secondary_launch_wakes_primary_over_socket() {
        use single_instance::*;
        let (tx, rx) = channel();
        start_wake_server(tx).expect("wake server binds");
        assert!(wake_existing_instance(COMMAND_ACTIVATE));
        let ev = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("primary received wake");
        assert_eq!(ev, WakeEvent::Activate);
        assert!(wake_existing_instance(COMMAND_ENSURE_RUNNING));
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            WakeEvent::EnsureRunning
        );
    }

    /// Startup applies the stored theme string through the kit theme (Light/
    /// Dark load directly; Auto follows the OS appearance). Checks the
    /// mapping path end-to-end on a live app without opening a window.
    #[test]
    fn configured_theme_string_maps_to_kit_mode() {
        use gpui_kit::component::theme::{Theme, ThemeMode};
        use gpui_kit::gpui::TestApp;

        let cases = [
            ("Light", ThemeMode::Light),
            ("Dark", ThemeMode::Dark),
            ("Auto", ThemeMode::Light), // test platform reports Light
            ("", ThemeMode::Light),     // unknown -> Auto -> Light
        ];
        for (stored, want) in cases {
            let mut app = TestApp::new();
            app.update(|cx| {
                gpui_kit::init(cx);
                let mut cfg = ConfigStore::with_paths("", "");
                cfg.set("QFluentWidgets", "ThemeMode", serde_json::json!(stored));
                apply_configured_theme(&cfg, cx);
                assert_eq!(Theme::global(cx).mode, want, "stored {stored:?}");
            });
        }
    }
}
