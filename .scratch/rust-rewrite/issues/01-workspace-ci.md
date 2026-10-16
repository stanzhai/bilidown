# 01: Rust workspace scaffold + three-platform CI

**What to build:** The repo carries a Rust workspace under `crates/` that
builds a hello-window binary (gpui-kit app skeleton rendering one empty
window) on Windows/macOS/Linux, with CI building+testing it on all three
platforms. Python tree untouched.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] `crates/` cargo workspace with root Cargo.toml, exactly pinned gpui-kit / gpui-pre, builds on macOS locally
- [ ] One crate (e.g. `app`) opens a native window and exits cleanly
- [ ] GitHub Actions job matrix (windows-latest, macos-latest, ubuntu-latest) runs cargo build + cargo test, green
- [ ] Python build/release workflows keep working (no regression)

## Comments
