# 06: URL patterns + auto-detect

**What to build:** URL recognition layer: every URL family the Python app
accepts (av/BV/au/am/b23.tv, season/space/favorites/watch-later/history/
weekly-popular/dynamic/cheese/mall, batch id lists) matched and classified
to a Parser Type, with b23.tv short-link expansion. Auto-detect needs no
manual mode pick.

**Blocked by:** 04.

**Status:** ready-for-agent

- [ ] URL pattern matrix from test_url_pattern passes verbatim
- [ ] b23.tv expansion via the HTTP seam (wiremock: canned redirect)
- [ ] Search-URL parsing ports test_search_url vectors
- [ ] Every URL family returns its Parser Type and raw ids

## Comments
