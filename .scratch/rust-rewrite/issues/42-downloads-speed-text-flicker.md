# 42: Downloads screen — speed-text flicker

**What broke:** The "下载速度" text in the downloads list flickered
at ~2 Hz — alternating between the real speed and blank. Any running
task's row lost its speed for a frame or two every poll cycle.

**Root cause:** Two writers raced for `task.Download.speed`:

1. The engine's `TaskState.speed` was **always 0** — the only
   assignment in `downloader/src/lib.rs` was `s.speed = 0` inside
   `recompute_downloaded()`. The engine never computes a rate.
2. `fold_engine_snapshots` (screen.rs) folded that snapshot into the
   row model **every frame**, unconditionally overwriting speed with
   0 between the sampler's DB writes.
3. The real speed came from the 500 ms sampler in
   `download_pipeline.rs` (byte delta × 2) via `update_async` → DB →
   the ~2 Hz poll.

Net effect: `status_text` rendered `format_speed(0)` → empty string
on most frames, real speed only on poll frames. Classic 0↔value
blink at the poll rate.

**Fix:** single owner + smoothing.

- `fold_engine_snapshots` no longer folds speed (downloaded_size /
  progress / paused folding kept — those are real engine facts).
  The doc comment now states why and links here.
- The sampler keeps exclusive ownership of speed and adds an EMA
  (`smooth = smooth * 0.6 + inst * 0.4`, first sample seeds) — the
  0.5 s instantaneous delta is noisier than the Python original's
  1 s QTimer display, so the value stops jumping digit-to-digit.
  Display concern only; no scheduling/limiting path reads it.
- Deleted the dead `TaskState.speed` field (zero readers — its only
  writer was the `= 0` reset) so nobody "folds engine speed" again.

Python parity: the original displayed the raw per-second value with
no smoothing; the Rust 0.5 s window doubles the noise, hence the EMA.

**Status:** resolved

- [x] `fold_engine_snapshots` speed folding removed (`screen.rs`)
- [x] Sampler EMA in `download_pipeline.rs`
- [x] `TaskState.speed` + `s.speed = 0` deleted (downloader)
- [x] `cargo check --workspace --all-targets`, `cargo test
      --workspace` (86 suites ok; one known-flaky ffmpeg-runner test
      passed on isolated reruns), `cargo clippy --workspace
      --all-targets` — only pre-existing warnings
- [x] UI smoke: launched the debug app, navigated Parser → Downloads
      via synthesized click (nav rail row 2 at logical (30, 120));
      downloads screen renders — tab strip with 下载任务 active and
      the empty-state text 没有正在下载的任务 confirmed by pixel
      analysis; no panic while on screen. Live speed stability is
      guaranteed by construction offline: the per-frame writer of 0
      is gone, the only remaining writer is the 500 ms EMA sampler.
