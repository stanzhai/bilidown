# 13: Chunked download engine

**What to build:** The multi-chunk ranged downloader: 1-10 chunks × 1-10
parallel tasks, per-chunk offset table persisted for resume, chunk-level
retry with exponential backoff, task-level auto-retry (1-20), token-bucket
global Speed Limit, file preallocation, 206-vs-full-response guard,
pause/resume/cancel. Fully driven at the wiremock HTTP seam.

**Blocked by:** 04.

**Status:** resolved

- [x] Multi-chunk ranged download reassembles byte-identical file from wiremock CDN fixtures
- [x] Resume from persisted offset table downloads only remaining ranges (retry semantics of test_auto_retry ported)
- [x] Non-206 full response triggers the guard (no silent corruption)
- [x] Chunk retry/backoff and task auto-retry behave per config
- [x] Token-bucket limiter caps observed throughput; preallocation on by config
- [x] Pause/resume/cancel observable through task state

## Answer

`crates/downloader`: engine with 4 MiB chunks, persisted per-chunk offset
tables (`OffsetTableStore`), chunk retry with capped exponential backoff,
task auto-retry per config, token-bucket speed limit, optional
preallocation, 206 guard, pause/resume/cancel via generation counting —
13/13 wire tests in `tests/wire.rs`. Commit 7895fdb5.

## Comments
