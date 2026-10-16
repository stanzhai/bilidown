# 07: FFmpeg wrapper

**What to build:** FFmpeg subprocess wrapper: locate ffmpeg (bundled
`bundle/ffmpeg(.exe)` → system PATH → custom path), run commands with
`-progress pipe:1 -nostats`, parse progress, no-output watchdog kill,
CREATE_NO_WINDOW on Windows. Later muxing tickets call this.

**Blocked by:** 01.

**Status:** in-progress

- [ ] Resolution order from config; missing-binary error surfaced
- [ ] Command builder ports test_ffmpeg_command expectations verbatim
- [ ] Progress parsing from `-progress pipe:1` frames tested (unit: canned stdout lines)
- [ ] Watchdog kills a hung process (tested with a fake ffmpeg script fixture)
- [ ] Windows spawn flags present behind cfg (compiles on unix, tested via builder)

## Comments
