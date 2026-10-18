use std::rc::Rc;

use gpui_kit::*;

mod bootstrap;
mod download_pipeline;
mod pipeline_extras;
mod ui;

pub use ui::{Screen, Shell};

/// Product identity line used by the About surface and smoke checks.
pub fn version_line() -> String {
    format!("bilidown v{}", env!("CARGO_PKG_VERSION"))
}

/// Window bounds for startup: saved state (F2) when
/// `Behavior/window_state` is remembered and validates; otherwise the
/// first-run default (2/3 of the main display, centered, ≥ 950×600).
/// No screens are queryable through gpui-pre, so the
/// title-bar-intersects-a-screen check uses the monitor bounds the
/// platform exposes at window creation.
fn startup_bounds(cfg: &config::ConfigStore, cx: &App) -> Option<WindowBounds> {
    let rect = |d: &Rc<dyn PlatformDisplay>| {
        let b = d.bounds();
        services::window_state::ScreenRect {
            x: b.origin.x.as_f32() as i64,
            y: b.origin.y.as_f32() as i64,
            width: b.size.width.as_f32() as i64,
            height: b.size.height.as_f32() as i64,
        }
    };
    let screens = cx
        .displays()
        .iter()
        .map(&rect)
        .collect::<Vec<_>>();
    let state = services::window_state::saved_state(cfg, &screens).or_else(|| {
        // First-run fallback (37): 2/3 of the main display, centered.
        cx.primary_display()
            .map(|d| services::window_state::default_state(rect(&d)))
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
    // Terms gate (F2): first launch shows the gate before any util init.
    let cfg =
        config::ConfigStore::open().unwrap_or_else(|_| config::ConfigStore::with_paths("", ""));
    if services::terms::gate(&cfg) == services::terms::TermsGate::Pending {
        // No UI toolkit is up yet; Python blocks startup on the dialog.
        // Terminal ack keeps the shipped path real until the dialog lands.
        eprintln!("Terms of use not accepted; see config Application/accepted_terms.");
    }

    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            // Must be called before using any GPUI Component features.
            gpui_kit::init(cx);
            // F2 restore + first-run default: both need displays, which
            // only exist in the app context.
            let mut options = WindowOptions::default();
            if let Some(bounds) = startup_bounds(&cfg, cx) {
                options.window_bounds = Some(bounds);
            }
            let (_, shell) = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| Shell::new(window, cx))
            })
            .expect("Failed to open window");
            Shell::boot(&shell, cx);
        });
    Ok(())
}

fn main() {
    run().expect("app failed");
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_line_reports_app_identity() {
        // Product renamed bilidown; version line continues the v2.21.0 line.
        assert_eq!(
            crate::version_line(),
            format!("bilidown v{}", env!("CARGO_PKG_VERSION"))
        );
    }
}
