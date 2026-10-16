# 24: Settings screens

**What to build:** All settings groups from the Python app: appearance
(theme/accent/scaling/language), network (proxy, UA, area), download
(paths, parallelism, retry, speed limit, delete-after), ffmpeg
(bundled/system/custom), naming rules management, danmaku/subtitle
styles, about screen (version/license), update settings (channel,
skip-version, prerelease). Reads/writes via the config store.

**Blocked by:** 02, 19, 20.

**Status:** resolved

- [x] Every Python settings group present with the same controls
- [x] Edits persist through the config store atomically and apply live where applicable
- [x] Naming rule management CRUD with the 12 conventions
- [x] About screen shows app id/version line info

## Answer

Settings screen ported in `crates/app/src/ui/settings/` (commit 891fd0f7,
merged 8a241474): seven SettingPages mirroring the Python interface groups
(interface/behavior/download/additional/file-naming/advanced/updates) with
write-through atomic config saves, media/cover/embed cascades, MCP token row,
native download-path picker, and the naming-rules window (filter, CRUD,
reorder, save, 12-convention editor with preview + variable table). Model
logic in `model.rs` under 38 unit tests; sibling-ticket dialogs hang off the
`pending_dialog` seams (`danmaku_style`, `subtitle_style`, T21 keys).

## Comments
