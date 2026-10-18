# 26: Tray + single instance + exit integration

**What to build:** System tray (tray-icon + muda): menu with restore/
quit, window restore on click; Single Instance mutual exclusion
replicating the Windows named mutex (same GUID), lock file with staleness
detection, local socket wake protocol — interop with the Python build
included; exit dialog integration with minimize-to-tray.

**Blocked by:** 20.

**Status:** resolved

- [x] Second launch (Rust or Python) focuses the existing window, no second instance
- [x] Named mutex GUID identical to Python's on Windows; lock-file staleness logic ports Python's
- [x] Socket wake brings window forward
- [x] Tray menu works on macOS/Linux; close-to-tray honors remember-choice

## Answer

`crates/app/src/tray.rs` + `single_instance.rs` + main.rs wiring:

- Tray (tray-icon + muda): menu Show/Exit, restore on click; events arrive on
  background threads, drained by a foreground `cx.spawn` pump (50ms tick) so
  all UI work stays on the main loop.
- Single instance: lock file under the app-data dir with staleness detection
  (mtime probe + PID liveness), local socket for the wake protocol; a second
  launch writes a wake event and exits — socket wake brings the primary window
  forward. Interop paths match the Python build (same data-dir layout).
- Exit integration: `Shell::ask_exit` (ticket 22's dialog) wired into
  `on_window_should_close`; `when_close` config (minimize/exit/ask) decides,
  "remember" persisted by the dialog itself.
- Tests: close_decision matrix, when_close config read, lock-file location,
  socket wake, version line — 5 in main.rs, all green on spec-batch-32.

Compile-pathology fix (worth knowing repo-wide): `use gpui_kit::*` and
`use gpui_kit::gpui::*` both re-export gpui's `test` attribute macro, which
shadows the builtin `#[test]`. Plain test fns then expand through gpui's
macro; its syn recursion on nested bodies made `cargo check -p app --tests`
run for hours. gpui-kit's own source comment warns about this. main.rs now
imports gpui items explicitly and keeps `#[test]` builtin; check ≈2s,
`cargo test -p app --bin bilidown` ≈16s.

## Comments
