# 14: CDN host probing + health tracking

**What to build:** CDN host selection: replacement mirror hosts
(upos-sz-mirror*), CN vs OV area lists, per-provider preference,
concurrent host probing with health tracking. The chunked downloader
consumes the chosen host.

**Blocked by:** 04.

**Status:** resolved

- [x] Host replacement matrix from test_cdn_candidates ports verbatim
- [x] Concurrent probes (HEAD/Range) rank hosts; failures mark unhealthy with recovery
- [x] Area lists (CN/OV) applied per provider preference
- [x] Downloader integrates probed host selection (wiremock: multiple fake mirrors)

## Answer

Ported in b8e4231c + 180a5810 (merged integration tip d7980a71 in
e9334340): network::cdn (Area/parse_area, CdnConfig with CN/OV lists,
candidate_urls with layer-wise arrange, HostHealth global: 2 failures ->
180s cooldown) + network::probe (resolve_download_url, sliding window of
4 HEAD probes with Range-GET fallback) + HttpClient::probe; 23 unit +
8 wiremock tests.

## Comments
