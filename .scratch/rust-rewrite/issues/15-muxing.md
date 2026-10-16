# 15: Muxing pipeline (merge, containers, mp3, covers, chapters)

**What to build:** Post-download processing via the FFmpeg wrapper:
DASH video+audio merge; legacy FLV multi-part concat; mp4/mkv container
choice; m4a→mp3 (libmp3lame); cover attach as attached_pic with optional
delete-after; chapter info embedded via ffmetadata.

**Blocked by:** 07, 13.

**Status:** resolved

- [x] DASH merge command correct for both containers (fixtures assert args, fake ffmpeg)
- [x] FLV concat path for multi-part legacy tasks
- [x] m4a→mp3 conversion; cover attached_pic; delete-after honored
- [x] Chapter metadata injected via ffmetadata when present
- [x] Progress + watchdog flow through the wrapper into task state

## Answer

`crates/mux`: synchronous port of `merger.py` as `Merger::run()` (commit e3b66a23); fake-ffmpeg integration tests in `crates/mux/tests/mux.rs` cover all five boxes.

## Comments
