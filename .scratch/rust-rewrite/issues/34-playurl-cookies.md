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

**Status:** ready-for-agent

- [ ] `auth` exports a cookie-seeding helper reading the persisted
      visitor + login cookie sets
- [ ] All four client sites seed cookies; login-dependent qualities
      (e.g. 4K) appear for VIP accounts
- [ ] Unit test: seeded client's cookie store contains the persisted
      visitor entries; login set present iff `is_login`

## Comments
