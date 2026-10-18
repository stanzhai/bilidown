# 33: Login identity — `uname` in status + icon rail with login footer

**What to build:** The main nav becomes a collapsed icon rail (5 items)
with the login entry moved to the sidebar `.footer(...)` as an icon
button (CircleUser when logged in, LogIn otherwise), tooltip = the
account `uname` or "Log In", always clickable, opening the login dialog
when logged out. Login status shows the account's `uname` (fetched from
`x/web-interface/nav` → `data.uname`), not the literal "Logged in".

Also covers ticket 32 item 3 verification only: the checkbox-alignment
fix (`collect_row_meta` walks children only) already exists on main —
run its regression test and record the result in the Answer.

**Blocked by:** (none)

**Status:** resolved

- [x] `AuthState` carries a persisted `uname`, refreshed at startup when
      `is_login` and after any login success
- [x] Sidebar is a collapsed icon rail (labels available via tooltip);
      5 nav items unchanged
- [x] Footer login button: icon reflects login state, tooltip shows
      `uname` / "Log In", opens login dialog when logged out, never disabled
- [x] Ticket-32 item 3 (`collect_row_meta_matches_display_rows` or
      equivalent) test located and passing; result recorded here

## Answer


Implemented on `ticket-33` (worktree `/tmp/bd-33`), merged with the
latest `spec-batch-32` tip before verification; build, tests, clippy
`-- -D warnings` all green.

- **`uname` field**: `AuthState.uname` (serde
  `default`/`skip_serializing_if String::is_empty`, no rename — not a
  cookie key), persisted via the existing `Cookie`-section save path
  (`login::controller::persist`). `Session::clear_login` clears it on
  logout.
- **Fetch**: `auth::login::fetch_uname(client, nav_url)` — signed GET
  nav, `check_response`, reads `data.uname` (empty string when the
  account hides it). The URL param is the wiremock seam (auth's consts
  stay pinned in production callers).
- **Refresh at startup**: `Shell::boot(shell, cx)` piped in `main.rs`
  right after `open_window` → `refresh_uname` (skips when not
  `is_login`). **After login success**: `on_login_success` calls the
  same `refresh_uname`. Both spawn via `cx.spawn` +
  `crate::bootstrap::tokio_context()`, read the client with
  `shell.read_with` (never re-entering the update lease), then persist
  on success; offline/failed fetch keeps the stored value.
- **Icon rail**: `Sidebar::new("nav").collapsed(true)` — the 5 nav
  items unchanged, labels via tooltip. The old absolutely-positioned
  `shell-login` overlay div is deleted; its `Disableable` import went
  with it.
- **Footer login button**: `.footer(Button::new("shell-login-button")
  .icon(CircleUser|LogIn by login state).tooltip(uname | "Log In")
  .on_click(open_login when logged out))` — never disabled.

**Ticket-32 item 3 result**: the regression test existed nowhere
(searched the worktree, main checkout, all branches, and dangling
commits) — and the misalignment was real: `collect_row_meta` pushed the
invisible wrapper root itself while `model::flatten` skips it, shifting
every checkbox/action by one row. Wrote
`ui::parse::screen::tests::collect_row_meta_matches_display_rows` red
(failed 6 vs 5 — the off-by-one, proving the bug live on main), then
fixed `collect_row_meta` to walk children only, matching `flatten`'s
shape. Test now passes; `cargo test -p app` 189 passed / 0 failed.

## Comments
