# 04: HTTP client + wiremock test seam

**What to build:** Shared async network layer (tokio + reqwest/rustls)
with cookie store, proxy (none/system/manual), custom User-Agent, and
request signing hooks; wiremock as the repo-wide HTTP mock seam. All
later Bilibili-facing tickets build on this.

**Blocked by:** 01.

**Status:** resolved

- [x] reqwest client builder: cookie jar, rustls, proxy config, UA from config
- [x] wiremock-based tests: given canned HTTP responses, client requests observable (headers/params/cookies)
- [x] Cookie persistence matching Python's behavior across restarts
- [x] Proxy modes tested at the seam (direct, proxied)

## Answer

Implemented `crates/network` (crate `network`): shared reqwest/rustls client with cookie jar + persistence snapshot, ProxyMode None/System/Manual, configurable User-Agent, Referer default header, Python-mirroring timeouts, and a `RequestSigner` hook seam for wbi (ticket 08). 9 wiremock external-behavior tests (7 wire + 2 proxy_env). Commit: see branch ticket-04.

## Comments
