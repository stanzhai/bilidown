# 06: URL patterns + auto-detect

**What to build:** URL recognition layer: every URL family the Python app
accepts (av/BV/au/am/b23.tv, season/space/favorites/watch-later/history/
weekly-popular/dynamic/cheese/mall, batch id lists) matched and classified
to a Parser Type, with b23.tv short-link expansion. Auto-detect needs no
manual mode pick.

**Blocked by:** 04.

**Status:** resolved

- [x] URL pattern matrix from test_url_pattern passes verbatim
- [x] b23.tv expansion via the HTTP seam (wiremock: canned redirect)
- [x] Search-URL parsing ports test_search_url vectors
- [x] Every URL family returns its Parser Type and raw ids

## Answer

`crates/urls`: order-sensitive dispatch table (ported verbatim from
`url_pattern.py`) classifying every URL family to a `ParserType` with raw
ids, `search` module porting `search_url.py`, and `expand` resolving
b23.tv/bili2233.cn redirects + festival pages over a `reqwest::Client`.
Batch lists are a dialog-only input (the batch parser receives av/BV lines
directly, never a URL), so they are not part of URL dispatch. Commit:
d04d40bf.

## Comments
