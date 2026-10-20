# 40: Login dialog floats after successful QR login

**What broke:** scanning the QR code logs in (cookies captured, rail
flips to uname) but the login dialog stays open and floats until
manually dismissed.

**Root cause:** the QR success branch (and the SMS + Cookie success
branches with it) called `Shell::on_login_success(cx)`, which only
flips the flag and refreshes uname — nothing ever closed
`LoginState::handle`. The dialog's own close paths (backdrop press,
`DialogHandle::close`) were never triggered on success. Only the
cookie sub-dialog got an indirect pass via `CookieModel.accepted`.

**Fix:** `on_login_success(window, cx)` now closes
`login.handle` first — one choke point covers all three flows. The
success branches run inside async spawns (no `Window`), so
`LoginState::open` saves the window handle
(`self.window = Some(window.window_handle())`) and a
`LoginState::finish_login(window, cx)` helper routes the close through
`AnyWindowHandle::update` + view downcast to `Shell`.

**Status:** resolved

- [x] QR/SMS/Cookie success all close the login dialog
- [x] `#[ignore]`d gpui test `login_success_closes_dialog` asserts
      `is_open()` flips false through `on_login_success` (network-real:
      `Shell::new` boots the tokio runtime; run with
      `cargo test -p app login_success_closes -- --ignored`)
- [x] Full `cargo test -p auth -p network -p app` green (206+), clippy
      `-D warnings` clean
- [x] Boot smoke: app launches clean with a logged-in config, no
      panics, rail shows `uname`

Visual limit: the QR scan itself cannot be synthesized in this
environment (no accessibility grant for synthetic clicks; no pyobjc),
so the close-on-scan path is verified by the dialog-state test, not
pixels.
