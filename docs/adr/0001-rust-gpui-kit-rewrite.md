---
status: accepted
---

# Rewrite in Rust with gpui-kit, same-repo clean cutover

The Python/PySide6 app (~25-30k LOC) is being rewritten in Rust with
gpui-kit as the UI framework, in this same repository under `crates/`.
Decided through a grilling session (2026-10-16):

- **Why Rust**: drop the bundled Python runtime and the Windows-only
  PyStand C++ launcher; native cross-platform binaries.
- **Why gpui-kit**: production-proven (Longbridge Pro), strong DataTable,
  CJK/IME first-class, Apache-2.0 (GPL-3.0-compatible). Accepted trade-off:
  it pins a weekly-updated GPUI snapshot (`gpui-pre`) with breaking API
  changes — we pin exactly and bump deliberately, never incidentally.
- **UI fidelity = layout/IA/interaction parity, native visuals** (option
  B). A pixel-level Fluent clone would mean writing a component library
  before writing the app; Mica/acrylic are dropped (GPUI cannot).
- **User data is drop-in compatible**: same directory, same config.json
  (with config_version migrations), same three SQLite schemas. The Python
  build stays releaseable until the parity checklist is green, then is
  deleted in one cutover — without a runnable reference build, "features
  unchanged" is unverifiable.
- **Platforms**: Win10 1809+ / macOS 12+ / Linux x64. Win7 support
  dropped (Rust+gpui ecosystem cannot honor it).
- **Identity kept**: same app name/id, v2.21.0 line, same Verhub update
  key; the Rust build is the continuation, not a new product.
- **Stack**: tokio + reqwest (rustls) + axum for non-UI work, decoupled
  from gpui snapshot churn; tray via `tray-icon` + `muda` (gpui has no
  tray); GeeTest captcha via `gpui-webview` with a system-browser
  loopback fallback.

## Considered Options

- Pixel-level Fluent replica on gpui primitives — rejected: cost of
  re-implementing qfluentwidgets before any app code.
- New repository — rejected: loses issue history and parallel
  verification against the reference build.
- gpui-tray — rejected: 8-star, 2 months old, snapshot pin lagging.

## Consequences

- Two mutually-exclusive builds coexist until cutover; single-instance
  interop (named mutex + lock file + socket wake) must be replicated.
- Weekly gpui-pre snapshot churn lands only when we choose to bump.
