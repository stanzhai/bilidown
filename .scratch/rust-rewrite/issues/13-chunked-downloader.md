# 13: Chunked download engine

**What to build:** The multi-chunk ranged downloader: 1-10 chunks × 1-10
parallel tasks, per-chunk offset table persisted for resume, chunk-level
retry with exponential backoff, task-level auto-retry (1-20), token-bucket
global Speed Limit, file preallocation, 206-vs-full-response guard,
pause/resume/cancel. Fully driven at the wiremock HTTP seam.

**Blocked by:** 04.

**Status:** ready-for-agent

- [ ] Multi-chunk ranged download reassembles byte-identical file from wiremock CDN fixtures
- [ ] Resume from persisted offset table downloads only remaining ranges (retry semantics of test_auto_retry ported)
- [ ] Non-206 full response triggers the guard (no silent corruption)
- [ ] Chunk retry/backoff and task auto-retry behave per config
- [ ] Token-bucket limiter caps observed throughput; preallocation on by config
- [ ] Pause/resume/cancel observable through task state

## Comments
