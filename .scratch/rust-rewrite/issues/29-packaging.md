# 29: Packaging + distribution

**What to build:** Native packages: Windows Inno Setup installer (same
app id, v2.21.0 line), macOS .app/.dmg, Linux AppImage. Bundled ffmpeg
resolution per platform. Small self-contained executables.

**Blocked by:** 26, 22, 23, 24, 25, 27.

**Status:** in-progress

- [ ] Three platform artifacts build in CI (release workflow)
- [ ] Bundled ffmpeg found at `bundle/ffmpeg(.exe)` at runtime
- [ ] App identity: same name/app id, version line continues v2.21.0
- [ ] Smoke: each artifact launches, opens window, reads user-data dir

## Comments
