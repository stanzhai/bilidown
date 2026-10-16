# 07: FFmpeg wrapper

**What to build:** FFmpeg subprocess wrapper: locate ffmpeg (bundled
`bundle/ffmpeg(.exe)` → system PATH → custom path), run commands with
`-progress pipe:1 -nostats`, parse progress, no-output watchdog kill,
CREATE_NO_WINDOW on Windows. Later muxing tickets call this.

**Blocked by:** 01.

**Status:** resolved

- [x] Resolution order from config; missing-binary error surfaced
- [x] Command builder ports test_ffmpeg_command expectations verbatim
- [x] Progress parsing from `-progress pipe:1` frames tested (unit: canned stdout lines)
- [x] Watchdog kills a hung process (tested with a fake ffmpeg script fixture)
- [x] Windows spawn flags present behind cfg (compiles on unix, tested via builder)

## Answer

`crates/ffmpeg` (locate/command/progress/runner, commit `36029b5c`, merged with integration tip in `cebd1a67`): bundled→system→custom fallback resolution with `MISSING_MESSAGE` error, argv builder byte-identical to the Python build, `-progress pipe:1 -nostats` parsing (probed duration overrides preset, 99% cap), no-output watchdog (SIGTERM unix / taskkill Windows), CREATE_NO_WINDOW behind cfg. 42 external-behavior tests: argv vectors, canned progress lines, fake-ffmpeg fixtures for watchdog/exit-code/argv/stdin/cwd.

## Comments
