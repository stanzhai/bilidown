# 29: Packaging + distribution

**What to build:** Native packages: Windows Inno Setup installer (same
app id, v2.21.0 line), macOS .app/.dmg, Linux AppImage. Bundled ffmpeg
resolution per platform. Small self-contained executables.

**Blocked by:** 26, 22, 23, 24, 25, 27.

**Status:** resolved

## Answer
Rust CI gains a clippy gate and packaging-checks job; new rust-release.yml builds per-platform artifacts (win zip+Inno, macos dmg x86_64+aarch64, linux AppImage) via crates/packaging/src/bin/packager.py which stages bundle/ffmpeg next to the binary and keeps the Python installer's app id and v2.21.0 line. Commit 8c40f35f.

- [x] Three platform artifacts build in CI (release workflow)
- [x] Bundled ffmpeg found at `bundle/ffmpeg(.exe)` at runtime
- [x] App identity: same name/app id, version line continues v2.21.0
- [x] Smoke: each artifact launches, opens window, reads user-data dir

## Comments
