# 31: Clean cutover — delete Python build

**What to build:** The single clean-cutover commit: delete the Python
sources, launcher (PyStand), and related build scripts once the parity
checklist is fully green. Rust tree becomes the product.

**Blocked by:** 30.

**Status:** in-progress

- [ ] src/ Python app, launcher/, PyStand, Python build scripts and CI jobs deleted
- [ ] README/docs updated to the Rust build only
- [ ] Repo still builds and all Rust tests green after deletion

## Comments

### F1 (webview GeeTest) — documented deviation, shipped as loopback-only

Spec decision C named webview-primary + browser-loopback fallback for the SMS
GeeTest step. The Rust build ships the loopback flow only
(`app login/server.rs` + embedded `captcha.html`, 6 wire tests): no
`gpui-webview` crate exists in the workspace and adding one would pull a
browser engine into a small native app for one page. The escape hatch (open
the captcha in the system browser, post the result back over the loopback
server) is the primary path here. Same UX, no new heavyweight dependency.

### F7 (three-platform CI) — local gate is the evidence available

The matrix workflow (`.github/workflows/rust.yml`: win/mac/linux
build+clippy+test + packaging-checks) is correct and locally validated, but
the repo is unpushed/private — the Actions API reports `total_count: 0`, so
no remote green run exists to cite. Evidence for the cutover gate is the
local full-workspace run (`cargo build --workspace`, `cargo test --workspace`,
`cargo clippy --workspace --all-targets -- -D warnings`) plus
`python3 crates/packaging/test/check_packaging.py`. The first push's CI run
is the remaining proof; the workflow files themselves are unchanged by this
ticket's Rust-only edits except deleting the Python job.

### F2-completion note — notification is eprintln until a notifier lands

`services::notify` decision logic (config gate `Behavior/show_notification` +
active-count==0, mirroring `list_view.py:140`) is wired into the bootstrap
download driver, but the payload currently goes to stderr: gpui-kit exposes no
OS notification API, and the Python build's `system_tray_icon.show_message`
path rides on the ticket-26 tray. When the tray lands, swap the eprintln for
the tray's message API (marked `ponytail:` in `bootstrap.rs`).

