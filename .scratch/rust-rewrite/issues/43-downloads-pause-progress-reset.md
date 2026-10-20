# 43: Downloads screen — pause resets progress display to zero

**What broke:** Clicking 暂停 on a running task reset the row's
已下载 to 0 (progress bar back to empty). The persisted row lost
`downloaded_size`/`progress` too, so the value stayed 0 until the
next resume.

**Root cause:** In `run_task`'s Paused branch
(`download_pipeline.rs`), the pipeline's local `task` clone never
tracked engine progress — its `downloaded_size` was whatever the row
had at admission (0 for a fresh task). The branch then wrote that
clone to the DB via `update_async` **after** the UI's
`toggle_pause_resume` had already persisted the engine's final
snapshot. `update_async` is latest-wins: the merge only unions
`chunk_offsets`, so the stale 0-progress clone overwrote the pending
snapshot's `downloaded_size`/`progress`. The Completed branch already
did this correctly — it folded `state.downloaded_size`/`progress`
into the clone before writing; the Paused branch just skipped those
lines.

Resume itself was never broken: `toggle_pause_resume` re-queues the
task, the driver re-admits it, `current_files()` prefers the pending
`chunk_offsets` snapshot, and `resume_from` seeks past committed
offsets. Only the persisted display fields were lost.

**Fix:** the Paused branch now folds the engine's final
`state.downloaded_size`/`progress` into the task clone before
`update_async`, mirroring the Completed branch — three lines. Resume
path unchanged; its correctness is now locked by a regression test.

**Status:** resolved

- [x] Paused branch folds engine final progress
      (`download_pipeline.rs` run_task)
- [x] Regression test `paused_run_persists_progress_and_resumes_from_confirmed_offset`:
      8 MiB file = 2 chunks, chunk 1's response delayed 500 ms (via
      wiremock `set_delay`) so pause lands with chunk 0 committed and
      chunk 1 unstarted. Pauses through the UI's own
      `toggle_pause_resume`, then asserts the persisted row keeps
      status PAUSED, `downloaded_size >= 4 MiB`, `0 < progress < 100`,
      non-empty `chunk_offsets`. Second run resumes through the same
      pipeline and asserts COMPLETED with exactly one `bytes=0-…`
      chunk request ever issued (chunk 0 never re-downloaded).
      Verified it fails on the pre-fix code (`paused row lost
      downloaded_size: 0`).
- [x] `cargo test -p app` (208 passed), `cargo test --workspace`
      (86 suites ok), `cargo clippy --workspace --all-targets` —
      only pre-existing warnings
- [x] UI pixel-level verification not feasible offline: no real
      network task can be paused mid-download without the mock
      server. Behavior is covered end-to-end by the integration test
      above, which drives the real UI verb
      (`toggle_pause_resume`) against the real pipeline and DB.
