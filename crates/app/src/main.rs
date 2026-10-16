use gpui_kit::*;

mod ui;

pub use ui::{Shell, Screen};

/// Product identity line used by the About surface and smoke checks.
pub fn version_line() -> String {
    format!("Bili23 Downloader v{}", env!("CARGO_PKG_VERSION"))
}

/// Opens one native window; returns when the app event loop exits cleanly.
pub fn run() -> anyhow::Result<()> {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            // Must be called before using any GPUI Component features.
            gpui_kit::init(cx);
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
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
