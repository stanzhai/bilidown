# 45: Windows GUI subsystem, exe/installer icon, Linux X11 icon

**What to fix:** The packaged Windows exe launched with both a console
window and the app window, and had no icon. Linux X11 windows showed the
generic "unknown" icon (macOS was already fine via app.icns).

**Status:** resolved

## Answer
- Windows: `crates/app/build.rs` (winresource, `cfg(windows)`-gated build
  dep) embeds `crates/app/assets/app.ico` at resource ID 1 — where gpui's
  `register_window_class` `LoadImageW` reads the title-bar/taskbar icon —
  plus `assets/app.manifest` (PerMonitorV2 DPI awareness; gpui never
  marks the process DPI-aware itself). `main.rs` gains
  `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` so
  release builds show no console (debug keeps it for logs; mcp-stdio is a
  separate bin). `app.ico` (6 sizes) regenerated from `assets/app.svg`
  via rsvg-convert + Pillow; command documented in README.
- Linux: `WindowOptions::icon` (`cfg(target_os = "linux")`) set from the
  embedded 128px `tray-icon.png` (pixel-identical to the app_128 render).
  Wayland/macOS ignore the field.
- Inno installer: `SetupIconFile=.\app.ico`; packager stages `app.ico`
  next to `setup.iss`.
- Enabler: reqwest's default `rustls` feature hard-pins the aws-lc-rs
  provider (C build needing an MSVC SDK — unbuildable on the mac host).
  Switched every reqwest crate dep to `rustls-no-provider` + direct
  `rustls` with the `ring` feature, and installed
  `rustls::crypto::ring::default_provider().install_default()` at each
  raw Client construction site (network HttpClient::new/probe, mcp
  stdio/parse, urls/mcp tests). aws-lc-sys is gone from the tree.
- Windows target verified via CI (rust.yml windows-latest real build);
  local xwin cross-compile was blocked on the C toolchain, not on code.
