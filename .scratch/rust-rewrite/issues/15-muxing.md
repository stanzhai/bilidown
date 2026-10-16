# 15: Muxing pipeline (merge, containers, mp3, covers, chapters)

**What to build:** Post-download processing via the FFmpeg wrapper:
DASH video+audio merge; legacy FLV multi-part concat; mp4/mkv container
choice; m4a→mp3 (libmp3lame); cover attach as attached_pic with optional
delete-after; chapter info embedded via ffmetadata.

**Blocked by:** 07, 13.

**Status:** ready-for-agent

- [ ] DASH merge command correct for both containers (fixtures assert args, fake ffmpeg)
- [ ] FLV concat path for multi-part legacy tasks
- [ ] m4a→mp3 conversion; cover attached_pic; delete-after honored
- [ ] Chapter metadata injected via ffmetadata when present
- [ ] Progress + watchdog flow through the wrapper into task state

## Comments
