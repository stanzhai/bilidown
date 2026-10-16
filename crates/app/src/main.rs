use gpui_kit::*;

mod bootstrap;
mod download_pipeline;
mod pipeline_extras;
mod ui;

pub use ui::{Screen, Shell};

/// Product identity line used by the About surface and smoke checks.
pub fn version_line() -> String {
    format!("Bili23 Downloader v{}", env!("CARGO_PKG_VERSION"))
}

/// Saved window bounds (F2): `Behavior/window_state` when
/// `remember_window_state` is on and the state validates; centered
/// 950×600 otherwise. No screens are queryable through gpui-pre, so the
/// title-bar-intersects-a-screen check uses the primary monitor bounds
/// the platform exposes at window creation.
fn saved_window_bounds(cfg: &config::ConfigStore, cx: &App) -> Option<WindowBounds> {
    let screens = cx
        .displays()
        .iter()
        .map(|d| {
            let b = d.bounds();
            services::window_state::ScreenRect {
                x: b.origin.x.as_f32() as i64,
                y: b.origin.y.as_f32() as i64,
                width: b.size.width.as_f32() as i64,
                height: b.size.height.as_f32() as i64,
            }
        })
        .collect::<Vec<_>>();
    let state = services::window_state::saved_state(cfg, &screens)?;
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
            // F2 restore: saved bounds need displays, which only exist in
            // the app context.
            let mut options = WindowOptions::default();
            if let Some(bounds) = saved_window_bounds(&cfg, cx) {
                options.window_bounds = Some(bounds);
            }
            gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| Shell::new(window, cx))
            })
            .expect("Failed to open window");
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
        // Product identity unchanged (spec): same name, continues the
        // v2.21.0 version line.
        assert_eq!(
            crate::version_line(),
            format!("Bili23 Downloader v{}", env!("CARGO_PKG_VERSION"))
        );
    }
}
