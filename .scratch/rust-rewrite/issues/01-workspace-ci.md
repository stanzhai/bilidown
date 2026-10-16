# 01: Rust workspace scaffold + three-platform CI

**What to build:** The repo carries a Rust workspace under `crates/` that
builds a hello-window binary (gpui-kit app skeleton rendering one empty
window) on Windows/macOS/Linux, with CI building+testing it on all three
platforms. Python tree untouched.

**Blocked by:** None (can start immediately).

**Status:** resolved

- [x] `crates/` cargo workspace with root Cargo.toml, exactly pinned gpui-kit / gpui-pre, builds on macOS locally
- [x] One crate (e.g. `app`) opens a native window and exits cleanly
- [x] GitHub Actions job matrix (windows-latest, macos-latest, ubuntu-latest) runs cargo build + cargo test, green
- [x] Python build/release workflows keep working (no regression)

## Comments

## Answer

Workspace scaffolded in `crates/` (root Cargo.toml + `app` crate, binary
`bili23-downloader`) on gpui-kit =0.7.3 / gpui-pre =0.3.9; opens one native
window via `gpui_kit::open_window`. New `.github/workflows/rust.yml` runs
cargo build+test on the 3-OS matrix (paths-filtered to `crates/**`); Python
workflows and tree untouched. Commit: 65bf9bdf.

