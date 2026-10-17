# 32: User-reported bug batch (7 issues)

**What to build:** Fix the 7 bugs reported against the Rust/GPUI build:
login status shows username (not "Logged in"), VIP login missing 4K
quality options, parse-list checkbox misalignment (done earlier), English
leftovers after switching to Chinese, dark-mode setting ignored, main nav
as icon sidebar, default window 2/3 screen centered.

**Status:** resolved

- [x] Login status shows the account's `uname`
- [x] Playurl clients carry visitor + login cookies (4K gated on them)
- [x] Checkbox alignment (issue 3, prior session)
- [x] All UI strings route through the i18n catalog
- [x] Theme + language apply instantly; startup reads configured theme
- [x] Main nav is a collapsed icon rail with a login footer button
- [x] First-run window = 2/3 of main display, centered, min 950x600

## Answer

All 7 fixed. Decisions recorded below; verification at the end.

### Issue 1 — username in login status
`auth::login::fetch_uname(client)` GETs `x/web-interface/nav` →
`data.uname`. `AuthState` gains a persisted `uname` field. `Shell::boot`
(piped in `main.rs`) refreshes it when `is_login` at startup;
`on_login_success` refreshes after any login. The sidebar footer
button's tooltip shows it.

### Issue 2 — 4K options missing for VIP users
Bilibili gates quality APIs on cookies. `auth::seed_client_cookies`
(new, exported) installs the persisted visitor set (buvid*, _uuid,
b_lsid, b_nut, buvid_fp, bili_ticket*, CURRENT_FNVAL/QUALITY) plus the
login set (SESSDATA, bili_jct, DedeUserID*) when logged in, domain
`bilibili.com`. Wired at all 4 single-purpose clients:
`Shell::new` (auth client), `ParseService::new` (covers every
parse/playurl path — it re-reads config per construction),
`bootstrap::driver_loop`, and `open_download_options`' preview worker
(auth state hoisted before the async block).

### Issue 3 — checkbox misalignment
Fixed in a prior session: `collect_row_meta` in `parse/screen.rs` walks
children only; regression test `collect_row_meta_matches_display_rows`.

### Issue 4 — i18n sweep
Missing catalog entries hand-added to the HAND-ADDED section of
`services/src/i18n_data.rs` (generation script is lost; mechanism noted
in the file header). All UI string sites routed through `tr` lookups:
download_options dialog (tabs, media/additional/download sections, rule
builder, footer), parse screen (toolbar, status lines, category names
via `convention_type`), parse model labels (count/match/pagination),
login placeholders, favorites stub, shell status notes. `Lang` plumbing
already existed; only the hard-coded literals were left.

### Issue 5 — theme + language instant apply
Startup: `run()` applies `QFluentWidgets.ThemeMode` right after
`gpui_kit::init` (kit default is Light) — Light/Dark via
`Theme::change`, Auto via `Theme::sync_system_appearance`.
Settings dropdowns: theme setter calls `Theme::change` /
`sync_system_appearance` directly; language setter flips
`SettingsUi.lang` + the `ShellI18n` app global (shell/login re-render
translated). `restart_notice` stays only for `display_scaling`.

### Issue 6 — icon sidebar
`Sidebar::new("nav").collapsed(true)` (maps to `SidebarCollapsible::Icon`
per kit docs) with the 5 nav items; labels translate through
`MainWindow` entries. Login entry moved into `.footer(...)` as an icon
`Button` (CircleUser when logged in, LogIn otherwise) whose tooltip is
the `uname` / "Log In"; always clickable (no `disabled`), opens the
login dialog when logged out. The old absolutely-positioned overlay is
deleted.

### Issue 7 — default window size
`saved_window_bounds` returns None (no saved state) → fallback computes
2/3 of the main display's bounds, floored at the 950x600 minimum
(`services::window_state` DEFAULT), centered on that display. Saved
bounds (remember_window_state) still win when present.

### Issue 8 — downloads page showed no live progress
The progress bar never moved while downloading (and the task could be
done already). Three root causes, one per layer:
1. **Pipeline**: `run_task` wrote status once before spawning the
   engine, then `handle.await` blocked until the terminal state — the
   engine's live `downloaded_size` never reached the DB. Fix:
   `spawn_engine` now also returns the `EngineControl`; the pipeline
   spawns a sampler task polling `control.snapshot()` every 500ms and
   persisting progress/downloaded_size/speed (speed computed as a byte
   delta per window — the engine never calculates it) through
   `update_async`'s coalescing writer.
2. **Merge**: `Merger` updated `task.Download.progress` in its ffmpeg
   callback but only persisted after `run()` returned. Fix:
   `Merger::set_progress_sink` hands each percentage to the pipeline,
   which persists every 2% step from a pre-borrow snapshot.
3. **UI**: the downloads screen refreshed only on tab switch. Fix:
   `Shell::boot` starts a 1s loop (clipboard-poll pattern) that calls
   `refresh_if_active` while the downloads page is on screen; it skips
   the query when no task is in flight.

### Known limitation (not fixed)
The download driver seeds cookies once at startup; logging in
afterwards leaves the driver's playurl requests anonymous until the
next restart. ParseService re-seeds per construction and is unaffected.

### Verification
- `cd crates && cargo build -p app` clean
- `cargo test -p services -p parsers -p app -p auth`: all green
  (189 app tests incl. updated `pagination_label_formats_totals`)
- `cargo run -p app` smoke: process boots, runs 8s, no panic (GPUI
  native window — visual confirmation beyond boot is a manual step)

## Comments
