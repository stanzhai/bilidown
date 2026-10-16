# 21: Download options dialog (media/additional/naming + preview)

**What to build:** The per-task download options dialog: media page
(quality/codec priorities, container), additional page (danmaku/
subtitle/cover/metadata toggles), naming page (rule selection, numbering
mode, Rule Builder), bottom filename preview — driven by the naming
engine so preview matches final output. Download-options model ports
test_download_options vectors.

**Blocked by:** 05, 12, 20.

**Status:** resolved

- [x] Three pages render with the same controls/options as Python
- [x] Quality/codec priority editors; codec fallback warning surfaced
- [x] Rule Builder chip editor composes Naming Rules
- [x] Bottom preview updates live and equals the engine's output
- [x] Options model serialization matches Python shape

## Answer

Ported `src/gui/dialog/download_options/` to `crates/app/src/ui/download_options/`
(model, view dialog, rule builder view-model, units, tests) hung off the
`ParseScreen::open_download_options_dialog` seam; commit `93f41b5e`, merge `af0ba977`.

## Comments
