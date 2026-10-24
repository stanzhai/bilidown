//! Windows resource embedding: the app icon.
//!
//! gpui's `register_window_class` loads the window icon from the exe's
//! resource ID 1 (`LoadImageW(module, PCWSTR(1), IMAGE_ICON, …)`), so
//! `set_icon` (which targets ID 1) is what makes both the title bar and
//! the taskbar pick up `assets/app.ico`.
//!
//! No manifest here: gpui's default `windows-manifest` feature already
//! embeds one at RT_MANIFEST ID 1 (PerMonitorV2 + Common-Controls) —
//! embedding a second manifest at the same resource ID is a CVT1100
//! duplicate-resource link error. No-op on other platforms.

fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        #[cfg(windows)]
        {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("assets/app.ico");
            res.compile().expect("embed windows icon");
        }
    }
}
