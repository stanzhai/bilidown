# 34: Playurl clients carry visitor + login cookies (4K quality)

**What to build:** Bilibili gates playurl/quality APIs on cookies. All
single-purpose HTTP clients used on parse/playurl paths must be seeded
with the persisted visitor cookie set (buvid*, _uuid, b_lsid, b_nut,
buvid_fp, bili_ticket*, CURRENT_FNVAL/QUALITY) plus, when logged in,
the login set (SESSDATA, bili_jct, DedeUserID*), domain `bilibili.com`.

Sites: the Shell's auth client, `ParseService` (covers every
parse/playurl path), the download driver loop, and the download-options
preview worker. Expose a seeding helper (e.g. `auth::seed_client_cookies`)
and call it at each client construction.

**Blocked by:** (none)

**Status:** resolved

- [x] `auth` exports a cookie-seeding helper reading the persisted
      visitor + login cookie sets
- [x] All four client sites seed cookies; login-dependent qualities
      (e.g. 4K) appear for VIP accounts
- [x] Unit test: seeded client's cookie store contains the persisted
      visitor entries; login set present iff `is_login`

## Answer

`auth::session::seed_client_cookies(&HttpClient, &AuthState)` (re-exported
from the crate root) installs the persisted visitor set — buvid3/buvid4,
_uuid, b_lsid, b_nut, buvid_fp, bili_ticket(+expires), CURRENT_FNVAL=4048,
CURRENT_QUALITY=0 — plus the login set (SESSDATA, bili_jct, DedeUserID,
DedeUserID__ckMd5) iff `is_login`, all on `.bilibili.com`, path `/`. It
mirrors Python `get_cookies` + `_apply_cookies`; `Session`'s own
sync methods now share the same entry lists
(`visitor_cookie_entries` / `login_cookie_entries`) so the two paths
cannot drift.

Wired at all four sites, each reading config.json's `Cookie` section via
`AuthState::from_config_file(config_path(cfg_dir))`:

- `Shell::new` (auth client): auth_state is now read **before** the
  client is built and the client is seeded right after construction.
- `ParseService::new`: derived from `cfg.path()`'s parent; every
  parse/playurl path goes through this service (and
  `with_episode_data`, which delegates to `new`).
- `bootstrap::driver_loop`: seeds the driver client before the scan
  loop starts.
- `open_download_options` preview worker: auth state is hoisted out of
  the `window.spawn` async block (the Shell update lease is held there,
  so the entity must not be re-read); the client is seeded immediately
  after construction inside the worker.

Test: `crates/auth/tests/seed_cookies.rs` — fake persisted state seeds
all 10 visitor entries with exact values (including CURRENT_FNVAL/QUALITY
constants) on `.bilibili.com`/`/`; SESSDATA/bili_jct/DedeUserID/
DedeUserID__ckMd5 present iff `is_login`.

Known limitation (unchanged): the driver/parse clients read auth state
once at construction; a login mid-session re-seeds only the Shell's auth
client (the login flow's own sync path). Restart picks up the new state.

## Comments
