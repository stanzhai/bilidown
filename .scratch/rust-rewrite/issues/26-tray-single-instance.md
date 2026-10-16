# 26: Tray + single instance + exit integration

**What to build:** System tray (tray-icon + muda): menu with restore/
quit, window restore on click; Single Instance mutual exclusion
replicating the Windows named mutex (same GUID), lock file with staleness
detection, local socket wake protocol — interop with the Python build
included; exit dialog integration with minimize-to-tray.

**Blocked by:** 20.

**Status:** ready-for-agent

- [ ] Second launch (Rust or Python) focuses the existing window, no second instance
- [ ] Named mutex GUID identical to Python's on Windows; lock-file staleness logic ports Python's
- [ ] Socket wake brings window forward
- [ ] Tray menu works on macOS/Linux; close-to-tray honors remember-choice

## Comments
