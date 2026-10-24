//! Windows resource embedding: the app icon and a DPI-aware manifest.
//!
//! gpui's `register_window_class` loads the window icon from the exe's
//! resource ID 1 (`LoadImageW(module, PCWSTR(1), IMAGE_ICON, …)`), so
//! `set_icon_with_id("1")` is what makes both the title bar and the
//! taskbar pick up `assets/app.ico`. No-op on other platforms.

fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    println!("cargo:rerun-if-changed=assets/app.manifest");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        #[cfg(windows)]
        {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("assets/app.ico");
            // gpui calls GetDpiForWindow / handles WM_DPICHANGED but never
            // marks the process DPI-aware — without this Windows
            // virtualizes coordinates and the window renders blurred on
            // scaled displays. (winresource writes the manifest as the
            // RT_MANIFEST resource of the version-info's FILETYPE id.)
            res.set_manifest(include_str!("assets/app.manifest"));
            res.compile().expect("embed windows icon + manifest");
        }
    }
}
