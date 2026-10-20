# 41: Downloads screen — live progress + row covers

**What broke:** Two consumer-visible defects on the downloads
screen:

1. Progress was frozen: the UI only queried the DB on nav click
   (`refresh_if_active` had zero callers). A running task's row sat
   at whatever progress it had when the user navigated to the tab,
   merge stages (FFMPEG_QUEUED/MERGING/CONVERTING) never appeared,
   and pause→resume desynced — the driver loop kept finishing and
   merging in the background while the row claimed to be paused,
   because the resume path never seeded the engine with the bytes
   already on disk (`resume_from` only had test callers).
2. Every row's cover was a static gray div — the cover URL was in
   `Episode.cover` but nothing fetched or rendered it.

**Status:** resolved

- [x] Live tick: `DownloadsTick` child entity self-reschedules via
      `window.request_animation_frame()` and drives
      `Shell::tick_downloads` (frame-driven, dies with the shell tree
      — no second async-timer convention next to the clipboard poll)
- [x] Two-lane refresh: per-frame `fold_engine_snapshots` (cheap
      mutex reads through the new `Scheduler::snapshot(task_id)`)
      folds engine progress/speed/pause into visible rows; the DB
      poll inside `refresh_if_active` is gated to ~2 Hz
      (`downloads_last_poll`) since `refresh` itself has no throttle
- [x] `refresh_if_active` busy set widened: QUEUED (so a resumed row
      keeps polling through the driver-loop admission),
      DOWNLOADING, PARSING, MERGING, CONVERTING, FFMPEG_QUEUED,
      ADDITIONAL_PROCESSING
- [x] Resume seeding: `run_task` builds `OffsetTable`s from
      `manager.current_files` (pending-first — the DB row lags the
      writer thread) and the new
      `Scheduler::spawn_engine_resumed(..., resume)` seeds the fresh
      engine via `resume_from` before spawning
- [x] Offset persistence: `run_task` folds
      `EngineControl::offset_tables()` into `task.Download.files`
      after the engine finishes (every exit path — Completed, Paused,
      Failed), and `TaskManager::update_async` preserves
      `chunk_offsets` from the pending snapshot instead of letting
      the sampler's in-memory task clobber them
- [x] Covers: `render_cover` caches decoded images in
      `ScreenState.covers` (`Option` = failed fetch, no per-frame
      retry), one-shot `window.spawn` fetch through
      `ParseService::client`, `image::guess_format` → gpui format;
      placeholder is a muted 72×60 rounded box. The fetch runs under
      `bootstrap::on_runtime(...).await` — gpui's executor is not a
      tokio context and reqwest panics there without a runtime. This
      replaced the original `tokio_context()` guard: holding an
      EnterGuard across an await on the gpui main thread aborts the
      process when two guarded futures overlap (see the EnterGuard
      fix below).
- [x] Regression test: `resumed_run_skips_already_downloaded_ranges`
      (range-counting CDN mock; second run must issue zero new ranged
      requests)
- [x] Smoke: app boots, window renders, no panics over 12 s
      (click-path pixel verification not automatable here — no
      vision model / pyobjc; see answer)

## Answer

The freeze was architectural, not a rendering bug: nothing connected
the running engine to the UI between DB polls, and nothing polled
while the screen sat open. The frame tick is the connective tissue —
engine snapshots fold per frame (mutex reads, coalesced by the render
rate), DB queries stay at 2 Hz.

The resume desync had two stacked causes: (1) the resume path never
seeded the engine, so resume meant "download everything again"; (2)
even with seeding, the persisted `files` map had empty
`chunk_offsets` — the engine's offset store merged into the pending
writer snapshot, but the 500 ms sampler's `update_async` replaced the
whole pending JSON with the in-memory task (empty tables), and
`mark_as_completed` then persisted that. Fix at both ends: fold
tables into the task at engine exit, and make `update_async` merge
`chunk_offsets` instead of overwriting.

Known-not-fixed (follow-ups, out of scope):

- `cancel_download` (model.rs) never calls `Scheduler::cancel`/
  `release` — the engine keeps running after a UI cancel
- `ADDITIONAL_PROCESSING` status is never set anywhere
- Crash-mid-download resume still loses offsets (sampler ticks only
  carry them once the engine exits cleanly)

## Follow-up fix (same issue): EnterGuard drop-order abort

Opening Parse History and clicking Parse aborted the app: two futures
each holding a `tokio_context()` EnterGuard overlapped on the gpui
main thread (startup `uname` fetch + the history parse) and dropped in
network-completion order; tokio asserts LIFO guard drops and aborts.
The guard convention itself was the bug — 13 call sites shared it.

Fix: `bootstrap::on_runtime(move || async move { ... }).await`
spawns the network block on the shared runtime and awaits the
`JoinHandle`, so no guard ever crosses an await on the gpui thread.
All 13 call sites converted (login flows, parse, uname/logout,
update check, settings, download options, cover fetch).
`tokio_context` is deleted. `BoxFuture` alias in `parsers` gained
`+ Send` (auto-parse now runs inside a spawned runtime task).
