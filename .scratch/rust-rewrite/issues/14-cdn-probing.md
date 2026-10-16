# 14: CDN host probing + health tracking

**What to build:** CDN host selection: replacement mirror hosts
(upos-sz-mirror*), CN vs OV area lists, per-provider preference,
concurrent host probing with health tracking. The chunked downloader
consumes the chosen host.

**Blocked by:** 04.

**Status:** ready-for-agent

- [ ] Host replacement matrix from test_cdn_candidates ports verbatim
- [ ] Concurrent probes (HEAD/Range) rank hosts; failures mark unhealthy with recovery
- [ ] Area lists (CN/OV) applied per provider preference
- [ ] Downloader integrates probed host selection (wiremock: multiple fake mirrors)

## Comments
