# 39: Login entry shows uname + centered label

**What broke:** Three consumer-visible defects in the bottom-left login
entry (the issue-38 rail):

1. After a successful QR login the button kept saying `Log In` — the
   login flow's four cookies (SESSDATA/bili_jct/DedeUserID/
   DedeUserID__ckMd5) only land in the reqwest `Jar`; they never reach
   the persisted `Cookie` list, and the nav request that would fetch
   `uname` runs anonymously, so `uname` stays empty and the label never
   flips.
2. The label under the icon sat flush-left in its 52px box (icon
   centered, text not).
3. A stale `Cookie.is_login=true` with all four login keys empty (the
   pre-harvest login flow's leftover) booted the shell "logged in":
   the rail offered `Log out`, and logout then failed against the dead
   session.

**Status:** resolved

- [x] Login flow harvests the four login cookies from the jar after the
      nav response and captures `uname`
- [x] Rail login label horizontally centered
- [x] `AuthState::from_config_file` normalizes `is_login=true` + empty
      SESSDATA → logged-out (auth layer, one fix for all five
      consumers); `Shell::new` adds a defensive `normalize_auth_state`
      for directly-constructed stale states
- [x] `Shell::logout` rewritten: empty `bili_jct` skips the doomed API
      call, and local state clears regardless of API outcome (a dead
      server session must not trap the user in "logged in"); only the
      toast text differs
- [x] QR poll harvests the jar at the poll response's host
      (`session_for_domain` + `url_host`); production endpoints are
      bilibili.com so behavior is unchanged there, tests point at the
      mock host
- [x] QR wiremock cookies switched from `Domain=.bilibili.com` to
      host-only — the old shape never entered the jar and the old
      assertion passed only via the stale-flag tautology
- [x] Regression tests: `config_load_normalizes_stale_is_login_flag`,
      `stale_is_login_config_boots_shell_logged_out`, rewritten
      `qr_flow_generates_polls_captures_and_persists` (jar → disk →
      reload, real cookies)
- [x] Pixel smoke: stale config boots to `登录`; injected
      SESSDATA+uname boots to `smokeuser` (label flips)

## Answer

`HttpClient::jar_cookie_header(url)` exposes the reqwest Jar's view of
a URL's cookies. `Session::capture_login_cookies` reads the four login
keys from the jar (falling back to the persisted values when the jar
has none), so the nav response's host-only Set-Cookies become the
persisted `Cookie` entries, and the follow-up nav call runs signed.

The stale-flag fix is at `from_config_file` because all five consumers
(mod.rs/dispatch.rs/download_options/bootstrap.rs/mcp) read auth state
through it. Logout failure no longer blocks local state clearing: the
server session can already be dead.

Click-path note: synthetic clicks (osascript/CGEvent) are not
reliably synthesizable in this environment (no pyobjc, no
accessibility grant); click behavior is covered by the deterministic
tests instead.
