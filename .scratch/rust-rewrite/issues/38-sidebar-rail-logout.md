# 38: Sidebar self-drawn rail; avatar shows uname with logout

**What to build:** (1) The kit `Sidebar` component pins a 48px collapsed
rail with horizontal items — it cannot host 64px icons stacked above
their module names. Draw the nav rail app-side: 88px column, one entry
per screen, icon above an 11px label, active entry on
`theme().sidebar_accent`, click routes identical to the old menu items;
login entry pinned at the bottom. (2) The bottom-left avatar entry shows
the account `uname` when logged in (falls back to the Log In label),
and clicking it while logged in opens a confirm dialog (ProfileCard
copy) whose OK calls the auth logout flow: `login::logout` API → clear
the four login cookies from the shared jar → `seed_client_cookies`
re-seeds the visitor set → persist the anonymous `Cookie` section →
success/error notification. Logged out it still opens the login dialog.

**Status:** resolved

- [x] Hand-drawn rail replaces kit `Sidebar` (nav + footer entries)
- [x] Avatar entry shows uname (or Log In); logged-in click → confirm
      → logout → jar cleared, visitor cookies re-seeded, config
      persisted, notification, footer back to logged-out state
- [x] Auto theme follows OS appearance live (observer only re-syncs
      while the setting is Auto)
- [x] Hardcoded dark-mode colors tokenized (`muted`/`popover`/
      `muted_foreground`/`background`)

## Answer

`crates/app/src/ui/mod.rs`:

- **Rail**: `Shell::render` now builds `div().id("nav-rail")` —
  88px wide, `theme().sidebar` background, right border. Nav entries
  (Parse/Downloads/Favorites/About/Settings) are clickable divs with
  `Icon::new(name).size_8()` above an 11px `Label` (`MainWindow`
  translations); the active entry gets `sidebar_accent` background +
  `sidebar_accent_foreground` ink, inactive entries `muted_foreground`.
  A `flex_1` spacer pushes the login entry to the bottom. Kit imports
  `Sidebar`/`SidebarMenuItem`/`Button` were dropped from the shell.
- **Login entry**: logged out → `Log In` label, click opens the login
  dialog (unchanged). Logged in → uname label (tooltip fallback gone;
  the label *is* the display), click opens `window.open_alert_dialog`
  (`.title(ProfileCard confirm).confirm().ok_text(Logout).cancel_text
  (Cancel)`); OK updates the shell entity.
- **`Shell::logout(window, cx)`**: `window.spawn` → `AsyncWindowContext`
  reads `(auth_state, auth_client, user_agent)` off the shell entity,
  builds a `Session` via the now-`pub`
  `login::controller::session_for`, awaits `auth::login::logout`
  (server-side cookie revoke). On success: clears SESSDATA/bili_jct/
  DedeUserID/DedeUserID__ckMd5 from the shared jar (empty values — the
  API only invalidates server-side), `seed_client_cookies` replays the
  visitor set, `set_auth_state` persists the anonymous Cookie section
  and flips `login.is_login`, then a success notification. Failure →
  `ERROR_MESSAGES/Logout failed` error notification.

Verification: `cargo build -p app` clean; `cargo test -p app -p
services` green (208 app tests, 62 services tests across suites, 270
total); app launched and ran 30s+ in the foreground with no panics or
stderr output.
