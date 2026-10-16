# 21: Download options dialog (media/additional/naming + preview)

**What to build:** The per-task download options dialog: media page
(quality/codec priorities, container), additional page (danmaku/
subtitle/cover/metadata toggles), naming page (rule selection, numbering
mode, Rule Builder), bottom filename preview — driven by the naming
engine so preview matches final output. Download-options model ports
test_download_options vectors.

**Blocked by:** 05, 12, 20.

**Status:** ready-for-agent

- [ ] Three pages render with the same controls/options as Python
- [ ] Quality/codec priority editors; codec fallback warning surfaced
- [ ] Rule Builder chip editor composes Naming Rules
- [ ] Bottom preview updates live and equals the engine's output
- [ ] Options model serialization matches Python shape

## Comments
