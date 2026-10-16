# 04: HTTP client + wiremock test seam

**What to build:** Shared async network layer (tokio + reqwest/rustls)
with cookie store, proxy (none/system/manual), custom User-Agent, and
request signing hooks; wiremock as the repo-wide HTTP mock seam. All
later Bilibili-facing tickets build on this.

**Blocked by:** 01.

**Status:** in-progress

- [ ] reqwest client builder: cookie jar, rustls, proxy config, UA from config
- [ ] wiremock-based tests: given canned HTTP responses, client requests observable (headers/params/cookies)
- [ ] Cookie persistence matching Python's behavior across restarts
- [ ] Proxy modes tested at the seam (direct, proxied)

## Comments
